// Window + software drawing utilities.
// Visual effects provided here:
// 1) A window that shows your live camera image.
// 2) A crosshair that follows your mouse.
// 3) A tiny 5x7 bitmap font to render HUD text on top of the video.

use crate::error::Error;
use crate::types::FrameBuffer;
use minifb::{Key, KeyRepeat, MouseButton, MouseMode, Window, WindowOptions};

pub struct Drawer {
    window: Window, // the on-screen window you see
}

impl Drawer {
    /// Create a window sized to the camera feed.
    /// Visual: a new empty window appears with your chosen title.
    pub fn new(title: &str, width: usize, height: usize) -> Result<Self, Error> {
        let window = Window::new(title, width, height, WindowOptions::default())
            .map_err(|e| Error::WindowInit(e.to_string()))?;
        Ok(Self { window })
    }

    /// Push the pixels for this frame to the screen.
    /// Visual: the window immediately displays the new image (live video).
    pub fn present(&mut self, framebuffer: &FrameBuffer) -> Result<(), Error> {
        self.window
            .update_with_buffer(&framebuffer.pixels, framebuffer.width, framebuffer.height)
            .map_err(|e| Error::WindowUpdate(e.to_string()))?;
        Ok(())
    }

    /// Returns false when the user closes the window (so we can stop the loop).
    pub fn is_open(&self) -> bool {
        self.window.is_open()
    }

    /// True while ESC is held down (we’ll exit when this is pressed).
    pub fn esc_pressed(&self) -> bool {
        self.window.is_key_down(Key::Escape)
    }

    /// Current mouse position in window pixel coordinates (clamped to the window).
    /// Visual: when this returns Some(x,y), your crosshair will be drawn at that pixel.
    pub fn mouse_pos(&self) -> Option<(usize, usize)> {
        self.window
            .get_mouse_pos(MouseMode::Clamp)
            .map(|(x, y)| (x.max(0.0) as usize, y.max(0.0) as usize))
    }

    // when this returns true, we will *start* capturing the BG.
    pub fn r_pressed_once(&self) -> bool {
        self.window.is_key_pressed(Key::R, KeyRepeat::No)
    }

    // we flip a boolean in main to switch displayed buffer.
    pub fn b_pressed_once(&self) -> bool {
        self.window.is_key_pressed(Key::B, KeyRepeat::No)
    }

    // Step 4 helpers
    /// Visual: when true, dabbing occurs at the mouse position (you see erase happening).
    pub fn left_mouse_down(&self) -> bool {
        self.window.get_mouse_down(MouseButton::Left)
    }

    /// Visual: when pressed, the current erase mask is cleared (screen looks un-erased again).
    pub fn c_pressed_once(&self) -> bool { self.window.is_key_pressed(Key::C, KeyRepeat::No) }

}

/* ---------- Software drawing: pixels, crosshair, tiny bitmap font ---------- */

/// Put a pixel on the framebuffer if (x,y) is inside bounds.
/// Visual: the exact pixel at (x,y) changes color.
#[inline]
fn put_pixel(fb: &mut FrameBuffer, x: i32, y: i32, color: u32) {
    if x < 0 || y < 0 {
        return;
    }
    let (x, y) = (x as usize, y as usize);
    if x >= fb.width || y >= fb.height {
        return;
    }
    let idx = y * fb.width + x;
    fb.pixels[idx] = color;
}

/// Draw a thin line between (x0,y0) and (x1,y1) using Bresenham.
/// Visual: a straight 1-pixel line appears on top of the camera image.
fn draw_line(fb: &mut FrameBuffer, x0: i32, y0: i32, x1: i32, y1: i32, color: u32) {
    let (mut x0, mut y0, x1, y1) = (x0, y0, x1, y1);
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        put_pixel(fb, x0, y0, color);
        if x0 == x1 && y0 == y1 { break; }
        let e2 = 2 * err;
        if e2 >= dy { err += dy; x0 += sx; }
        if e2 <= dx { err += dx; y0 += sy; }
    }
}

/// Draw a small crosshair centered at (cx,cy).
/// Visual: a “+” shape (with a tiny gap at the center) follows your mouse.
pub fn draw_crosshair(fb: &mut FrameBuffer, cx: i32, cy: i32, size: i32, color: u32) {
    // Horizontal line (left part)
    draw_line(fb, cx - size, cy, cx - 2, cy, color);
    // Horizontal line (right part)
    draw_line(fb, cx + 2, cy, cx + size, cy, color);
    // Vertical line (top part)
    draw_line(fb, cx, cy - size, cx, cy - 2, color);
    // Vertical line (bottom part)
    draw_line(fb, cx, cy + 2, cx, cy + size, color);
    // Small center dot to anchor the crosshair visually
    put_pixel(fb, cx, cy, color);
}

/* ---------- 5x7 bitmap font (ASCII subset we need for "IDLE | FPS: 00.0") ---------- */

/// Return a 5x7 glyph bitmap for a limited character set.
/// Each u8 is a row; the low 5 bits are the pixels (bit 4 = leftmost).
fn glyph5x7(ch: char) -> Option<[u8; 7]> {
    // Helper macro to define a glyph quickly
    macro_rules! g { ($a:expr,$b:expr,$c:expr,$d:expr,$e:expr,$f:expr,$g:expr) => {
        Some([$a,$b,$c,$d,$e,$f,$g])
    }; }

    match ch {
        // Digits 0..9
        '0' => g!(0b01110,0b10001,0b10011,0b10101,0b11001,0b10001,0b01110),
        '1' => g!(0b00100,0b01100,0b00100,0b00100,0b00100,0b00100,0b01110),
        '2' => g!(0b01110,0b10001,0b00001,0b00010,0b00100,0b01000,0b11111),
        '3' => g!(0b11110,0b00001,0b00001,0b01110,0b00001,0b00001,0b11110),
        '4' => g!(0b00010,0b00110,0b01010,0b10010,0b11111,0b00010,0b00010),
        '5' => g!(0b11111,0b10000,0b11110,0b00001,0b00001,0b10001,0b01110),
        '6' => g!(0b00110,0b01000,0b10000,0b11110,0b10001,0b10001,0b01110),
        '7' => g!(0b11111,0b00001,0b00010,0b00100,0b01000,0b01000,0b01000),
        '8' => g!(0b01110,0b10001,0b10001,0b01110,0b10001,0b10001,0b01110),
        '9' => g!(0b01110,0b10001,0b10001,0b01111,0b00001,0b00010,0b01100),

        // Uppercase letters we need: I D L E F P S
        'I' => g!(0b01110,0b00100,0b00100,0b00100,0b00100,0b00100,0b01110),
        'D' => g!(0b11100,0b10010,0b10001,0b10001,0b10001,0b10010,0b11100),
        'L' => g!(0b10000,0b10000,0b10000,0b10000,0b10000,0b10000,0b11111),
        'E' => g!(0b11111,0b10000,0b10000,0b11110,0b10000,0b10000,0b11111),
        'F' => g!(0b11111,0b10000,0b10000,0b11110,0b10000,0b10000,0b10000),
        'P' => g!(0b11110,0b10001,0b10001,0b11110,0b10000,0b10000,0b10000),
        'S' => g!(0b01111,0b10000,0b10000,0b01110,0b00001,0b00001,0b11110),

        // Punctuation: space, vertical bar, colon, dot
        ' ' => g!(0b00000,0b00000,0b00000,0b00000,0b00000,0b00000,0b00000),
        '|' => g!(0b00100,0b00100,0b00100,0b00100,0b00100,0b00100,0b00100),
        ':' => g!(0b00000,0b00100,0b00000,0b00000,0b00100,0b00000,0b00000),
        '.' => g!(0b00000,0b00000,0b00000,0b00000,0b00000,0b00100,0b00000),

        _ => None,
    }
}

/// Draw a single 5x7 character at (x,y).
/// Visual: a tiny white glyph appears with a 1-pixel black shadow for contrast.
fn draw_char_5x7(fb: &mut FrameBuffer, x: i32, y: i32, ch: char, color: u32) {
    if let Some(rows) = glyph5x7(ch) {
        // Shadow pass: offset by (1,1) in black to improve readability
        for (ry, rowbits) in rows.iter().enumerate() {
            for rx in 0..5 {
                if (rowbits & (1 << (4 - rx))) != 0 {
                    put_pixel(fb, x + rx as i32 + 1, y + ry as i32 + 1, 0x00000000);
                }
            }
        }

        // Foreground pass: actual glyph in chosen color
        for (ry, rowbits) in rows.iter().enumerate() {
            for rx in 0..5 {
                if (rowbits & (1 << (4 - rx))) != 0 {
                    put_pixel(fb, x + rx as i32, y + ry as i32, color);
                }
            }
        }
    }
}

/// Draw a text string using 5x7 glyphs.
/// Visual: a compact HUD string appears; each glyph is 5x7 with 1-pixel spacing.
pub fn draw_text_5x7(fb: &mut FrameBuffer, mut x: i32, y: i32, text: &str, color: u32) {
    for ch in text.chars() {
        draw_char_5x7(fb, x, y, ch, color);
        x += 6; // 5 pixels glyph width + 1 pixel spacing
    }
}
