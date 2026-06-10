//! host-windows::h264 — hardware H.264 encode via Media Foundation (ADR-0011 4c).
//!
//! 4c.0 (this file): a **capability probe**. It enumerates the H.264 *encoder* MFTs on this
//! machine — hardware async MFTs (NVENC / Quick Sync / AMF) and the Microsoft software
//! encoder floor — via `MFTEnumEx`, and prints their friendly names. This tells us which
//! encoder 4c.1 should instantiate, and compiles the Media Foundation interop surface before
//! the real encoder is built. Encoding does not touch the default GDI/interframe path, so
//! there is no regression.

use anyhow::{Context, Result};
use windows::core::PWSTR;
use windows::Win32::Media::MediaFoundation::{
    IMFActivate, MFMediaType_Video, MFShutdown, MFStartup, MFTEnumEx, MFT_FRIENDLY_NAME_Attribute,
    MFVideoFormat_H264, MFSTARTUP_FULL, MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG,
    MFT_ENUM_FLAG_ASYNCMFT, MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_SORTANDFILTER,
    MFT_ENUM_FLAG_SYNCMFT, MFT_ENUM_FLAG_TRANSCODE_ONLY, MFT_REGISTER_TYPE_INFO, MF_VERSION,
};
use windows::Win32::System::Com::{CoIncrementMTAUsage, CoTaskMemFree};

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
