// What you SEE now:
// • Live camera is always the base image.
// • Hold Left Mouse: you "paint blur" into the live feed (soft edges).
// • B toggles "show BLUR" (debug): the fully blurred live frame for this instant.
// • C clears the painted mask. ESC quits.
// • (R is unused now.)

mod camera;
mod draw;
mod error;
mod types;
mod vision;
mod gamma;
mod fx;

use camera::CameraCapture;
use draw::{draw_crosshair, draw_text_5x7, Drawer};
use error::Error;
use gamma::GammaLut;
use std::time::{Duration, Instant};
use types::{FrameBuffer, Mask};
use vision::{box_blur_rgb, blend_linear_in_place};
use fx::Fx;

fn main() -> Result<(), Error> {
    /* --- Camera + window setup ---
       Visual: window opens with live camera feed. */
    let mut cam = CameraCapture::new(0, 640, 480)?;
    let (w, h) = cam.resolution();
    let mut drawer = Drawer::new("Magic Eraser — Blur Brush", w as usize, h as usize)?;

    /* --- Reusable screen buffer ---
       Visual: this is the image you actually see each frame. */
    let mut screen = FrameBuffer {
        width:  w as usize,
        height: h as usize,
        pixels: vec![0u32; (w as usize) * (h as usize)],
    };

    /* --- Blur buffers (reused every frame) ---
       Visual: `blur_tmp` is invisible scratch; `blur_sink` becomes BLUR(LIVE). */
    let mut blur_tmp = FrameBuffer { width: screen.width, height: screen.height, pixels: vec![0u32; screen.pixels.len()] };
    let mut blur_sink = FrameBuffer { width: screen.width, height: screen.height, pixels: vec![0u32; screen.pixels.len()] };
    let blur_radius: usize = 8; // visual: softness of the blur brush (bigger = softer/slower)

    /* --- Gamma LUT (fast linear-light blend) ---
       Visual: seamless edges with no halos when mixing blur into live. */
    let lut = GammaLut::new();

    /* --- Mask & brush stamp (same as before) ---
       Visual: α mask controls where blur appears (1=blur, 0=raw live). */
    let mut mask = Mask { width: screen.width, height: screen.height, alpha: vec![0.0; screen.pixels.len()] };
    let eraser_radius: i32 = 22;       // visual: brush size in pixels
    let sigma: f32 = eraser_radius as f32 * 0.5; // visual: feather softness
    let stamp = vision::make_gaussian_stamp(eraser_radius, sigma);
    let mut mask_has_any = false;      // visual: if false, we skip blending (faster)

    /* --- FX (sparkles/lightning) ---
       Visual: glows around your brush while painting; fades on its own. */
    let mut fx = Fx::new(600);

    /* --- HUD / FPS ---
       Visual: small text shows mode hints + FPS. */
    let mut last_fps_time = Instant::now();
    let mut frames_this_second: u32 = 0;
    let mut hud_fps_text = String::from("FPS: 0.0");
    let mut last_frame_time = Instant::now();

    /* --- Debug toggles ---
       Visual: B shows the full blurred frame; helpful to verify blur itself. */
    let mut show_blur = false;

    /* ------------------------------ Main loop ------------------------------ */
    while drawer.is_open() && !drawer.esc_pressed() {
        let now = Instant::now();
        let dt = (now - last_frame_time).as_secs_f32(); // visual: drives FX timing
        last_frame_time = now;

        /* 1) Grab a fresh live frame (what the camera sees right now).
           Visual: this is the raw base we’ll start from. */
        let live = cam.next_frame()?; // immutable here; we copy it into screen below

        /* 2) Inputs */
        if drawer.b_pressed_once() { show_blur = !show_blur; } // visual: toggles BLUR preview (debug)
        if drawer.c_pressed_once() {                           // visual: eraser cleared (blur disappears)
            for a in &mut mask.alpha { *a = 0.0; }
            mask_has_any = false;
        }

        // Paint when holding left mouse: α grows under the cursor (soft edges).
        let mut erasing_now = false;
        if drawer.left_mouse_down() {
            if let Some((mx, my)) = drawer.mouse_pos() {
                vision::dab_mask(&mut mask, mx as i32, my as i32, &stamp); // visual: mask accumulates
                mask_has_any = true;                                       // visual: enables blending
                erasing_now = true;
                fx.spawn_sparkles(mx as f32, my as f32, 12);               // visual: glows appear
                fx.maybe_spawn_bolt(mx as f32, my as f32);
            }
        }

        /* 3) Build the blurred sink from the live frame (BLUR(LIVE)).
           Visual: not shown directly unless B is on; used for eraser mixing. */
        box_blur_rgb(&live, &mut blur_tmp, &mut blur_sink, blur_radius)?;

        /* 4) Choose what to show as the base image this frame. */
        if show_blur {
            // Visual: full-screen blurred camera (debug view)
            screen.pixels.copy_from_slice(&blur_sink.pixels);
        } else {
            // Visual: raw live camera
            screen.pixels.copy_from_slice(&live.pixels);
        }

        /* 5) If we have any painted mask, blend BLUR into LIVE where α>0.
           Visual: you “paint blur” into the live feed with soft edges. */
        if !show_blur && mask_has_any {
            blend_linear_in_place(&mut screen, &blur_sink, &mask, &lut)?; // visual: blur appears under brush
        }

        /* 6) FX on top (sparkles/bolt), crosshair, HUD text */
        fx.update_and_render(&mut screen, dt);                             // visual: glows fade & drift

        if let Some((mx, my)) = drawer.mouse_pos() {
            draw_crosshair(&mut screen, mx as i32, my as i32, 12, 0x00_FF_CC_33); // visual: yellow + at cursor
        }

        let status = if show_blur { "BLUR (Showing)" } else { "LIVE" };    // visual: left HUD tag
        let hint = if erasing_now { " | LMB: painting blur…  C: clear  B: show BLUR" }
                   else            { " | LMB: paint blur     C: clear  B: show BLUR" };
        let hud = format!("{}{} | {}", status, hint, hud_fps_text);
        draw_text_5x7(&mut screen, 8, 8, &hud, 0x00_FF_FF_FF);             // visual: small white HUD

        /* 7) Present to the window (this is when the on-screen image updates). */
        drawer.present(&screen)?;

        /* 8) FPS counter (prints to terminal + HUD once per second) */
        frames_this_second += 1;
        if now.duration_since(last_fps_time) >= Duration::from_secs(1) {
            let secs = now.duration_since(last_fps_time).as_secs_f32();
            let fps = frames_this_second as f32 / secs;
            println!("FPS: {:.1}", fps);                   // terminal
            hud_fps_text = format!("FPS: {:.1}", fps);     // HUD part
            frames_this_second = 0;
            last_fps_time = now;
        }
    }

    Ok(())
}
