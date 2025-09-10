// FX: sparkles (particles) + lightning, fully software-drawn with additive blending.
// Visual outcomes:
// - Small warm-glow sparkles spawn at the cursor while you hold LMB and gradually fade.
// - Occasionally, a bluish-white lightning bolt flickers near the cursor and disappears quickly.

use crate::types::FrameBuffer;

// ----------------------------- tiny RNG (no external crate) -----------------------------

/// Deterministic xorshift32 RNG for lightweight randomness.
/// Visual: controls particle velocities/jitter and the chance of spawning lightning.
#[derive(Clone)]
struct Rng32 { state: u32 }

impl Rng32 {
    pub fn from_seed(seed: u32) -> Self { Self { state: seed | 1 } }
    #[inline] fn next_u32(&mut self) -> u32 {
        // Xorshift—fast and good enough for visual noise
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        x
    }
    #[inline] fn next_f32(&mut self) -> f32 {
        // Uniform [0,1)
        (self.next_u32() >> 8) as f32 / ((1u32 << 24) as f32)
    }
    #[inline] fn range(&mut self, min: f32, max: f32) -> f32 {
        min + (max - min) * self.next_f32()
    }
}

// ----------------------------- additive drawing helpers --------------------------------

/// Additive blend one RGB triplet at (x,y) with saturation to 255.
/// Visual: the pixel gets brighter/colored; repeated draws stack until white.
#[inline]
fn add_rgb_saturating(fb: &mut FrameBuffer, x: i32, y: i32, r: u8, g: u8, b: u8) {
    if x < 0 || y < 0 { return; }
    let (x, y) = (x as usize, y as usize);
    if x >= fb.width || y >= fb.height { return; }

    let idx = y * fb.width + x;
    let old = fb.pixels[idx];

    let or = ((old >> 16) & 0xFF) as u16;
    let og = ((old >> 8)  & 0xFF) as u16;
    let ob = ( old        & 0xFF) as u16;

    // Add with clamp (saturating)
    let nr = (or + r as u16).min(255) as u32;
    let ng = (og + g as u16).min(255) as u32;
    let nb = (ob + b as u16).min(255) as u32;

    fb.pixels[idx] = (nr << 16) | (ng << 8) | nb;
}

/// Draw a soft round glow disc centered at (cx,cy) with additive blending.
/// `radius` in pixels; `strength` in [0,1] scales brightness; `base` is the color.
/// Visual: a fuzzy dot of light. Nearby pixels brighten more than far pixels.
fn draw_additive_disc(
    fb: &mut FrameBuffer,
    cx: i32, cy: i32,
    radius: i32,
    base_r: u8, base_g: u8, base_b: u8,
    strength: f32
) {
    if radius <= 0 { return; }
    let r = radius;
    let r2 = (r * r) as f32;
    let sigma = (r as f32) * 0.5;             // softness; smaller = sharper edge
    let denom = 2.0 * sigma * sigma;

    // Scan just the bounding box (fast enough for small radii)
    for y in (cy - r)..=(cy + r) {
        for x in (cx - r)..=(cx + r) {
            let dx = (x - cx) as f32;
            let dy = (y - cy) as f32;
            let d2 = dx*dx + dy*dy;
            if d2 > r2 { continue; }          // outside the circle

            // Gaussian falloff: 1.0 at center → ~0 at edge
            let w = (-d2 / denom).exp() * strength;

            // Scale color by w; convert to u8
            let r = (base_r as f32 * w).round().clamp(0.0, 255.0) as u8;
            let g = (base_g as f32 * w).round().clamp(0.0, 255.0) as u8;
            let b = (base_b as f32 * w).round().clamp(0.0, 255.0) as u8;

            add_rgb_saturating(fb, x, y, r, g, b);
        }
    }
}

// ----------------------------- particles (sparkles) ------------------------------------

/// One sparkle. Visual: small glowing dot that moves a bit and fades out.
pub struct Particle {
    pub x: f32, pub y: f32,        // position in pixels
    pub vx: f32, pub vy: f32,      // velocity in px/sec
    pub life: f32,                 // remaining lifetime in seconds
    pub max_life: f32,             // initial lifetime (for fade)
    pub energy: f32,               // brightness multiplier (0..1)
}

impl Particle {
    #[inline] fn alive(&self) -> bool { self.life > 0.0 }
}

/// A quick lightning bolt. Visual: jagged line that flickers briefly then vanishes.
pub struct Bolt {
    pub pts: Vec<(f32,f32)>,       // polyline points
    pub ttl: f32,                  // time-to-live in seconds
}

/// FX container. Visual: keeps everything that glows/zaps on screen.
pub struct Fx {
    rng: Rng32,
    particles: Vec<Particle>,
    max_particles: usize,
    bolt: Option<Bolt>,
}

impl Fx {
    /// Create with capacity for N particles.
    /// Visual: no immediate effect; screen unchanged until you spawn/update.
    pub fn new(max_particles: usize) -> Self {
        Self {
            rng: Rng32::from_seed(0xC0FFEEu32),
            particles: Vec::with_capacity(max_particles),
            max_particles,
            bolt: None,
        }
    }

    /// Spawn a handful of warm-colored sparkles at (x,y).
    /// Visual: tiny glowing dots appear at the cursor and drift outward.
    pub fn spawn_sparkles(&mut self, x: f32, y: f32, count: usize) {
        for _ in 0..count {
            if self.particles.len() >= self.max_particles { break; }
            // Random small velocity (both directions)
            let speed = self.rng.range(30.0, 90.0); // px/sec
            let angle = self.rng.range(0.0, std::f32::consts::TAU);
            let vx = speed * angle.cos();
            let vy = speed * angle.sin() - self.rng.range(0.0, 20.0); // slight upward bias

            let max_life = self.rng.range(0.35, 0.75); // seconds
            let p = Particle {
                x, y, vx, vy,
                life: max_life,
                max_life,
                energy: self.rng.range(0.6, 1.0),
            };
            self.particles.push(p);
        }
    }

    /// Maybe spawn a lightning bolt with small probability.
    /// Visual: once in a while, a blue-white bolt crackles near the cursor.
    pub fn maybe_spawn_bolt(&mut self, x: f32, y: f32) {
        // ~3% chance per call while erasing (tweak to taste)
        if self.rng.next_f32() > 0.03 { return; }

        let segs = 10;                               // number of segments
        let len = self.rng.range(40.0, 90.0);        // total length in px
        let theta = self.rng.range(0.0, std::f32::consts::TAU);

        // Build a jagged polyline starting at (x,y)
        let mut pts = Vec::with_capacity(segs + 1);
        let mut px = x;
        let mut py = y;
        pts.push((px, py));

        for _ in 0..segs {
            // Step forward a bit in the main direction with jitter
            let step = len / segs as f32;
            let jitter = self.rng.range(-0.6, 0.6);
            let dir = theta + jitter;
            px += step * dir.cos();
            py += step * dir.sin();
            // sideways wobble
            px += self.rng.range(-2.0, 2.0);
            py += self.rng.range(-2.0, 2.0);
            pts.push((px, py));
        }

        self.bolt = Some(Bolt { pts, ttl: 0.10 });   // 100 ms flash
    }

    /// Step simulation and render all FX on top of `fb` (additive).
    /// Visual: particles drift/fade; bolt flickers then disappears.
    pub fn update_and_render(&mut self, fb: &mut FrameBuffer, dt: f32) {
        // --- Particles ---
        let w = fb.width as i32;
        let h = fb.height as i32;

        // Update positions and lifetimes; draw as warm glows
        let mut i = 0;
        while i < self.particles.len() {
            let p = &mut self.particles[i];

            // Integrate motion (simple Euler)
            p.x += p.vx * dt;
            p.y += p.vy * dt;

            // Gentle drag + slight downward pull (looks lively)
            p.vx *= 0.98;
            p.vy = p.vy * 0.98 + 10.0 * dt; // gravity-ish

            p.life -= dt;

            // Render: radius shrinks with life; brightness fades with (life/max_life)
            if p.alive() {
                let life01 = (p.life / p.max_life).clamp(0.0, 1.0);
                let radius = (6.0 * life01 + 2.0) as i32;            // 2..8 px
                let strength = (0.9 * p.energy * life01).clamp(0.0, 1.0);

                // Warm gold color
                let base_r = 255u8;
                let base_g = 200u8;
                let base_b = 80u8;

                draw_additive_disc(fb, p.x as i32, p.y as i32, radius, base_r, base_g, base_b, strength);
                i += 1;
            } else {
                // Remove dead particle (swap-remove, O(1))
                self.particles.swap_remove(i);
            }
        }

        // --- Lightning bolt ---
        if let Some(b) = &mut self.bolt {
            // Fade bolt quickly
            b.ttl -= dt;

            // Draw along segments as a sequence of small bright discs
            let base_r = 210u8;
            let base_g = 230u8;
            let base_b = 255u8;

            // Strength decays over time
            let s = (b.ttl / 0.10).clamp(0.0, 1.0);

            for seg in 0..(b.pts.len().saturating_sub(1)) {
                let (x0, y0) = b.pts[seg];
                let (x1, y1) = b.pts[seg + 1];

                // Stamp small discs along the segment for a thick, glowy line
                let dx = x1 - x0;
                let dy = y1 - y0;
                let dist = (dx*dx + dy*dy).sqrt().max(1.0);
                let steps = (dist / 2.0).ceil() as i32; // stamp every ~2 px
                for i in 0..=steps {
                    let t = i as f32 / steps as f32;
                    let x = x0 + dx * t;
                    let y = y0 + dy * t;
                    draw_additive_disc(fb, x as i32, y as i32, 3, base_r, base_g, base_b, 1.2 * s);
                }
            }

            // Kill bolt when time runs out
            if b.ttl <= 0.0 { self.bolt = None; }
        }

        // Clamp not strictly needed—our additive helper already clamps per channel.
        // Out-of-bounds stamping is guarded, so the screen won’t flicker.
        let _ = (w, h); // silence unused warnings if optimized paths change
    }
}
