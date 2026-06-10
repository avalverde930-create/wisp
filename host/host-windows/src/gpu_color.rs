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
    ID3D11VideoDevice, ID3D11VideoProcessorEnumerator, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
    D3D11_VIDEO_PROCESSOR_CONTENT_DESC, D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_INPUT,
    D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_OUTPUT, D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12, DXGI_RATIONAL,
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

        Ok(format!(
            "video processor created; BGRA input support={in_ok}, NV12 output support={out_ok}"
        ))
    }
}
