// Step 5 main: FX (sparkles + lightning) layered on top of your current pipeline.

mod camera;
mod draw;
mod error;
mod types;
mod vision;
mod fx;          // NEW

use camera::CameraCapture;
use draw::{draw_crosshair, draw_text_5x7, Drawer};
use error::Error;
use std::time::{Duration, Instant};
use types::{FrameBuffer, Mask};
use vision::{
    clear_mask, dab_mask, make_gaussian_stamp, median_background, blend_linear_in_place,
    BG_CAPTURE_COUNT,
};
use fx::Fx;      // NEW

fn main() -> Result<(), Error> {
    // --- Camera + Window setup ---
    let mut cam = CameraCapture::new(0, 640, 480)?;
    let (w, h) = cam.resolution();
    let mut drawer = Drawer::new("Magic Eraser — Step 5 (FX Sparkles + Lightning)", w as usize, h as usize)?;

    // --- FPS accounting for HUD + terminal ---
    let mut last_fps_time = Instant::now();
    let mut frames_this_second: u32 = 0;
    let mut hud_fps_text = String::from("FPS: 0.0");

    // --- Frame delta time for FX simulation ---
    let mut last_frame_time = Instant::now();

    // --- Step 3 state (capture BG) ---
    let mut capturing_bg = false;
    let mut captured: Vec<FrameBuffer> = Vec::new();
    let mut bg_image: Option<FrameBuffer> = None;
    let mut show_bg = false;

    // --- Step 4 state (eraser) ---
    let mut mask = Mask {
        width: w as usize,
        height: h as usize,
        alpha: vec![0.0; (w as usize) * (h as usize)],
    };
    let eraser_radius: i32 = 22;
    let sigma: f32 = eraser_radius as f32 * 0.5;
    let stamp = make_gaussian_stamp(eraser_radius, sigma);

    // --- Step 5 state (FX) ---
    // Visual: capacity caps total sparkles on screen; tune for performance.
    let mut fx = Fx::new(600);

    // --- Main loop ---
    while drawer.is_open() && !drawer.esc_pressed() {
        // dt for FX
        let now_frame = Instant::now();
        let dt = (now_frame - last_frame_time).as_secs_f32();
        last_frame_time = now_frame;

        // 1) Pull a fresh live frame
        let mut live = cam.next_frame()?; // mutable: HUD/crosshair and blending will change it

        // 2) Inputs
        if drawer.r_pressed_once() && !capturing_bg {
            capturing_bg = true;
            captured.clear();
            bg_image = None;
            clear_mask(&mut mask); // reset erase when starting a new BG
        }
        if drawer.b_pressed_once() { show_bg = !show_bg; }
        if drawer.c_pressed_once() { clear_mask(&mut mask); } // clears erase only

        // 3) Capture flow
        if capturing_bg {
            captured.push(live.clone());
            if captured.len() >= BG_CAPTURE_COUNT {
                let bg = median_background(&captured)?;
                bg_image = Some(bg);
                capturing_bg = false;
            }
        }

        // 4) Eraser dab + FX spawn when LMB held and BG exists
        let mut erasing_now = false;
        if let (Some(_bg), true) = (bg_image.as_ref(), drawer.left_mouse_down()) {
            if let Some((mx, my)) = drawer.mouse_pos() {
                dab_mask(&mut mask, mx as i32, my as i32, &stamp); // eraser grows here
                erasing_now = true;

                // --- FX spawns at the cursor while erasing ---
                // Visual: sparkles pop around the eraser location
                fx.spawn_sparkles(mx as f32, my as f32, 28);

                // Visual: occasionally a lightning bolt cracks near the cursor
                fx.maybe_spawn_bolt(mx as f32, my as f32);
            }
        }

        // 5) Compose the base frame to show (same logic as Step 4)
        let mut screen = if show_bg {
            if let Some(bg) = &bg_image { bg.clone() } else { live.clone() }
        } else if let Some(bg) = &bg_image {
            let mut blended = live.clone();
            blend_linear_in_place(&mut blended, bg, &mask)?; // magic erase
            blended
        } else {
            live.clone()
        };

        // 6) Update & render FX on top (additive glow/bolt)
        fx.update_and_render(&mut screen, dt);

        // 7) Crosshair + HUD
        if let Some((mx, my)) = drawer.mouse_pos() {
            let yellow = 0x00_FF_CC_33;
            draw_crosshair(&mut screen, mx as i32, my as i32, 12, yellow);
        }

        // HUD status line
        let status = if capturing_bg {
            format!("CAPTURING ({}/{})", captured.len(), BG_CAPTURE_COUNT)
        } else if bg_image.is_some() {
            if show_bg { "BG READY (Showing)".to_string() } else { "BG READY".to_string() }
        } else {
            "IDLE".to_string()
        };
        let hint = if bg_image.is_some() {
            if erasing_now { " | LMB: erasing…  C: clear  FX" } else { " | LMB: erase  C: clear  FX" }
        } else {
            " | Press R to capture BG"
        };
        let hud = format!("{}{} | {}", status, hint, hud_fps_text);
        let white = 0x00_FF_FF_FF;
        draw_text_5x7(&mut screen, 8, 8, &hud, white);

        // 8) Present to screen
        drawer.present(&mut screen)?;

        // 9) FPS accounting (terminal + HUD)
        frames_this_second += 1;
        let now = Instant::now();
        if now.duration_since(last_fps_time) >= Duration::from_secs(1) {
            let secs = now.duration_since(last_fps_time).as_secs_f32();
            let fps = frames_this_second as f32 / secs;
            println!("FPS: {:.1}", fps);
            hud_fps_text = format!("FPS: {:.1}", fps);
            frames_this_second = 0;
            last_fps_time = now;
        }
    }

    Ok(())
}
