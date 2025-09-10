// Background median builder for Step 3.
// Visual expectation: after you record N frames, the computed background looks
// like your empty scene without moving subjects (hands/you/etc.).

use crate::error::Error;
use crate::types::{FrameBuffer, Mask, Stamp};

pub const BG_CAPTURE_COUNT: usize = 35; // ~1–2 seconds of frames at 30 FPS

/// Compute per-pixel median across the provided frames (all same size).
/// What you *see* afterward: a clean background image with moving objects removed.
pub fn median_background(frames: &[FrameBuffer]) -> Result<FrameBuffer, Error> {
    // 1) Must have at least 1 frame; otherwise we cannot build a background.
    if frames.is_empty() {
        return Err(Error::CameraFrame("median_background: no frames".into()));
    }

    // 2) Verify all frames share the same resolution, else drawing will look scrambled.
    let w = frames[0].width;
    let h = frames[0].height;
    for f in frames.iter() {
        if f.width != w || f.height != h {
            return Err(Error::CameraFrame(
                "median_background: frames must share identical dimensions".into(),
            ));
        }
    }

    // 3) Prepare an output buffer of the same size (what we'll show as BG).
    let mut out = Vec::with_capacity(w * h);

    // 4) We'll compute median per pixel, channel by channel (R,G,B).
    //    For speed (and to avoid heap allocs per pixel), we use fixed-size arrays
    //    sized by the capture count, and slice them to the actual length.
    let k = frames.len();
    let mut rbuf = vec![0u8; k];
    let mut gbuf = vec![0u8; k];
    let mut bbuf = vec![0u8; k];

    // 5) For each pixel index, collect all Rs, all Gs, all Bs, then sort and pick the middle.
    for idx in 0..(w * h) {
        // Gather channel values across all frames
        for (i, f) in frames.iter().enumerate() {
            let px = f.pixels[idx];
            // px = 0x00RRGGBB
            rbuf[i] = ((px >> 16) & 0xFF) as u8;
            gbuf[i] = ((px >> 8) & 0xFF) as u8;
            bbuf[i] = (px & 0xFF) as u8;
        }

        // Sort in place and pick median (k is small ~35; this is fine for learning)
        rbuf[..k].sort_unstable();
        gbuf[..k].sort_unstable();
        bbuf[..k].sort_unstable();
        let mid = k / 2;
        let r = rbuf[mid] as u32;
        let g = gbuf[mid] as u32;
        let b = bbuf[mid] as u32;

        out.push((r << 16) | (g << 8) | b); // pack back as 0x00RRGGBB
    }

    Ok(FrameBuffer { width: w, height: h, pixels: out })
}

/// Make a circular Gaussian stamp with peak 1.0 at the center.
/// Visual: defines how soft the eraser edge looks.
pub fn make_gaussian_stamp(radius: i32, sigma: f32) -> Stamp {
    let d = 2 * radius + 1;                   // kernel size (width = height)
    let mut weights = Vec::with_capacity((d * d) as usize);
    let s2 = 2.0 * sigma * sigma;             // denominator in the exponent
    let mut maxw = 0.0_f32;

    // Build a radially symmetric weight per pixel in the kernel
    for y in -radius..=radius {
        for x in -radius..=radius {
            let r2 = (x as f32) * (x as f32) + (y as f32) * (y as f32);
            let w = (-r2 / s2).exp();         // e^{ -r^2 / (2 sigma^2) }
            if w > maxw { maxw = w; }
            weights.push(w);
        }
    }
    // Normalize to peak 1.0 (not sum=1); we want full strength at the center
    if maxw > 0.0 {
        for w in &mut weights { *w /= maxw; }
    }

    Stamp { radius, weights }
}

/// Add (dab) the stamp into the alpha mask at (cx, cy).
/// Visual: increases erase strength under the cursor, with soft edges.
pub fn dab_mask(mask: &mut Mask, cx: i32, cy: i32, stamp: &Stamp) {
    let w = mask.width as i32;
    let h = mask.height as i32;
    let r = stamp.radius;
    let d = 2 * r + 1;

    for ky in 0..d {
        for kx in 0..d {
            let sx = cx + kx - r;             // screen x for this kernel cell
            let sy = cy + ky - r;             // screen y for this kernel cell
            if sx < 0 || sy < 0 || sx >= w || sy >= h { continue; }
            let idx = sy as usize * mask.width + sx as usize;
            let kidx = ky as usize * d as usize + kx as usize;

            // Add stamp weight; clamp to 1.0 (full erase).
            let a = mask.alpha[idx] + stamp.weights[kidx];
            mask.alpha[idx] = if a > 1.0 { 1.0 } else { a };
        }
    }
}

/// Clear the mask to 0 (no erase anywhere).
pub fn clear_mask(mask: &mut Mask) {
    for a in &mut mask.alpha { *a = 0.0; }
}

// ---------------------- sRGB <-> Linear helpers (gamma correct) ----------------------

#[inline] fn srgb_u8_to_linear(c: u8) -> f32 {
    // Convert 0..255 to 0..1 sRGB, then to linear light
    let c = (c as f32) / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}
#[inline] fn linear_to_srgb_u8(l: f32) -> u8 {
    // Convert linear 0..1 back to sRGB 0..255
    let l = if l <= 0.0 { 0.0 } else if l >= 1.0 { 1.0 } else { l };
    let s = if l <= 0.0031308 { 12.92 * l } else { 1.055 * l.powf(1.0/2.4) - 0.055 };
    let v = (s * 255.0).round();
    if v < 0.0 { 0 } else if v > 255.0 { 255 } else { v as u8 }
}

/// Blend FG (live) with BG using the alpha mask in **linear light**.
/// Visual: where alpha=1 you fully see the background; where 0 you see live; edges are invisible.
pub fn blend_linear_in_place(fg_live: &mut FrameBuffer, bg: &FrameBuffer, mask: &Mask) -> Result<(), Error> {
    // Precondition checks so you don't see scrambled pictures.
    if fg_live.width != bg.width || fg_live.height != bg.height {
        return Err(Error::CameraFrame("blend: dimension mismatch".into()));
    }
    if mask.width != fg_live.width || mask.height != fg_live.height {
        return Err(Error::CameraFrame("blend: mask dimension mismatch".into()));
    }

    // For every pixel: out = α * BG + (1-α) * FG (all in linear space, per channel)
    let len = fg_live.width * fg_live.height;
    for i in 0..len {
        // Unpack live FG
        let px_f = fg_live.pixels[i];
        let rf = ((px_f >> 16) & 0xFF) as u8;
        let gf = ((px_f >>  8) & 0xFF) as u8;
        let bf = ( px_f        & 0xFF) as u8;

        // Unpack BG
        let px_b = bg.pixels[i];
        let rb = ((px_b >> 16) & 0xFF) as u8;
        let gb = ((px_b >>  8) & 0xFF) as u8;
        let bb = ( px_b        & 0xFF) as u8;

        // Convert to linear light (0..1)
        let rf_lin = srgb_u8_to_linear(rf);
        let gf_lin = srgb_u8_to_linear(gf);
        let bf_lin = srgb_u8_to_linear(bf);
        let rb_lin = srgb_u8_to_linear(rb);
        let gb_lin = srgb_u8_to_linear(gb);
        let bb_lin = srgb_u8_to_linear(bb);

        // Alpha for this pixel (0..1)
        let a = mask.alpha[i];

        // Linear blend
        let r_lin = a * rb_lin + (1.0 - a) * rf_lin;
        let g_lin = a * gb_lin + (1.0 - a) * gf_lin;
        let b_lin = a * bb_lin + (1.0 - a) * bf_lin;

        // Back to sRGB u8 and repack to 0x00RRGGBB
        let r = linear_to_srgb_u8(r_lin) as u32;
        let g = linear_to_srgb_u8(g_lin) as u32;
        let b = linear_to_srgb_u8(b_lin) as u32;
        fg_live.pixels[i] = (r << 16) | (g << 8) | b;
    }
    Ok(())
}
