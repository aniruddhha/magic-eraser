// A tiny error type so we don't rely on anyhow/thiserror.
// Every variant states *where* things went wrong.
use std::fmt::{self, Display};

#[derive(Debug)]
pub enum Error {
    WindowInit(String),   // Creating the window failed
    WindowUpdate(String), // Updating the window buffer failed
    CameraInit(String),   // Opening/starting the camera failed
    CameraFrame(String),  // Grabbing/decoding a frame failed
}

impl Display for Error {
    // This decides how the error is printed to your console.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::WindowInit(s) => write!(f, "Window init error: {s}"),
            Error::WindowUpdate(s) => write!(f, "Window update error: {s}"),
            Error::CameraInit(s) => write!(f, "Camera init error: {s}"),
            Error::CameraFrame(s) => write!(f, "Camera frame error: {s}"),
        }
    }
}

// We don't implement std::error::Error for now to keep things minimal.
// It's easy to add later when we wire in more components.

