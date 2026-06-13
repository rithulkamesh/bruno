//! Primary display screenshot, scaled to 512x512.

use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::oneshot;

pub struct ScaledScreenshot {
    pub jpeg_base64: String,
    pub width: u32,
    pub height: u32,
}

const TARGET_SIZE: u32 = 512;

/// Send-safe handle for requesting screenshots from the UI thread.
#[derive(Clone)]
pub struct CaptureService {
    request_tx: Sender<oneshot::Sender<Option<ScaledScreenshot>>>,
}

pub fn pair() -> (CaptureService, Receiver<oneshot::Sender<Option<ScaledScreenshot>>>) {
    let (request_tx, request_rx) = mpsc::channel();
    (CaptureService { request_tx }, request_rx)
}

impl CaptureService {
    pub fn pair() -> (Self, Receiver<oneshot::Sender<Option<ScaledScreenshot>>>) {
        pair()
    }

    pub async fn capture_scaled(&self) -> Option<ScaledScreenshot> {
        let (tx, rx) = oneshot::channel();
        self.request_tx.send(tx).ok()?;
        rx.await.ok().flatten()
    }
}

/// Non-blocking capture worker; keep one instance on the UI thread.
pub struct CapturePollState {
    queued: VecDeque<oneshot::Sender<Option<ScaledScreenshot>>>,
}

impl CapturePollState {
    pub fn new() -> Self {
        Self {
            queued: VecDeque::new(),
        }
    }
}

impl Default for CapturePollState {
    fn default() -> Self {
        Self::new()
    }
}

/// Call from the main-thread event loop to fulfill pending capture requests.
pub fn poll_capture_requests(
    rx: &Receiver<oneshot::Sender<Option<ScaledScreenshot>>>,
    state: &mut CapturePollState,
) {
    while let Ok(reply_tx) = rx.try_recv() {
        state.queued.push_back(reply_tx);
    }

    // One synchronous capture per frame to avoid blocking the event loop for long.
    if let Some(reply) = state.queued.pop_front() {
        let result = platform::capture_on_main_thread();
        let _ = reply.send(result);
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use core_foundation::data::CFData;
    use core_graphics::display::CGDisplay;
    use image::{imageops::FilterType, DynamicImage, ImageFormat, RgbaImage};
    use tracing::warn;

    use super::{ScaledScreenshot, TARGET_SIZE};

    pub fn capture_on_main_thread() -> Option<ScaledScreenshot> {
        let image = CGDisplay::main().image()?;
        let width = image.width() as u32;
        let height = image.height() as u32;
        if width == 0 || height == 0 {
            warn!("display screenshot empty");
            return None;
        }

        let rgba = rgba_from_cgimage(&image, width, height)?;
        Some(scale_and_encode((width, height, rgba)))
    }

    fn rgba_from_cgimage(
        image: &core_graphics::image::CGImage,
        width: u32,
        height: u32,
    ) -> Option<Vec<u8>> {
        let bytes_per_row = image.bytes_per_row();
        let data: CFData = image.data();
        let src = data.bytes();

        // CGDisplayCreateImage typically returns 32-bit BGRA (noneSkipFirst).
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height as usize {
            let row_start = y * bytes_per_row;
            for x in 0..width as usize {
                let i = row_start + x * 4;
                if i + 3 >= src.len() {
                    return None;
                }
                rgba.push(src[i + 2]); // R
                rgba.push(src[i + 1]); // G
                rgba.push(src[i]);     // B
                rgba.push(255);        // A
            }
        }
        Some(rgba)
    }

    fn scale_and_encode((width, height, rgba): (u32, u32, Vec<u8>)) -> ScaledScreenshot {
        let img = RgbaImage::from_raw(width, height, rgba).unwrap_or_else(|| RgbaImage::new(1, 1));
        let dynamic = DynamicImage::ImageRgba8(img);
        let resized = dynamic.resize_exact(TARGET_SIZE, TARGET_SIZE, FilterType::Triangle);
        let mut jpeg_bytes = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut jpeg_bytes);
        resized
            .write_to(&mut cursor, ImageFormat::Jpeg)
            .unwrap_or(());
        ScaledScreenshot {
            jpeg_base64: STANDARD.encode(jpeg_bytes),
            width: TARGET_SIZE,
            height: TARGET_SIZE,
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::ScaledScreenshot;

    pub fn capture_on_main_thread() -> Option<ScaledScreenshot> {
        None
    }
}
