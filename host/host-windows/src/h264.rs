//! host-windows::h264 — hardware H.264 encode via Media Foundation (ADR-0011 4c).
//!
//! 4c.0 (this file): a **capability probe**. It enumerates the H.264 *encoder* MFTs on this
//! machine — hardware async MFTs (NVENC / Quick Sync / AMF) and the Microsoft software
//! encoder floor — via `MFTEnumEx`, and prints their friendly names. This tells us which
//! encoder 4c.1 should instantiate, and compiles the Media Foundation interop surface before
//! the real encoder is built. Encoding does not touch the default GDI/interframe path, so
//! there is no regression.

use std::mem::ManuallyDrop;

use anyhow::{Context, Result};
use windows::core::PWSTR;
use windows::Win32::Media::MediaFoundation::{
    eAVEncH264VProfile_Main, IMFActivate, IMFTransform, MFCreateMediaType, MFCreateMemoryBuffer,
    MFCreateSample, MFMediaType_Video, MFShutdown, MFStartup, MFTEnumEx,
    MFT_FRIENDLY_NAME_Attribute, MFVideoFormat_H264, MFVideoFormat_NV12,
    MFVideoInterlace_Progressive, MFSTARTUP_FULL, MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG,
    MFT_ENUM_FLAG_ASYNCMFT, MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_SORTANDFILTER,
    MFT_ENUM_FLAG_SYNCMFT, MFT_ENUM_FLAG_TRANSCODE_ONLY, MFT_MESSAGE_COMMAND_DRAIN,
    MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER,
    MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MFT_REGISTER_TYPE_INFO, MF_E_TRANSFORM_NEED_MORE_INPUT,
    MF_MT_AVG_BITRATE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE,
    MF_MT_MPEG2_PROFILE, MF_MT_SUBTYPE, MF_VERSION,
};
use windows::Win32::System::Com::{CoIncrementMTAUsage, CoTaskMemFree};

/// Pack two u32 into the u64 layout Media Foundation uses for size/ratio attributes
/// (`MF_MT_FRAME_SIZE`, `MF_MT_FRAME_RATE`): high u32 = first value, low u32 = second.
fn pack_u32x2(hi: u32, lo: u32) -> u64 {
    ((hi as u64) << 32) | lo as u64
}

/// The friendly name of an MFT activation object (`MFT_FRIENDLY_NAME_Attribute`).
unsafe fn friendly_name(act: &IMFActivate) -> Option<String> {
    let mut pw = PWSTR::null();
    let mut len = 0u32;
    act.GetAllocatedString(&MFT_FRIENDLY_NAME_Attribute, &mut pw, &mut len)
        .ok()?;
    let name = pw.to_string().ok();
    if !pw.is_null() {
        CoTaskMemFree(Some(pw.0 as *const _));
    }
    name
}

/// Enumerate H.264 video-encoder MFTs matching `flags`; returns their friendly names.
unsafe fn enumerate(flags: MFT_ENUM_FLAG) -> Result<Vec<String>> {
    let output = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_H264,
    };
    let mut activates: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0u32;
    MFTEnumEx(
        MFT_CATEGORY_VIDEO_ENCODER,
        flags,
        None,
        Some(&output),
        &mut activates,
        &mut count,
    )
    .context("MFTEnumEx (video encoder, H.264)")?;

    let mut names = Vec::new();
    for i in 0..count as usize {
        // Take ownership of each activation object so it is Released on drop.
        let slot: Option<IMFActivate> = std::ptr::read(activates.add(i));
        if let Some(act) = slot {
            if let Some(n) = friendly_name(&act) {
                names.push(n);
            }
        }
    }
    if !activates.is_null() {
        CoTaskMemFree(Some(activates as *const _));
    }
    Ok(names)
}

/// 4c.0 capability probe: list the H.264 hardware + software encoder MFTs available here.
pub fn probe() -> Result<()> {
    unsafe {
        CoIncrementMTAUsage().context("CoIncrementMTAUsage")?;
        MFStartup(MF_VERSION, MFSTARTUP_FULL).context("MFStartup")?;

        let hardware = enumerate(
            MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_ASYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER,
        )?;
        let software = enumerate(
            MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_TRANSCODE_ONLY | MFT_ENUM_FLAG_SORTANDFILTER,
        )?;

        let _ = MFShutdown();

        println!("[host] H.264 hardware encoders ({}):", hardware.len());
        for n in &hardware {
            println!("[host]   - {n}");
        }
        if hardware.is_empty() {
            println!("[host]   (none found — 4c would use the software encoder floor)");
        }
        println!("[host] H.264 software encoders ({}):", software.len());
        for n in &software {
            println!("[host]   - {n}");
        }
    }
    Ok(())
}

/// Activate the first H.264 *software* encoder MFT (synchronous; the async hardware QSV path
/// is a later slice). Returns its `IMFTransform`.
unsafe fn software_encoder_transform() -> Result<IMFTransform> {
    let output = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_H264,
    };
    let mut activates: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0u32;
    MFTEnumEx(
        MFT_CATEGORY_VIDEO_ENCODER,
        MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_TRANSCODE_ONLY | MFT_ENUM_FLAG_SORTANDFILTER,
        None,
        Some(&output),
        &mut activates,
        &mut count,
    )
    .context("MFTEnumEx (software H.264 encoder)")?;
    anyhow::ensure!(
        count > 0 && !activates.is_null(),
        "no software H.264 encoder MFT"
    );

    let first: Option<IMFActivate> = std::ptr::read(activates);
    for i in 1..count as usize {
        let _drop: Option<IMFActivate> = std::ptr::read(activates.add(i)); // release the rest
    }
    CoTaskMemFree(Some(activates as *const _));

    let act = first.context("null IMFActivate")?;
    act.ActivateObject::<IMFTransform>()
        .context("ActivateObject IMFTransform")
}

/// A synchronous H.264 encoder MFT configured for NV12 input. Feed NV12 frames with
/// [`H264Encoder::encode`]; flush the tail with [`H264Encoder::drain`]. Output is the raw
/// H.264 elementary stream (Annex-B) produced by the MFT.
pub struct H264Encoder {
    transform: IMFTransform,
    out_size: usize,
    time: i64,
    frame_duration: i64,
}

impl H264Encoder {
    /// Build + configure the Microsoft software H.264 encoder (NV12 in, H.264 out).
    pub fn new_software(width: u32, height: u32, fps: u32, bitrate: u32) -> Result<Self> {
        unsafe {
            CoIncrementMTAUsage().context("CoIncrementMTAUsage")?;
            MFStartup(MF_VERSION, MFSTARTUP_FULL).context("MFStartup")?;
            let transform = software_encoder_transform()?;

            // Output type MUST be set before the input type for an H.264 encoder MFT.
            let out = MFCreateMediaType().context("MFCreateMediaType (output)")?;
            out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            out.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
            out.SetUINT32(&MF_MT_AVG_BITRATE, bitrate)?;
            out.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            out.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_Main.0 as u32)?;
            out.SetUINT64(&MF_MT_FRAME_SIZE, pack_u32x2(width, height))?;
            out.SetUINT64(&MF_MT_FRAME_RATE, pack_u32x2(fps, 1))?;
            transform
                .SetOutputType(0, &out, 0)
                .context("SetOutputType")?;

            // Input type: NV12.
            let inp = MFCreateMediaType().context("MFCreateMediaType (input)")?;
            inp.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            inp.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
            inp.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            inp.SetUINT64(&MF_MT_FRAME_SIZE, pack_u32x2(width, height))?;
            inp.SetUINT64(&MF_MT_FRAME_RATE, pack_u32x2(fps, 1))?;
            transform.SetInputType(0, &inp, 0).context("SetInputType")?;

            let info = transform
                .GetOutputStreamInfo(0)
                .context("GetOutputStreamInfo")?;
            anyhow::ensure!(
                info.dwFlags & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32) == 0,
                "software encoder unexpectedly provides its own samples"
            );
            let out_size = (info.cbSize as usize).max((width * height * 3 / 2) as usize);

            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;

            Ok(Self {
                transform,
                out_size,
                time: 0,
                frame_duration: (10_000_000i64) / fps.max(1) as i64,
            })
        }
    }

    /// Encode one NV12 frame; returns any H.264 bytes the encoder emitted (may be empty while
    /// it buffers — emitted bytes arrive on a later `encode`/`drain`).
    pub fn encode(&mut self, nv12: &[u8]) -> Result<Vec<u8>> {
        unsafe {
            let buf = MFCreateMemoryBuffer(nv12.len() as u32).context("MFCreateMemoryBuffer")?;
            let mut ptr = std::ptr::null_mut();
            buf.Lock(&mut ptr, None, None).context("buffer Lock")?;
            std::ptr::copy_nonoverlapping(nv12.as_ptr(), ptr, nv12.len());
            buf.Unlock().ok();
            buf.SetCurrentLength(nv12.len() as u32)?;

            let sample = MFCreateSample().context("MFCreateSample")?;
            sample.AddBuffer(&buf)?;
            sample.SetSampleTime(self.time)?;
            sample.SetSampleDuration(self.frame_duration)?;
            self.time += self.frame_duration;

            self.transform
                .ProcessInput(0, &sample, 0)
                .context("ProcessInput")?;
            self.collect_output()
        }
    }

    /// Flush the encoder; returns the remaining buffered H.264 bytes.
    pub fn drain(&mut self) -> Result<Vec<u8>> {
        unsafe {
            self.transform
                .ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)
                .context("DRAIN")?;
            self.collect_output()
        }
    }

    /// Pull every currently-available output sample (until the MFT asks for more input).
    unsafe fn collect_output(&mut self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        loop {
            let out_sample = MFCreateSample().context("MFCreateSample (out)")?;
            let out_buf =
                MFCreateMemoryBuffer(self.out_size as u32).context("MFCreateMemoryBuffer (out)")?;
            out_sample.AddBuffer(&out_buf)?;

            let mut data = [MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: ManuallyDrop::new(Some(out_sample.clone())),
                dwStatus: 0,
                pEvents: ManuallyDrop::new(None),
            }];
            let mut status = 0u32;
            let r = self.transform.ProcessOutput(0, &mut data, &mut status);
            ManuallyDrop::drop(&mut data[0].pSample);
            ManuallyDrop::drop(&mut data[0].pEvents);

            match r {
                Ok(()) => {
                    let mut ptr = std::ptr::null_mut();
                    let mut len = 0u32;
                    out_buf
                        .Lock(&mut ptr, None, Some(&mut len))
                        .context("out buffer Lock")?;
                    out.extend_from_slice(std::slice::from_raw_parts(ptr, len as usize));
                    out_buf.Unlock().ok();
                }
                Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => break,
                Err(e) => return Err(e).context("ProcessOutput"),
            }
        }
        Ok(out)
    }
}

/// 4c.1b self-test: synthesize a gradient frame, convert BGRA -> NV12 (`core::color`), encode a
/// few frames through the software MFT + drain, and return the H.264 elementary-stream bytes.
pub fn selftest(width: u32, height: u32) -> Result<Vec<u8>> {
    let mut enc = H264Encoder::new_software(width, height, 30, 8_000_000)?;
    let mut bgra = vec![0u8; (width * height * 4) as usize];
    for y in 0..height as usize {
        for x in 0..width as usize {
            let i = (y * width as usize + x) * 4;
            bgra[i] = (x % 256) as u8; // B
            bgra[i + 1] = (y % 256) as u8; // G
            bgra[i + 2] = ((x + y) % 256) as u8; // R
            bgra[i + 3] = 255;
        }
    }
    let nv12 = wisp_core::color::bgra_to_nv12(&bgra, width, height);
    let mut nal = Vec::new();
    for _ in 0..3 {
        nal.extend(enc.encode(&nv12)?);
    }
    nal.extend(enc.drain()?);
    Ok(nal)
}
