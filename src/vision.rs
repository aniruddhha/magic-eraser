// Background median builder for Step 3.
// Visual expectation: after you record N frames, the computed background looks
// like your empty scene without moving subjects (hands/you/etc.).
use crate::gamma::GammaLut;
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


pub fn box_blur_rgb(
    src: &FrameBuffer,      // input (live camera for this frame)
    tmp: &mut FrameBuffer,  // horizontal pass result (scratch)
    dst: &mut FrameBuffer,  // final blurred output
    radius: usize,          // blur amount; bigger = softer (and slightly slower)
) -> Result<(), Error> {
    if src.width != dst.width || src.height != dst.height {
        return Err(Error::CameraFrame("box_blur: size mismatch src↔dst".into()));
    }
    if tmp.width != src.width || tmp.height != src.height {
        return Err(Error::CameraFrame("box_blur: size mismatch tmp".into()));
    }
    let w = src.width as i32;     // screen width in pixels
    let h = src.height as i32;    // screen height in pixels
    let r = radius as i32;        // blur radius
    let win = (2 * r + 1) as u32; // window width for averaging (constant everywhere)

    /* ---- Pass 1: Horizontal (store averaged rows in tmp) ----
       What you SEE: nothing yet (tmp is off-screen), but we prepare row averages. */
    for y in 0..h {
        // Grab row start index once (faster indexing)
        let row_ofs = (y as usize) * (w as usize);

        // Edge pixel value at x=0 (we "extend" edges to avoid dark borders)
        let px0 = src.pixels[row_ofs + 0];
        let (mut sr, mut sg, mut sb) = (
            (((px0 >> 16) & 0xFF) as u32) * (r as u32 + 1),
            (((px0 >>  8) & 0xFF) as u32) * (r as u32 + 1),
            (((px0      ) & 0xFF) as u32) * (r as u32 + 1),
        );

        // Prime the right side of the initial window [0..r]
        for x in 1..=r {
            let xr = x.min(w - 1) as usize;        // clamp at right edge
            let p = src.pixels[row_ofs + xr];
            sr += ((p >> 16) & 0xFF) as u32;
            sg += ((p >>  8) & 0xFF) as u32;
            sb += ((p      ) & 0xFF) as u32;
        }

        // Slide the window across the row
        for x in 0..w {
            // Average = sum / window; pack back to 0x00RRGGBB
            let r8 = (sr / win) as u32;
            let g8 = (sg / win) as u32;
            let b8 = (sb / win) as u32;
            tmp.pixels[row_ofs + x as usize] = (r8 << 16) | (g8 << 8) | b8;

            // Update sums for next column (add right, remove left)
            let left_x  = (x - r).max(0) as usize;     // clamped left index
            let right_x = (x + r + 1).min(w - 1) as usize; // clamped new right index

            let p_sub = src.pixels[row_ofs + left_x];
            let p_add = src.pixels[row_ofs + right_x];

            sr = sr + (((p_add >> 16) & 0xFF) as u32) - (((p_sub >> 16) & 0xFF) as u32);
            sg = sg + (((p_add >>  8) & 0xFF) as u32) - (((p_sub >>  8) & 0xFF) as u32);
            sb = sb + (((p_add      ) & 0xFF) as u32) - (((p_sub      ) & 0xFF) as u32);
        }
    }

    /* ---- Pass 2: Vertical (read tmp, write dst) ----
       What you SEE: `dst` becomes a blurred copy of `src`. */
    for x in 0..w {
        // Edge pixel at y=0 for this column
        let p0 = tmp.pixels[x as usize];
        let (mut sr, mut sg, mut sb) = (
            (((p0 >> 16) & 0xFF) as u32) * (r as u32 + 1),
            (((p0 >>  8) & 0xFF) as u32) * (r as u32 + 1),
            (((p0      ) & 0xFF) as u32) * (r as u32 + 1),
        );

        // Prime the initial window [0..r] downwards
        for y in 1..=r {
            let yr = y.min(h - 1);
            let p = tmp.pixels[(yr as usize) * (w as usize) + x as usize];
            sr += ((p >> 16) & 0xFF) as u32;
            sg += ((p >>  8) & 0xFF) as u32;
            sb += ((p      ) & 0xFF) as u32;
        }

        // Slide the window down the column
        for y in 0..h {
            let idx = (y as usize) * (w as usize) + x as usize;
            let r8 = (sr / win) as u32;
            let g8 = (sg / win) as u32;
            let b8 = (sb / win) as u32;
            dst.pixels[idx] = (r8 << 16) | (g8 << 8) | b8;

            let top_y    = (y - r).max(0);
            let bottom_y = (y + r + 1).min(h - 1);

            let p_sub = tmp.pixels[(top_y as usize)    * (w as usize) + x as usize];
            let p_add = tmp.pixels[(bottom_y as usize) * (w as usize) + x as usize];

            sr = sr + (((p_add >> 16) & 0xFF) as u32) - (((p_sub >> 16) & 0xFF) as u32);
            sg = sg + (((p_add >>  8) & 0xFF) as u32) - (((p_sub >>  8) & 0xFF) as u32);
            sb = sb + (((p_add      ) & 0xFF) as u32) - (((p_sub      ) & 0xFF) as u32);
        }
    }

    Ok(())
}

pub fn blend_linear_in_place(
    fg_live: &mut FrameBuffer,
    sink: &FrameBuffer,     // NOTE: was `bg` before; now it's BLUR(LIVE)
    mask: &Mask,
    lut: &GammaLut,
) -> Result<(), Error> {
    if fg_live.width != sink.width || fg_live.height != sink.height {
        return Err(Error::CameraFrame("blend: dimension mismatch".into()));
    }
    if mask.width != fg_live.width || mask.height != fg_live.height {
        return Err(Error::CameraFrame("blend: mask dimension mismatch".into()));
    }

    let len = fg_live.width * fg_live.height;
    for i in 0..len {
        let a = mask.alpha[i];
        if a <= 0.0 { continue; }            // visual: keep raw live
        if a >= 1.0 {                        // visual: fully blurred at this pixel
            fg_live.pixels[i] = sink.pixels[i];
            continue;
        }

        let pf = fg_live.pixels[i];
        let ps = sink.pixels[i];

        let rf = ((pf >> 16) & 0xFF) as u8;  // live R
        let gf = ((pf >>  8) & 0xFF) as u8;  // live G
        let bf = ( pf        & 0xFF) as u8;  // live B

        let rs = ((ps >> 16) & 0xFF) as u8;  // sink (blurred) R
        let gs = ((ps >>  8) & 0xFF) as u8;  // sink (blurred) G
        let bs = ( ps        & 0xFF) as u8;  // sink (blurred) B

        let rf_lin = lut.srgb_u8_to_linear(rf);
        let gf_lin = lut.srgb_u8_to_linear(gf);
        let bf_lin = lut.srgb_u8_to_linear(bf);

        let rs_lin = lut.srgb_u8_to_linear(rs);
        let gs_lin = lut.srgb_u8_to_linear(gs);
        let bs_lin = lut.srgb_u8_to_linear(bs);

        let inv = 1.0 - a;
        let r_lin = a * rs_lin + inv * rf_lin;
        let g_lin = a * gs_lin + inv * gf_lin;
        let b_lin = a * bs_lin + inv * bf_lin;

        let r = lut.linear_to_srgb_u8(r_lin) as u32;
        let g = lut.linear_to_srgb_u8(g_lin) as u32;
        let b = lut.linear_to_srgb_u8(b_lin) as u32;
        fg_live.pixels[i] = (r << 16) | (g << 8) | b; // visual: blurred mix at this pixel
    }
    Ok(())
}