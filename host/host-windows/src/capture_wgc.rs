//! host-windows::capture_wgc — Windows.Graphics.Capture (WGC) capture path (ADR-0011 4b).
//!
//! 4b.0 was a capability probe; 4b.1 (this file) adds `WgcCapturer`: a free-threaded
//! `Direct3D11CaptureFramePool` whose frames (GPU `ID3D11Texture2D`s) are copied into a
//! CPU-readable staging texture and read out as tight BGRA8 — the same pixel format the
//! interframe encoder consumes. WGC captures at the monitor's *native* resolution (e.g. 4K)
//! vs GDI's DPI-scaled logical size, so until the hardware H.264 encoder (4c) lands, WGC is
//! opt-in (`WISP_CAPTURE=wgc`) and GDI stays the default — software-encoding native 4K would
//! cut fps. The GDI path is untouched, so there is no regression.

use anyhow::{Context, Result};
use windows::core::Interface;
use windows::Graphics::Capture::{
    Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::{HMODULE, POINT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTOPRIMARY};
use windows::Win32::System::Com::CoIncrementMTAUsage;
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

/// A hardware D3D11 device plus its WinRT `IDirect3DDevice` projection (what WGC consumes).
pub struct WgcDevice {
    pub d3d: ID3D11Device,
    pub winrt: IDirect3DDevice,
}

/// Ensure an MTA exists on this process so WinRT factory/activation calls succeed. Idempotent;
/// the returned cookie is intentionally leaked (keeps the MTA alive for the process lifetime).
fn ensure_mta() -> Result<()> {
    unsafe {
        CoIncrementMTAUsage().context("CoIncrementMTAUsage")?;
    }
    Ok(())
}

/// Create a BGRA-capable hardware D3D11 device and its WinRT `IDirect3DDevice` projection.
pub fn create_device() -> Result<WgcDevice> {
    unsafe {
        let mut d3d: Option<ID3D11Device> = None;
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut d3d),
            None,
            None,
        )
        .context("D3D11CreateDevice (hardware, BGRA)")?;
        let d3d = d3d.context("D3D11CreateDevice returned no device")?;
        let dxgi: IDXGIDevice = d3d.cast().context("cast ID3D11Device -> IDXGIDevice")?;
        let inspectable = CreateDirect3D11DeviceFromDXGIDevice(&dxgi)
            .context("CreateDirect3D11DeviceFromDXGIDevice")?;
        let winrt: IDirect3DDevice = inspectable
            .cast()
            .context("cast IInspectable -> IDirect3DDevice")?;
        Ok(WgcDevice { d3d, winrt })
    }
}

/// Build a `GraphicsCaptureItem` for the primary monitor via the WGC interop factory.
pub fn primary_capture_item() -> Result<GraphicsCaptureItem> {
    unsafe {
        let hmon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        anyhow::ensure!(
            !hmon.is_invalid(),
            "MonitorFromPoint found no primary monitor"
        );
        let interop: IGraphicsCaptureItemInterop =
            windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
                .context("GraphicsCaptureItem interop factory")?;
        interop.CreateForMonitor(hmon).context("CreateForMonitor")
    }
}

/// 4b.0 capability probe: confirm the whole WGC init chain works here. Returns the
/// WGC-reported primary-monitor size `(width, height)`.
pub fn probe() -> Result<(i32, i32)> {
    ensure_mta()?;
    anyhow::ensure!(
        GraphicsCaptureSession::IsSupported().context("GraphicsCaptureSession::IsSupported")?,
        "Windows.Graphics.Capture is not supported on this OS"
    );
    let device = create_device()?;
    let item = primary_capture_item()?;
    let size = item.Size().context("GraphicsCaptureItem::Size")?;
    let _pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &device.winrt,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        2,
        size,
    )
    .context("Direct3D11CaptureFramePool::CreateFreeThreaded")?;
    Ok((size.Width, size.Height))
}

/// A live WGC capture session for the primary monitor. Poll [`WgcCapturer::try_next`] for the
/// next BGRA8 frame; it returns `Ok(None)` when no new frame is buffered yet.
pub struct WgcCapturer {
    d3d: ID3D11Device,
    context: ID3D11DeviceContext,
    _item: GraphicsCaptureItem,
    _session: GraphicsCaptureSession,
    pool: Direct3D11CaptureFramePool,
    /// Reused CPU-readable staging texture (texture, width, height); recreated on a size change.
    staging: Option<(ID3D11Texture2D, u32, u32)>,
    size: (u32, u32),
}

impl WgcCapturer {
    pub fn new() -> Result<Self> {
        ensure_mta()?;
        anyhow::ensure!(
            GraphicsCaptureSession::IsSupported().context("GraphicsCaptureSession::IsSupported")?,
            "Windows.Graphics.Capture is not supported on this OS"
        );
        let device = create_device()?;
        let context = unsafe { device.d3d.GetImmediateContext() }.context("GetImmediateContext")?;
        let item = primary_capture_item()?;
        let size = item.Size().context("GraphicsCaptureItem::Size")?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &device.winrt,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            size,
        )
        .context("Direct3D11CaptureFramePool::CreateFreeThreaded")?;
        let session = pool
            .CreateCaptureSession(&item)
            .context("CreateCaptureSession")?;
        session.StartCapture().context("StartCapture")?;
        Ok(Self {
            d3d: device.d3d,
            context,
            _item: item,
            _session: session,
            pool,
            staging: None,
            size: (size.Width.max(0) as u32, size.Height.max(0) as u32),
        })
    }

    pub fn width(&self) -> u32 {
        self.size.0
    }
    pub fn height(&self) -> u32 {
        self.size.1
    }

    /// Poll for the next captured frame; `Ok(None)` if none is ready yet (WGC delivers a frame
    /// when the desktop composition changes — a fully static screen yields none).
    pub fn try_next(&mut self) -> Result<Option<(u32, u32, Vec<u8>)>> {
        // A null frame (no buffered frame) surfaces as an Err from windows-rs — treat as None.
        let frame = match self.pool.TryGetNextFrame() {
            Ok(f) => f,
            Err(_) => return Ok(None),
        };
        let out = unsafe { self.copy_frame_to_bgra(&frame)? };
        Ok(Some(out))
    }

    /// Copy a captured GPU texture into a CPU staging texture and read it out as tight BGRA8.
    unsafe fn copy_frame_to_bgra(
        &mut self,
        frame: &Direct3D11CaptureFrame,
    ) -> Result<(u32, u32, Vec<u8>)> {
        let surface = frame.Surface().context("frame surface")?;
        let access: IDirect3DDxgiInterfaceAccess =
            surface.cast().context("surface DXGI interface access")?;
        let src: ID3D11Texture2D = access
            .GetInterface()
            .context("GetInterface ID3D11Texture2D")?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        src.GetDesc(&mut desc);
        let (w, h) = (desc.Width, desc.Height);

        let need_new = match &self.staging {
            Some((_, sw, sh)) => *sw != w || *sh != h,
            None => true,
        };
        if need_new {
            let staging_desc = D3D11_TEXTURE2D_DESC {
                Width: w,
                Height: h,
                MipLevels: 1,
                ArraySize: 1,
                Format: desc.Format,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
            };
            let mut tex: Option<ID3D11Texture2D> = None;
            self.d3d
                .CreateTexture2D(&staging_desc, None, Some(&mut tex))
                .context("CreateTexture2D (staging)")?;
            self.staging = Some((tex.context("CreateTexture2D returned none")?, w, h));
        }
        let staging = self.staging.as_ref().unwrap().0.clone();

        self.context.CopyResource(&staging, &src);

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        self.context
            .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .context("Map staging texture")?;

        let row_bytes = (w as usize) * 4;
        let mut bgra = vec![0u8; row_bytes * h as usize];
        let src_ptr = mapped.pData as *const u8;
        for y in 0..h as usize {
            let s = src_ptr.add(y * mapped.RowPitch as usize);
            let d = bgra.as_mut_ptr().add(y * row_bytes);
            std::ptr::copy_nonoverlapping(s, d, row_bytes);
        }
        self.context.Unmap(&staging, 0);
        Ok((w, h, bgra))
    }
}
