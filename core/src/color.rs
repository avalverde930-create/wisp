//! core::color — BGRA8 <-> NV12 colour conversion for the H.264 path (ADR-0011 4c).
//!
//! Hardware H.264 encoders (QSV/NVENC/AMF) take NV12 (4:2:0), not BGRA, and their decoders
//! emit NV12; this module is the shared, tested conversion both ends use. Because Wisp owns
//! both the encode-input and decode-output conversions, the exact matrix matters only insofar
//! as the two are inverses — we use BT.709 full-range throughout.
//!
//! NV12 layout: a full-resolution Y plane (`w*h` bytes) followed by an interleaved UV plane
//! (`w*h/2` bytes) at half resolution — one U,V pair per 2x2 luma block. Width and height must
//! be even (true for every monitor resolution). Fixed-point integer arithmetic (scale 1024)
//! keeps it fast and deterministic; the GPU path (D3D11 video processor / shader) is a later
//! optimisation that also removes this CPU cost from the per-frame pipeline.

/// Bytes of NV12 for a `width`x`height` frame: `w*h` luma + `w*h/2` interleaved chroma.
pub fn nv12_len(width: u32, height: u32) -> usize {
    let (w, h) = (width as usize, height as usize);
    w * h + w * h / 2
}

#[inline]
fn clamp8(x: i32) -> u8 {
    x.clamp(0, 255) as u8
}

/// Convert a BGRA8 frame to NV12 (BT.709 full-range). `width` and `height` must be even.
pub fn bgra_to_nv12(bgra: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    debug_assert!(w % 2 == 0 && h % 2 == 0, "NV12 requires even dimensions");
    debug_assert!(bgra.len() >= w * h * 4, "BGRA buffer too small");

    let mut out = vec![0u8; nv12_len(width, height)];
    let (y_plane, uv_plane) = out.split_at_mut(w * h);

    // Luma, per pixel (BT.709 full-range, fixed-point scale 1024; +512 rounds to nearest).
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            let b = bgra[i] as i32;
            let g = bgra[i + 1] as i32;
            let r = bgra[i + 2] as i32;
            y_plane[y * w + x] = clamp8((218 * r + 732 * g + 74 * b + 512) >> 10);
        }
    }

    // Chroma, averaged over each 2x2 block (>>12 = scale 1024 then /4; +2048 rounds to nearest).
    let cw = w / 2;
    for cy in 0..h / 2 {
        for cx in 0..cw {
            let (mut sr, mut sg, mut sb) = (0i32, 0i32, 0i32);
            for dy in 0..2 {
                for dx in 0..2 {
                    let i = ((cy * 2 + dy) * w + (cx * 2 + dx)) * 4;
                    sb += bgra[i] as i32;
                    sg += bgra[i + 1] as i32;
                    sr += bgra[i + 2] as i32;
                }
            }
            let u = (((-117 * sr - 395 * sg + 512 * sb) + 2048) >> 12) + 128;
            let v = (((512 * sr - 465 * sg - 47 * sb) + 2048) >> 12) + 128;
            uv_plane[(cy * cw + cx) * 2] = clamp8(u);
            uv_plane[(cy * cw + cx) * 2 + 1] = clamp8(v);
        }
    }
    out
}

/// Convert an NV12 frame back to BGRA8 (BT.709 full-range, opaque alpha). Inverse of
/// [`bgra_to_nv12`]; `width` and `height` must be even.
pub fn nv12_to_bgra(nv12: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    debug_assert!(w % 2 == 0 && h % 2 == 0, "NV12 requires even dimensions");
    debug_assert!(
        nv12.len() >= nv12_len(width, height),
        "NV12 buffer too small"
    );

    let (y_plane, uv_plane) = nv12.split_at(w * h);
    let cw = w / 2;
    let mut out = vec![0u8; w * h * 4];
    // BT.709 full-range inverse, fixed-point scale 1024 (+512 rounds to nearest).
    for y in 0..h {
        for x in 0..w {
            let yy = y_plane[y * w + x] as i32;
            let uvi = ((y / 2) * cw + (x / 2)) * 2;
            let u = uv_plane[uvi] as i32 - 128;
            let v = uv_plane[uvi + 1] as i32 - 128;
            let o = (y * w + x) * 4;
            out[o] = clamp8(yy + ((1900 * u + 512) >> 10)); // B
            out[o + 1] = clamp8(yy - ((192 * u + 479 * v + 512) >> 10)); // G
            out[o + 2] = clamp8(yy + ((1613 * v + 512) >> 10)); // R
            out[o + 3] = 255; // A
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, b: u8, g: u8, r: u8) -> Vec<u8> {
        let mut v = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..w * h {
            v.extend_from_slice(&[b, g, r, 255]);
        }
        v
    }

    #[test]
    fn nv12_size_is_3_halves() {
        assert_eq!(nv12_len(1920, 1080), 1920 * 1080 * 3 / 2);
        let nv = bgra_to_nv12(&solid(8, 8, 10, 20, 30), 8, 8);
        assert_eq!(nv.len(), nv12_len(8, 8));
    }

    #[test]
    fn gray_roundtrips_closely() {
        let back = nv12_to_bgra(&bgra_to_nv12(&solid(4, 4, 128, 128, 128), 4, 4), 4, 4);
        for px in back.chunks(4) {
            assert!((px[0] as i32 - 128).abs() <= 2, "B {}", px[0]);
            assert!((px[1] as i32 - 128).abs() <= 2, "G {}", px[1]);
            assert!((px[2] as i32 - 128).abs() <= 2, "R {}", px[2]);
            assert_eq!(px[3], 255);
        }
    }

    #[test]
    fn solid_color_roundtrips() {
        let (b, g, r) = (50u8, 100u8, 200u8);
        let back = nv12_to_bgra(&bgra_to_nv12(&solid(8, 8, b, g, r), 8, 8), 8, 8);
        for px in back.chunks(4) {
            assert!((px[0] as i32 - b as i32).abs() <= 4, "B {} vs {b}", px[0]);
            assert!((px[1] as i32 - g as i32).abs() <= 4, "G {} vs {g}", px[1]);
            assert!((px[2] as i32 - r as i32).abs() <= 4, "R {} vs {r}", px[2]);
        }
    }

    #[test]
    fn white_and_black_luma() {
        let white = bgra_to_nv12(&solid(2, 2, 255, 255, 255), 2, 2);
        assert!(white[0] >= 250, "white Y {}", white[0]);
        let black = bgra_to_nv12(&solid(2, 2, 0, 0, 0), 2, 2);
        assert!(black[0] <= 5, "black Y {}", black[0]);
    }
}
