//! host-windows::gpu_color — D3D11 Video Processor BGRA -> NV12 colour conversion on the GPU
//! (ADR-0011, the GPU zero-copy pipeline). This removes the per-frame CPU colour cost: WGC
//! already hands us a D3D11 BGRA texture, and a hardware video processor converts it to an
//! NV12 texture the QSV encoder can consume in D3D mode.
//!
//! 4d-gpu.0 (this file): a capability probe — create the `ID3D11VideoDevice`, a video-processor
//! enumerator for BGRA-in / NV12-out at the target size, confirm both formats are supported, and
//! create the processor. This de-risks the video-processor API surface before the full
//! convert + readback slice (4d-gpu.1).

use std::mem::ManuallyDrop;

use anyhow::{Context, Result};
use windows::core::Interface;
use windows::Win32::Foundation::BOOL;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11Texture2D, ID3D11VideoContext, ID3D11VideoDevice,
    ID3D11VideoProcessorEnumerator, ID3D11VideoProcessorInputView, ID3D11VideoProcessorOutputView,
    D3D11_BIND_RENDER_TARGET, D3D11_CPU_ACCESS_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ,
    D3D11_TEX2D_VPIV, D3D11_TEX2D_VPOV, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
    D3D11_USAGE_STAGING, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, D3D11_VIDEO_PROCESSOR_CONTENT_DESC,
    D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_INPUT, D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_OUTPUT,
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0,
    D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0,
    D3D11_VIDEO_PROCESSOR_STREAM, D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
    D3D11_VPIV_DIMENSION_TEXTURE2D, D3D11_VPOV_DIMENSION_TEXTURE2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
};

/// 4d-gpu.0 probe: confirm the GPU's video processor can convert BGRA -> NV12 at `width`x`height`.
pub fn probe(width: u32, height: u32) -> Result<String> {
    unsafe {
        let device = crate::capture_wgc::create_device()?;
        let video_device: ID3D11VideoDevice = device
            .d3d
            .cast()
            .context("cast ID3D11Device -> ID3D11VideoDevice")?;

        let rate = DXGI_RATIONAL {
            Numerator: 30,
            Denominator: 1,
        };
        let desc = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
            InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            InputFrameRate: rate,
            InputWidth: width,
            InputHeight: height,
            OutputFrameRate: rate,
            OutputWidth: width,
            OutputHeight: height,
            Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
        };
        let enumerator: ID3D11VideoProcessorEnumerator = video_device
            .CreateVideoProcessorEnumerator(&desc)
            .context("CreateVideoProcessorEnumerator")?;

        let bgra = enumerator
            .CheckVideoProcessorFormat(DXGI_FORMAT_B8G8R8A8_UNORM)
            .context("CheckVideoProcessorFormat (BGRA)")?;
        let nv12 = enumerator
            .CheckVideoProcessorFormat(DXGI_FORMAT_NV12)
            .context("CheckVideoProcessorFormat (NV12)")?;
        let in_ok = bgra & D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_INPUT.0 as u32 != 0;
        let out_ok = nv12 & D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_OUTPUT.0 as u32 != 0;

        let _processor = video_device
            .CreateVideoProcessor(&enumerator, 0)
            .context("CreateVideoProcessor")?;

        // Create the input (BGRA) and output (NV12) textures + their video-processor views — the
        // surfaces a VideoProcessorBlt converts between (4d-gpu.1b does the Blt + readback).
        let in_tex = create_texture(&device.d3d, width, height, DXGI_FORMAT_B8G8R8A8_UNORM, 0)?;
        let out_tex = create_texture(
            &device.d3d,
            width,
            height,
            DXGI_FORMAT_NV12,
            D3D11_BIND_RENDER_TARGET.0 as u32,
        )?;

        let in_view_desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
            FourCC: 0,
            ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPIV {
                    MipSlice: 0,
                    ArraySlice: 0,
                },
            },
        };
        let mut in_view: Option<ID3D11VideoProcessorInputView> = None;
        video_device
            .CreateVideoProcessorInputView(&in_tex, &enumerator, &in_view_desc, Some(&mut in_view))
            .context("CreateVideoProcessorInputView")?;

        let out_view_desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
            ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
            },
        };
        let mut out_view: Option<ID3D11VideoProcessorOutputView> = None;
        video_device
            .CreateVideoProcessorOutputView(
                &out_tex,
                &enumerator,
                &out_view_desc,
                Some(&mut out_view),
            )
            .context("CreateVideoProcessorOutputView")?;

        let views_ok = in_view.is_some() && out_view.is_some();
        Ok(format!(
            "processor + textures + views created (views_ok={views_ok}); BGRA input support={in_ok}, NV12 output support={out_ok}"
        ))
    }
}

/// Create a `width`x`height` texture of `format` with the given bind flags (default usage,
/// no CPU access). For the GPU colour-conversion input/output surfaces.
unsafe fn create_texture(
    d3d: &ID3D11Device,
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
    bind_flags: u32,
) -> Result<ID3D11Texture2D> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: bind_flags,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut tex: Option<ID3D11Texture2D> = None;
    d3d.CreateTexture2D(&desc, None, Some(&mut tex))
        .context("CreateTexture2D")?;
    tex.context("CreateTexture2D returned none")
}

/// Convert a BGRA8 frame to NV12 on the GPU via the D3D11 Video Processor, reading the result
/// back to a tight NV12 byte vec. This is the one-shot bring-up path (it builds the whole
/// pipeline per call); the live pipeline keeps the textures resident and feeds QSV's D3D mode.
pub fn convert_bgra_to_nv12(bgra: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
    unsafe {
        let device = crate::capture_wgc::create_device()?;
        let d3d = &device.d3d;
        let context = d3d.GetImmediateContext().context("GetImmediateContext")?;
        let video_device: ID3D11VideoDevice = d3d.cast().context("ID3D11VideoDevice")?;
        let video_context: ID3D11VideoContext = context.cast().context("ID3D11VideoContext")?;

        let rate = DXGI_RATIONAL {
            Numerator: 30,
            Denominator: 1,
        };
        let desc = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
            InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            InputFrameRate: rate,
            InputWidth: width,
            InputHeight: height,
            OutputFrameRate: rate,
            OutputWidth: width,
            OutputHeight: height,
            Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
        };
        let enumerator = video_device
            .CreateVideoProcessorEnumerator(&desc)
            .context("CreateVideoProcessorEnumerator")?;
        let processor = video_device
            .CreateVideoProcessor(&enumerator, 0)
            .context("CreateVideoProcessor")?;

        // Input BGRA texture (upload the frame) + output NV12 render target.
        let in_tex = create_texture(d3d, width, height, DXGI_FORMAT_B8G8R8A8_UNORM, 0)?;
        context.UpdateSubresource(
            &in_tex,
            0,
            None,
            bgra.as_ptr() as *const core::ffi::c_void,
            width * 4,
            0,
        );
        let out_tex = create_texture(
            d3d,
            width,
            height,
            DXGI_FORMAT_NV12,
            D3D11_BIND_RENDER_TARGET.0 as u32,
        )?;

        let in_view_desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
            FourCC: 0,
            ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPIV {
                    MipSlice: 0,
                    ArraySlice: 0,
                },
            },
        };
        let mut in_view: Option<ID3D11VideoProcessorInputView> = None;
        video_device
            .CreateVideoProcessorInputView(&in_tex, &enumerator, &in_view_desc, Some(&mut in_view))
            .context("CreateVideoProcessorInputView")?;
        let in_view = in_view.context("null input view")?;

        let out_view_desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
            ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
            },
        };
        let mut out_view: Option<ID3D11VideoProcessorOutputView> = None;
        video_device
            .CreateVideoProcessorOutputView(
                &out_tex,
                &enumerator,
                &out_view_desc,
                Some(&mut out_view),
            )
            .context("CreateVideoProcessorOutputView")?;
        let out_view = out_view.context("null output view")?;

        // Convert.
        let mut stream = [D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: BOOL(1),
            OutputIndex: 0,
            InputFrameOrField: 0,
            PastFrames: 0,
            FutureFrames: 0,
            ppPastSurfaces: std::ptr::null_mut(),
            pInputSurface: ManuallyDrop::new(Some(in_view)),
            ppFutureSurfaces: std::ptr::null_mut(),
            ppPastSurfacesRight: std::ptr::null_mut(),
            pInputSurfaceRight: ManuallyDrop::new(None),
            ppFutureSurfacesRight: std::ptr::null_mut(),
        }];
        let blt = video_context.VideoProcessorBlt(&processor, &out_view, 0, &stream);
        ManuallyDrop::drop(&mut stream[0].pInputSurface);
        ManuallyDrop::drop(&mut stream[0].pInputSurfaceRight);
        blt.context("VideoProcessorBlt")?;

        // Read the NV12 output back via a staging copy.
        let staging = {
            let sdesc = D3D11_TEXTURE2D_DESC {
                Width: width,
                Height: height,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_NV12,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
            };
            let mut t: Option<ID3D11Texture2D> = None;
            d3d.CreateTexture2D(&sdesc, None, Some(&mut t))
                .context("CreateTexture2D (staging)")?;
            t.context("staging none")?
        };
        context.CopyResource(&staging, &out_tex);

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        context
            .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .context("Map staging NV12")?;
        let (w, h) = (width as usize, height as usize);
        let pitch = mapped.RowPitch as usize;
        let src = mapped.pData as *const u8;
        let mut nv12 = vec![0u8; wisp_core::color::nv12_len(width, height)];
        // Y plane: h rows of `w` bytes (mapped at `pitch` stride).
        for y in 0..h {
            let s = src.add(y * pitch);
            std::ptr::copy_nonoverlapping(s, nv12.as_mut_ptr().add(y * w), w);
        }
        // UV plane: starts after the Y plane (pitch * h); h/2 rows of `w` bytes.
        let uv_base = pitch * h;
        for cy in 0..h / 2 {
            let s = src.add(uv_base + cy * pitch);
            std::ptr::copy_nonoverlapping(s, nv12.as_mut_ptr().add(w * h + cy * w), w);
        }
        context.Unmap(&staging, 0);
        Ok(nv12)
    }
}

/// 4d-gpu.1b self-test: convert a synthetic gradient BGRA frame to NV12 on the GPU, convert it
/// back to BGRA on the CPU, and return the mean abs error vs the original. A working conversion
/// lands well under a garbage threshold (colour-space pinning is a later refinement, so some
/// error from the GPU's default matrix is expected).
pub fn selftest(width: u32, height: u32) -> Result<f64> {
    let mut bgra = vec![0u8; (width * height * 4) as usize];
    for y in 0..height as usize {
        for x in 0..width as usize {
            let i = (y * width as usize + x) * 4;
            bgra[i] = (x % 256) as u8;
            bgra[i + 1] = (y % 256) as u8;
            bgra[i + 2] = ((x + y) % 256) as u8;
            bgra[i + 3] = 255;
        }
    }
    let nv12 = convert_bgra_to_nv12(&bgra, width, height)?;
    let back = wisp_core::color::nv12_to_bgra(&nv12, width, height);
    let mut sum = 0u64;
    for (a, b) in bgra.chunks(4).zip(back.chunks(4)) {
        for c in 0..3 {
            sum += (a[c] as i32 - b[c] as i32).unsigned_abs() as u64;
        }
    }
    let mae = sum as f64 / (width as f64 * height as f64 * 3.0);
    // A correct conversion lands near the CPU round-trip error; garbage (wrong plane/stride/
    // colour space) is ~85+. Generous threshold absorbs the GPU's exact matrix.
    anyhow::ensure!(
        mae < 25.0,
        "GPU colour round-trip MAE {mae:.2} too high — conversion likely wrong"
    );
    Ok(mae)
}
