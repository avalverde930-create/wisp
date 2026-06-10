//! host-windows::capture_wgc — Windows.Graphics.Capture (WGC) capture path (ADR-0011 4b).
//!
//! Slice 4b.0 (this file): a **capability probe**. It stands up the full WGC init chain —
//! a D3D11 device, the WinRT `IDirect3DDevice`, the primary-monitor `GraphicsCaptureItem`,
//! and a free-threaded `Direct3D11CaptureFramePool` — to confirm WGC actually initializes on
//! this machine and to compile the interop surface, *before* the full capture pipeline (4b.1)
//! starts replacing the GDI grab. The GDI path (`capture`) is untouched; if WGC is
//! unavailable the host keeps using GDI, so there is no regression.

use anyhow::{Context, Result};
use windows::core::Interface;
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::{HMODULE, POINT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTOPRIMARY};
use windows::Win32::System::Com::CoIncrementMTAUsage;
use windows::Win32::System::WinRT::Direct3D11::CreateDirect3D11DeviceFromDXGIDevice;
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

/// A hardware D3D11 device plus its WinRT `IDirect3DDevice` projection (what WGC consumes).
pub struct WgcDevice {
    /// The raw D3D11 device. 4b.1 uses it (its immediate context) to copy each captured
    /// texture into a CPU-readable staging texture; the probe only needs `winrt`.
    #[allow(dead_code)]
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
    // Creating (then dropping) a free-threaded pool exercises the full device+item init path.
    let _pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &device.winrt,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        2,
        size,
    )
    .context("Direct3D11CaptureFramePool::CreateFreeThreaded")?;
    Ok((size.Width, size.Height))
}
