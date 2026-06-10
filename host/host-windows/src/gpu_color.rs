//! host-windows::gpu_color — D3D11 Video Processor BGRA -> NV12 colour conversion on the GPU
//! (ADR-0011, the GPU zero-copy pipeline). This removes the per-frame CPU colour cost: WGC
//! already hands us a D3D11 BGRA texture, and a hardware video processor converts it to an
//! NV12 texture the QSV encoder can consume in D3D mode.
//!
//! 4d-gpu.0 (this file): a capability probe — create the `ID3D11VideoDevice`, a video-processor
//! enumerator for BGRA-in / NV12-out at the target size, confirm both formats are supported, and
//! create the processor. This de-risks the video-processor API surface before the full
//! convert + readback slice (4d-gpu.1).

use anyhow::{Context, Result};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Texture2D, ID3D11VideoDevice, ID3D11VideoProcessorEnumerator,
    ID3D11VideoProcessorInputView, ID3D11VideoProcessorOutputView, D3D11_BIND_RENDER_TARGET,
    D3D11_TEX2D_VPIV, D3D11_TEX2D_VPOV, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
    D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, D3D11_VIDEO_PROCESSOR_CONTENT_DESC,
    D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_INPUT, D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_OUTPUT,
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0,
    D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0,
    D3D11_VIDEO_USAGE_PLAYBACK_NORMAL, D3D11_VPIV_DIMENSION_TEXTURE2D,
    D3D11_VPOV_DIMENSION_TEXTURE2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
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
    d3d: &windows::Win32::Graphics::Direct3D11::ID3D11Device,
    width: u32,
    height: u32,
    format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT,
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
