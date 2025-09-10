// Core types used by Steps 1–4.

#[derive(Clone)]
pub struct FrameBuffer {
    pub width: usize,      // how wide the frame is on screen (pixels)
    pub height: usize,     // how tall the frame is on screen (pixels)
    pub pixels: Vec<u32>,  // each entry is 0x00RRGGBB for minifb
}

/// Alpha mask in [0,1] per pixel; 1 = use background, 0 = use live foreground.
/// Visual: unseen directly; it controls how much “erase” happens at each pixel.
pub struct Mask {
    pub width: usize,
    pub height: usize,
    pub alpha: Vec<f32>,   // length = width * height, values clamped to [0.0, 1.0]
}

/// Precomputed circular Gaussian “stamp” we dab into the Mask at the pointer.
/// Visual: makes the erase edge soft/feathered.
pub struct Stamp {
    pub radius: i32,       // pixels from center to edge
    pub weights: Vec<f32>, // (2r+1)*(2r+1), centered kernel, already normalized to peak 1.0
}
