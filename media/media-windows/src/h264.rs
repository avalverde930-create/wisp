//! host-windows::h264 — H.264 encode/decode via Media Foundation (ADR-0011 4c).
//!
//! - `probe` (4c.0): enumerate the H.264 encoder MFTs (hardware NVENC/QSV/AMF + software floor).
//! - `H264Encoder` (4c.1b): the synchronous Microsoft software H.264 encoder (NV12 in, H.264 out).
//! - `H264Decoder` (4c.2): the synchronous Microsoft H.264 decoder (H.264 in, NV12 out).
//!
//! This is bring-up: keeping the decoder beside the encoder lets `selftest` round-trip
//! (encode -> decode -> compare) on one machine. 4c.3 relocates the codec to a shared/client
//! home and wires `FrameCodec::HwH264` into the pipeline. Nothing here touches the default
//! GDI/interframe capture path, so there is no regression. Async hardware QSV is a later slice.

use std::mem::ManuallyDrop;

use anyhow::{Context, Result};
use windows::core::PWSTR;
use windows::Win32::Media::MediaFoundation::{
    eAVEncH264VProfile_Main, IMFActivate, IMFSample, IMFTransform, MFCreateMediaType,
    MFCreateMemoryBuffer, MFCreateSample, MFMediaType_Video, MFShutdown, MFStartup, MFTEnumEx,
    MFT_FRIENDLY_NAME_Attribute, MFVideoFormat_H264, MFVideoFormat_NV12,
    MFVideoInterlace_Progressive, MFSTARTUP_FULL, MFT_CATEGORY_VIDEO_DECODER,
    MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG, MFT_ENUM_FLAG_ASYNCMFT, MFT_ENUM_FLAG_HARDWARE,
    MFT_ENUM_FLAG_SORTANDFILTER, MFT_ENUM_FLAG_SYNCMFT, MFT_ENUM_FLAG_TRANSCODE_ONLY,
    MFT_MESSAGE_COMMAND_DRAIN, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
    MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES,
    MFT_REGISTER_TYPE_INFO, MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE,
    MF_E_TRANSFORM_TYPE_NOT_SET, MF_LOW_LATENCY, MF_MT_AVG_BITRATE, MF_MT_FRAME_RATE,
    MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_MPEG2_PROFILE, MF_MT_SUBTYPE,
    MF_VERSION,
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

            // Request low-latency mode (no B-frames / lookahead) so each input frame yields its
            // output promptly — essential for real-time streaming. Best-effort.
            if let Ok(attrs) = transform.GetAttributes() {
                let _ = attrs.SetUINT32(&MF_LOW_LATENCY, 1);
            }

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

/// Activate the first synchronous H.264 *decoder* MFT (H.264 in). Returns its `IMFTransform`.
unsafe fn software_decoder_transform() -> Result<IMFTransform> {
    let input = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_H264,
    };
    let mut activates: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0u32;
    MFTEnumEx(
        MFT_CATEGORY_VIDEO_DECODER,
        MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER,
        Some(&input),
        None,
        &mut activates,
        &mut count,
    )
    .context("MFTEnumEx (H.264 decoder)")?;
    anyhow::ensure!(count > 0 && !activates.is_null(), "no H.264 decoder MFT");

    let first: Option<IMFActivate> = std::ptr::read(activates);
    for i in 1..count as usize {
        let _drop: Option<IMFActivate> = std::ptr::read(activates.add(i));
    }
    CoTaskMemFree(Some(activates as *const _));

    first
        .context("null IMFActivate")?
        .ActivateObject::<IMFTransform>()
        .context("ActivateObject IMFTransform (decoder)")
}

/// Read an `IMFSample`'s single buffer out as a contiguous byte vec.
unsafe fn sample_bytes(sample: &IMFSample) -> Result<Vec<u8>> {
    let buf = sample
        .ConvertToContiguousBuffer()
        .context("ConvertToContiguousBuffer")?;
    let mut ptr = std::ptr::null_mut();
    let mut len = 0u32;
    buf.Lock(&mut ptr, None, Some(&mut len)).context("Lock")?;
    let v = std::slice::from_raw_parts(ptr, len as usize).to_vec();
    buf.Unlock().ok();
    Ok(v)
}

/// A synchronous H.264 decoder MFT producing NV12. Feed an H.264 elementary stream to
/// [`H264Decoder::decode`]; it returns the decoded NV12 frames.
pub struct H264Decoder {
    transform: IMFTransform,
    width: u32,
    height: u32,
    out_size: usize,
    provides_samples: bool,
    output_set: bool,
}

impl H264Decoder {
    pub fn new_software(width: u32, height: u32, fps: u32) -> Result<Self> {
        unsafe {
            CoIncrementMTAUsage().context("CoIncrementMTAUsage")?;
            MFStartup(MF_VERSION, MFSTARTUP_FULL).context("MFStartup")?;
            let transform = software_decoder_transform()?;

            let inp = MFCreateMediaType().context("MFCreateMediaType (decoder input)")?;
            inp.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            inp.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
            inp.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            inp.SetUINT64(&MF_MT_FRAME_SIZE, pack_u32x2(width, height))?;
            inp.SetUINT64(&MF_MT_FRAME_RATE, pack_u32x2(fps, 1))?;
            transform.SetInputType(0, &inp, 0).context("SetInputType")?;

            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;

            Ok(Self {
                transform,
                width,
                height,
                out_size: (width * height * 3 / 2) as usize,
                provides_samples: false,
                output_set: false,
            })
        }
    }

    /// Negotiate the NV12 output type (the decoder announces it via MF_E_TRANSFORM_STREAM_CHANGE
    /// once it has parsed the stream's SPS).
    unsafe fn set_output_type(&mut self) -> Result<()> {
        let t = self
            .transform
            .GetOutputAvailableType(0, 0)
            .context("GetOutputAvailableType")?;
        t.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12).ok();
        self.transform
            .SetOutputType(0, &t, 0)
            .context("SetOutputType (decoder)")?;
        let info = self
            .transform
            .GetOutputStreamInfo(0)
            .context("GetOutputStreamInfo (decoder)")?;
        self.provides_samples = info.dwFlags & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32) != 0;
        self.out_size = (info.cbSize as usize).max((self.width * self.height * 3 / 2) as usize);
        self.output_set = true;
        Ok(())
    }

    /// Decode an H.264 elementary stream; returns the decoded NV12 frames.
    pub fn decode(&mut self, h264: &[u8]) -> Result<Vec<Vec<u8>>> {
        unsafe {
            let buf = MFCreateMemoryBuffer(h264.len() as u32).context("MFCreateMemoryBuffer")?;
            let mut ptr = std::ptr::null_mut();
            buf.Lock(&mut ptr, None, None).context("Lock")?;
            std::ptr::copy_nonoverlapping(h264.as_ptr(), ptr, h264.len());
            buf.Unlock().ok();
            buf.SetCurrentLength(h264.len() as u32)?;
            let sample = MFCreateSample().context("MFCreateSample")?;
            sample.AddBuffer(&buf)?;
            sample.SetSampleTime(0)?;
            self.transform
                .ProcessInput(0, &sample, 0)
                .context("ProcessInput (decoder)")?;

            let mut frames = self.collect_output()?;
            self.transform
                .ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)
                .context("DRAIN")?;
            frames.extend(self.collect_output()?);
            Ok(frames)
        }
    }

    unsafe fn collect_output(&mut self) -> Result<Vec<Vec<u8>>> {
        let mut frames = Vec::new();
        let mut renegotiations = 0;
        loop {
            // Allocate an output sample unless the MFT provides its own.
            let kept = if self.provides_samples {
                None
            } else {
                let os = MFCreateSample()?;
                let ob = MFCreateMemoryBuffer(self.out_size as u32)?;
                os.AddBuffer(&ob)?;
                Some(os)
            };
            let mut data = [MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: ManuallyDrop::new(kept.clone()),
                dwStatus: 0,
                pEvents: ManuallyDrop::new(None),
            }];
            let mut status = 0u32;
            let r = self.transform.ProcessOutput(0, &mut data, &mut status);
            let produced = ManuallyDrop::take(&mut data[0].pSample);
            ManuallyDrop::drop(&mut data[0].pEvents);

            match r {
                Ok(()) => {
                    if let Some(s) = produced.or(kept) {
                        frames.push(sample_bytes(&s)?);
                    }
                }
                // The decoder announces its NV12 output type via STREAM_CHANGE, or asks for it
                // up front via TYPE_NOT_SET; in both cases (re)negotiate and retry.
                Err(e)
                    if e.code() == MF_E_TRANSFORM_STREAM_CHANGE
                        || e.code() == MF_E_TRANSFORM_TYPE_NOT_SET =>
                {
                    renegotiations += 1;
                    anyhow::ensure!(
                        renegotiations <= 4,
                        "decoder output-type negotiation did not converge"
                    );
                    self.set_output_type()?;
                }
                Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => break,
                Err(e) => return Err(e).context("ProcessOutput (decoder)"),
            }
        }
        Ok(frames)
    }
}

/// 4c.2 self-test: synthesize a gradient, encode it (BGRA -> NV12 -> H.264), decode it back to
/// NV12, convert to BGRA, and report the encoded size, decoded frame count, and mean abs error
/// vs the original (H.264 is lossy, so a small non-zero MAE is expected).
pub struct SelfTest {
    pub encoded_bytes: usize,
    pub start_code: bool,
    pub decoded_frames: usize,
    pub mean_abs_error: f64,
}

pub fn selftest(width: u32, height: u32) -> Result<SelfTest> {
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

    let mut enc = H264Encoder::new_software(width, height, 30, 8_000_000)?;
    let mut nal = Vec::new();
    for _ in 0..3 {
        nal.extend(enc.encode(&nv12)?);
    }
    nal.extend(enc.drain()?);
    let start_code = nal.windows(4).any(|w| w == [0, 0, 0, 1]);

    let mut dec = H264Decoder::new_software(width, height, 30)?;
    let frames = dec.decode(&nal)?;
    anyhow::ensure!(!frames.is_empty(), "decoder produced no frames");

    let expected = wisp_core::color::nv12_len(width, height);
    let first = &frames[0];
    anyhow::ensure!(
        first.len() >= expected,
        "decoded NV12 {} < expected {} (stride padding?)",
        first.len(),
        expected
    );
    let dec_bgra = wisp_core::color::nv12_to_bgra(&first[..expected], width, height);
    let mut sum = 0u64;
    for (a, b) in bgra.chunks(4).zip(dec_bgra.chunks(4)) {
        for c in 0..3 {
            sum += (a[c] as i32 - b[c] as i32).unsigned_abs() as u64;
        }
    }
    let mean_abs_error = sum as f64 / (width as f64 * height as f64 * 3.0);
    // A correct lossy round-trip lands well under this; garbage (wrong plane/stride) is ~85+.
    anyhow::ensure!(
        mean_abs_error < 35.0,
        "round-trip mean abs error {mean_abs_error:.2} too high — decode likely wrong"
    );

    Ok(SelfTest {
        encoded_bytes: nal.len(),
        start_code,
        decoded_frames: frames.len(),
        mean_abs_error,
    })
}
