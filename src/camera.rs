// Opens the default camera and converts frames into a buffer suitable for the window.
// Visual expectation: when main.rs calls `next_frame()`, you get a
// Vec<u32> where each pixel is 0x00RRGGBB, ready to push to the screen.

use crate::error::Error;
use crate::types::FrameBuffer;

// Bring in nokhwa types for camera control.
use nokhwa::{
    Camera,
    pixel_format::RgbFormat,
    utils::{
        CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType, Resolution,
    },
};

// We also use `image` crate types to help decode frames cleanly when needed.
use image::{ImageBuffer, Rgb};

// A small wrapper around nokhwa::Camera so our main loop stays clean.
pub struct CameraCapture {
    cam: Camera,
    width: u32,
    height: u32,
}

impl CameraCapture {
    /// Try to open camera index 0 at a target resolution (falls back if not exact).
    /// On success, nothing is shown on screen yet — we just hold an open stream.

    pub fn new(index: u32, width: u32, height: u32) -> Result<Self, Error> {
        // 1) Choose the device (0 = default webcam)
        let idx = CameraIndex::Index(index);

        let fmt = CameraFormat::new(
            Resolution::new(width, height),
            FrameFormat::YUYV, // uncompressed; cheap to convert to RGB
            30,                // target FPS
        );

         // 2) Ask for RGB frames, prioritizing the highest frame rate near our request.
        let req = RequestedFormat::new::<RgbFormat>(
            RequestedFormatType::Closest(fmt)
        );

        // let req = RequestedFormat::new::<RgbFormat>(
        //     RequestedFormatType::HighestResolution(
        //         Resolution::new(width, height),
        //     )
        // );

        // 3) Create the camera (this might fail if no device exists).
        let mut cam =
            Camera::new(idx, req)
            .map_err(|e| Error::CameraInit(format!("Create camera: {e}")))?;

        // 4) Start streaming frames from the camera.
        cam.open_stream()
            .map_err(|e| Error::CameraInit(format!("Open stream: {e}")))?;

        // 5) The actual stream might choose a slightly different resolution.
        let actual = cam.resolution();

        Ok(Self {
            cam,
            width: actual.width(),
            height: actual.height(),
        })
    }

    /// Grab one frame from the camera and convert it to 0x00RRGGBB pixels.
    /// What you’ll see: after main.rs pushes this buffer to the window,
    /// the live camera image updates by one frame.
    pub fn next_frame(&mut self) -> Result<FrameBuffer, Error> {
        // 1) Pull a frame from the camera (this blocks until a new frame is ready).
        let frame = self
            .cam
            .frame()
            .map_err(|e| Error::CameraFrame(format!("Fetch frame: {e}")))?;

        // 2) Decode to an ImageBuffer<Rgb<u8>, Vec<u8>> (handles various raw formats safely).
        let rgb_img = frame
            .decode_image::<RgbFormat>() // tells nokhwa to produce ImageBuffer<Rgb<u8>, Vec<u8>>
            .map_err(|e| Error::CameraFrame(format!("Decode RGB: {e}")))?;

        // 3) Prepare the pixel buffer for the window (u32 per pixel, 0x00RRGGBB).
        //    You won't see anything yet; this just builds the data in RAM.
        let (w, h) = rgb_img.dimensions();
        let mut out = Vec::with_capacity((w as usize) * (h as usize));
        for (_x, _y, pixel) in rgb_img.enumerate_pixels() {
            // Each `pixel` is RGB<u8>. We pack it as 0x00RRGGBB.
            let r = pixel[0] as u32;
            let g = pixel[1] as u32;
            let b = pixel[2] as u32;
            out.push((r << 16) | (g << 8) | b);
        }

        Ok(FrameBuffer {
            width: w as usize,
            height: h as usize,
            pixels: out,
        })
    }

    /// Report the actual resolution the camera is delivering.
    pub fn resolution(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}
