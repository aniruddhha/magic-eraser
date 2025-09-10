// FX: sparkles + lightning with precomputed glow discs (no per-pixel exp()).
// What you SEE on screen:
// - Warm sparkles pop around your eraser stroke, drift a bit, then fade out.
// - Occasionally a bluish lightning bolt flickers briefly and disappears.
// - Visuals match the previous version, but run much faster.

use crate::types::FrameBuffer;

/* -------------------- tiny RNG (visual jitter only) -------------------- */

#[derive(Clone)]
struct Rng32 { state: u32 }

impl Rng32 {
    // Creates a repeatable random sequence (so the "feel" is consistent).
    pub fn from_seed(seed: u32) -> Self { Self { state: seed | 1 } }

    // Produces the next random 32-bit number (used for velocity/angles/chance).
    #[inline] fn next_u32(&mut self) -> u32 {
        let mut x = self.state;
        x ^= x << 13; x ^= x >> 17; x ^= x << 5;
        self.state = x;
        x
    }

    // Uniform float in [0,1); used to scale velocities/lifetimes.
    #[inline] fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / ((1u32 << 24) as f32)
    }

    // Picks a random value in [min,max); used for speeds/angles/jitter.
    #[inline] fn range(&mut self, min: f32, max: f32) -> f32 {
        min + (max - min) * self.next_f32()
    }
}

/* -------------------- additive pixel helper (fast & simple) -------------------- */

/// Adds an RGB triplet at (x,y) with saturation (clamps to 255).
/// What you SEE: the pixel gets brighter/colored; repeated stamps glow.
#[inline]
fn add_rgb_saturating(fb: &mut FrameBuffer, x: i32, y: i32, r: u8, g: u8, b: u8) {
    if x < 0 || y < 0 { return; }
    let (x, y) = (x as usize, y as usize);
    if x >= fb.width || y >= fb.height { return; }

    let idx = y * fb.width + x;
    let old = fb.pixels[idx];

    // Extract current pixel color (0x00RRGGBB → r,g,b)
    let or = ((old >> 16) & 0xFF) as u16;
    let og = ((old >>  8) & 0xFF) as u16;
    let ob = ( old        & 0xFF) as u16;

    // Add the new light and clamp to 255 (brighter but never wraps around).
    let nr = (or + r as u16).min(255) as u32;
    let ng = (og + g as u16).min(255) as u32;
    let nb = (ob + b as u16).min(255) as u32;

    fb.pixels[idx] = (nr << 16) | (ng << 8) | nb;
}

/* -------------------- precomputed glow discs (the BIG speedup) -------------------- */

/// One circular glow kernel. Each entry is a weight 0..255 (255 at center).
/// What you SEE: a soft round glow when this kernel is stamped.
struct DiscKernel {
    radius: i32,       // visual size (pixels from center to edge)
    dim: i32,          // width = height = 2r+1 (convenience)
    weights: Vec<u8>,  // (dim * dim) weights, row-major, 0..255
}

impl DiscKernel {
    /// Build a Gaussian-like disc once. Called at startup only.
    fn build(radius: i32) -> Self {
        let dim = 2 * radius + 1;
        let mut weights = Vec::with_capacity((dim * dim) as usize);

        // Choose sigma ≈ radius/2 for a pleasant falloff (soft but not too wide).
        let sigma = (radius as f32) * 0.5_f32;
        let s2 = 2.0 * sigma * sigma;

        // First pass: compute float weights and find the max at the center.
        let mut maxw = 0.0f32;
        let mut temp = Vec::with_capacity((dim * dim) as usize);

        for y in -radius..=radius {
            for x in -radius..=radius {
                let r2 = (x as f32) * (x as f32) + (y as f32) * (y as f32);
                // Gaussian shape: 1 at center, decays towards the edge.
                let w = (-r2 / s2).exp();
                if w > maxw { maxw = w; }
                temp.push(w);
            }
        }

        // Second pass: normalize so center = 255, quantize to u8 (0..255).
        let scale = if maxw > 0.0 { 255.0 / maxw } else { 0.0 };
        for w in temp {
            let q = (w * scale).round().clamp(0.0, 255.0) as u8;
            weights.push(q);
        }

        Self { radius, dim, weights }
    }

    /// Stamps this disc at (cx,cy) with color (base_r,base_g,base_b) and strength [0,1].
    /// All INT math inside inner loop (fast). 
    /// What you SEE: a fuzzy glowing dot centered at (cx,cy).
    #[inline]
    fn stamp_additive(
        &self,
        fb: &mut FrameBuffer,
        cx: i32, cy: i32,
        base_r: u8, base_g: u8, base_b: u8,
        strength: f32,
    ) {
        // Convert strength 0..1 to 0..255 once (avoids float per pixel).
        let s8: u16 = (strength.clamp(0.0, 1.0) * 255.0).round() as u16;

        let w = fb.width as i32;
        let h = fb.height as i32;
        let r = self.radius;
        let dim = self.dim;

        // Scan only the disc bounding box on the screen.
        for ky in 0..dim {
            for kx in 0..dim {
                // Screen coordinate this kernel cell hits.
                let sx = cx + kx - r;
                let sy = cy + ky - r;
                if sx < 0 || sy < 0 || sx >= w || sy >= h { continue; }

                // Kernel weight (0..255) at this cell.
                let w8 = self.weights[(ky * dim + kx) as usize] as u16;
                if w8 == 0 { continue; }

                // Combine kernel weight with strength → 0..255 (still integer).
                let wscaled = (w8 * s8 + 127) / 255; // round

                // Scale base color by wscaled/255 (still integer math).
                let rr = ((base_r as u16 * wscaled + 127) / 255) as u8;
                let gg = ((base_g as u16 * wscaled + 127) / 255) as u8;
                let bb = ((base_b as u16 * wscaled + 127) / 255) as u8;

                // Add the glow to the framebuffer (saturating).
                add_rgb_saturating(fb, sx, sy, rr, gg, bb);
            }
        }
    }
}

/* -------------------- particles (sparkles) + bolt (lightning) -------------------- */

/// One sparkle. What you SEE: tiny glow that moves a bit and fades out.
pub struct Particle {
    pub x: f32, pub y: f32,      // screen position in pixels
    pub vx: f32, pub vy: f32,    // velocity in px/sec
    pub life: f32,               // remaining lifetime (seconds)
    pub max_life: f32,           // initial lifetime (for fade)
    pub energy: f32,             // brightness multiplier 0..1
}

/// One lightning bolt. What you SEE: jagged bright line that flickers briefly.
pub struct Bolt {
    pub pts: Vec<(f32,f32)>,     // polyline points
    pub ttl: f32,                // time to live (seconds)
}

/// FX system. What you SEE: all sparkles and the rare lightning on screen.
pub struct Fx {
    rng: Rng32,
    particles: Vec<Particle>,
    max_particles: usize,
    bolt: Option<Bolt>,

    // Precomputed glow discs so stamping is fast (no exp during rendering).
    // We keep a small set that looks good and covers typical sizes.
    kernels: [DiscKernel; 7],    // radii: 2..8 inclusive
}

impl Fx {
    /// Create the effect system. What you SEE: nothing yet; ready to spawn FX.
    pub fn new(max_particles: usize) -> Self {
        // Build discs once; the cost is paid at startup, never per pixel per frame.
        let kernels = [
            DiscKernel::build(2),
            DiscKernel::build(3),
            DiscKernel::build(4),
            DiscKernel::build(5),
            DiscKernel::build(6),
            DiscKernel::build(7),
            DiscKernel::build(8),
        ];

        Self {
            rng: Rng32::from_seed(0xBADA55),
            particles: Vec::with_capacity(max_particles),
            max_particles,
            bolt: None,
            kernels,
        }
    }

    /// Spawn a handful of warm sparkles at (x,y).
    /// What you SEE: small glows popping at the cursor when you erase.
    pub fn spawn_sparkles(&mut self, x: f32, y: f32, count: usize) {
        for _ in 0..count {
            if self.particles.len() >= self.max_particles { break; }

            // Random speed and angle → lively motion.
            let speed = self.rng.range(30.0, 90.0);
            let ang = self.rng.range(0.0, std::f32::consts::TAU);
            let vx = speed * ang.cos();
            let vy = speed * ang.sin() - self.rng.range(0.0, 20.0); // slight upward bias

            // Lifetime drives fade: short = snappy sparkles.
            let max_life = self.rng.range(0.35, 0.75);

            self.particles.push(Particle {
                x, y, vx, vy,
                life: max_life,
                max_life,
                energy: self.rng.range(0.6, 1.0),
            });
        }
    }

    /// Randomly spawn a lightning bolt near (x,y).
    /// What you SEE: an occasional fast “zap” to add excitement.
    pub fn maybe_spawn_bolt(&mut self, x: f32, y: f32) {
        // ~3% chance per call while erasing (tweak to taste).
        if self.rng.next_f32() > 0.03 { return; }

        let segs = 10;                        // how many segments in the bolt
        let len  = self.rng.range(40.0, 90.0);// total length (pixels)
        let theta = self.rng.range(0.0, std::f32::consts::TAU);

        let mut pts = Vec::with_capacity(segs + 1);
        let (mut px, mut py) = (x, y);
        pts.push((px, py));

        // Build a jagged path with some sideways wobble.
        for _ in 0..segs {
            let step = len / segs as f32;
            let jitter = self.rng.range(-0.6, 0.6);
            let dir = theta + jitter;

            px += step * dir.cos();
            py += step * dir.sin();

            px += self.rng.range(-2.0, 2.0);
            py += self.rng.range(-2.0, 2.0);

            pts.push((px, py));
        }

        self.bolt = Some(Bolt { pts, ttl: 0.10 }); // quick flash (~100 ms)
    }

    /// Update physics and render FX into the framebuffer (additive).
    /// What you SEE: sparkles drift & fade; bolt flashes then vanishes.
    pub fn update_and_render(&mut self, fb: &mut FrameBuffer, dt: f32) {
        /* ---- Particles ---- */
        let mut i = 0;
        while i < self.particles.len() {
            let p = &mut self.particles[i];

            // Move the particle a bit (simple Euler integration).
            p.x += p.vx * dt;
            p.y += p.vy * dt;

            // Add a touch of drag and gravity-ish pull for a lively feel.
            p.vx *= 0.98;
            p.vy = p.vy * 0.98 + 10.0 * dt;

            // Tick down its lifetime (drives fade & size).
            p.life -= dt;

            if p.life > 0.0 {
                // life01: 1 at birth → 0 at death (controls radius/brightness).
                let life01 = (p.life / p.max_life).clamp(0.0, 1.0);

                // Choose a precomputed disc close to the target radius (2..8 px).
                // Bigger near birth, smaller near death (feels like a spark).
                let desired = (6.0 * life01 + 2.0).round() as i32; // ~2..8
                let idx = (desired - 2).clamp(0, 6) as usize;
                let kernel = &self.kernels[idx];

                // Brightness fades with life; energy adds variation.
                let strength = (0.9 * p.energy * life01).clamp(0.0, 1.0);

                // Warm gold color looks “magical”.
                let (r, g, b) = (255u8, 200u8, 80u8);

                // Stamp the disc at the particle position (integer math inside).
                kernel.stamp_additive(fb, p.x as i32, p.y as i32, r, g, b, strength);

                i += 1; // keep this particle for the next frame
            } else {
                // Remove dead particle quickly (swap_remove = O(1)).
                self.particles.swap_remove(i);
            }
        }

        /* ---- Lightning ---- */
        if let Some(b) = &mut self.bolt {
            // Bolt fades quickly (ttl → 0).
            b.ttl -= dt;
            let s = (b.ttl / 0.10).clamp(0.0, 1.0);

            // Use a small, bright bluish disc to draw along the polyline.
            let kernel = &self.kernels[1]; // radius 3 → crisp thin bolt
            let (r, g, bcol) = (210u8, 230u8, 255u8);

            // For each segment, stamp discs every ~2 px to make a continuous line.
            for seg in 0..b.pts.len().saturating_sub(1) {
                let (x0, y0) = b.pts[seg];
                let (x1, y1) = b.pts[seg + 1];
                let dx = x1 - x0;
                let dy = y1 - y0;
                let dist = (dx * dx + dy * dy).sqrt().max(1.0);
                let steps = (dist / 2.0).ceil() as i32;

                for tstep in 0..=steps {
                    let t = tstep as f32 / steps as f32;
                    let x = x0 + dx * t;
                    let y = y0 + dy * t;

                    // Strength scales with bolt fade (s): starts bright → vanishes.
                    kernel.stamp_additive(fb, x as i32, y as i32, r, g, bcol, 1.2 * s);
                }
            }

            // When ttl runs out, the bolt disappears completely.
            if b.ttl <= 0.0 { self.bolt = None; }
        }
    }
}
