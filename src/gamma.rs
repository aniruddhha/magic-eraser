// Speeds up gamma-correct blending by replacing powf with table lookups.
// Visual: identical to before (gamma-correct edges), but much faster.

pub struct GammaLut {
    // sRGB(0..255) -> linear (0..1) as f32
    srgb_to_linear: [f32; 256],
    // linear(0..1) -> sRGB(0..255) via 4096-step quantization
    // (index = (linear * 4095).round())
    linear_to_srgb: [u8; 4096],
}

impl GammaLut {
    /// Build both tables once at startup.
    pub fn new() -> Self {
        // sRGB -> linear
        let mut s2l = [0.0f32; 256];
        for v in 0..=255 {
            let c = v as f32 / 255.0;
            s2l[v] = if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
        }

        // linear -> sRGB (quantized to 4096 steps)
        let mut l2s = [0u8; 4096];
        for i in 0..4096 {
            let l = (i as f32) / 4095.0; // 0..1
            let s = if l <= 0.003_130_8 { 12.92 * l } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
            let v = (s * 255.0).round().clamp(0.0, 255.0) as u8;
            l2s[i] = v;
        }

        Self { srgb_to_linear: s2l, linear_to_srgb: l2s }
    }

    #[inline]
    pub fn srgb_u8_to_linear(&self, v: u8) -> f32 {
        self.srgb_to_linear[v as usize]
    }

    #[inline]
    pub fn linear_to_srgb_u8(&self, l: f32) -> u8 {
        // Quantize to 0..4095 index
        let idx = (l.clamp(0.0, 1.0) * 4095.0).round() as usize;
        self.linear_to_srgb[idx]
    }
}
