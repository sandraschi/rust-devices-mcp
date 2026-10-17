//! USB webcam frames via `nokhwa` (MediaFoundation on Windows).
//!
//! Fast path: MJPEG stream, whose frame buffer IS a complete JPEG — no
//! decode, no re-encode. Fallback: default format → RGB decode → JPEG
//! encode (some devices refuse MJPEG outright). The blocking capture runs on
//! a `spawn_blocking` worker so the Tokio runtime never stalls.
use crate::error::DeviceError;
use nokhwa::utils::{CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType};
use nokhwa_core::pixel_format::RgbFormat;

pub async fn grab(index: u32) -> Result<Vec<u8>, DeviceError> {
    tokio::task::spawn_blocking(move || grab_blocking(index))
        .await
        .map_err(|e| DeviceError::Protocol(format!("usb capture task: {e}")))?
}

fn grab_blocking(index: u32) -> Result<Vec<u8>, DeviceError> {
    let mjpeg = RequestedFormat::with_formats(
        RequestedFormatType::AbsoluteHighestFrameRate,
        &[FrameFormat::MJPEG],
    );
    match open_grab(index, mjpeg) {
        Ok(bytes) => Ok(bytes),
        Err(first) => {
            // Device refused MJPEG: take whatever it offers and encode JPEG.
            let any =
                RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestFrameRate);
            let mut camera = nokhwa::Camera::new(CameraIndex::Index(index), any)
                .map_err(|e| DeviceError::Protocol(format!("usb camera {index}: {e}")))?;
            camera
                .open_stream()
                .map_err(|e| DeviceError::Protocol(format!("usb camera {index} stream: {e}")))?;
            let frame = camera
                .frame()
                .map_err(|e| DeviceError::Protocol(format!("usb camera {index} frame: {e}")))?;
            let rgb = frame
                .decode_image::<RgbFormat>()
                .map_err(|e| DeviceError::Protocol(format!("usb camera {index} decode: {e}")))?;
            let (width, height) = (rgb.width(), rgb.height());
            let mut out = Vec::new();
            image::codecs::jpeg::JpegEncoder::new(&mut out)
                .encode(
                    &rgb.into_raw(),
                    width,
                    height,
                    image::ExtendedColorType::Rgb8,
                )
                .map_err(|e| DeviceError::Protocol(format!("usb camera {index} encode: {e}")))?;
            let _ = first;
            Ok(out)
        }
    }
}

fn open_grab(index: u32, format: RequestedFormat<'_>) -> Result<Vec<u8>, DeviceError> {
    let mut camera = nokhwa::Camera::new(CameraIndex::Index(index), format)
        .map_err(|e| DeviceError::Protocol(format!("usb camera {index}: {e}")))?;
    camera
        .open_stream()
        .map_err(|e| DeviceError::Protocol(format!("usb camera {index} stream: {e}")))?;
    camera
        .frame()
        .map(|frame| frame.buffer().to_vec())
        .map_err(|e| DeviceError::Protocol(format!("usb camera {index} frame: {e}")))
}
