//! Live video: RTSP/H264 (retina) + decode (openh264), no ffmpeg.
//!
//! Two pumps feed one frame channel. `pump_rtsp` runs a retina RTSP
//! session into openh264 (Tapo/ONVIF cameras, raw RTSP URLs).
//! `pump_snapshots` repeats `camera::snapshot_full` grabs (~0.5 fps) for
//! sources without streams (Ring, USB, public URLs, demo). The UI owns
//! the worker task and aborts it on Stop.
use crate::creds::{classify_vendor_error, with_timeout_secs};
use crate::error::DeviceError;
use futures::StreamExt;

#[derive(Debug, Clone)]
pub struct RgbFrame {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct StreamRequest {
    pub url: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

/// RTSP URL for Tapo cameras (main stream). Deliberately credential-free:
/// retina rejects userinfo in URLs ("URL must not contain credentials") —
/// login travels via `SessionOptions::creds`, never the URL string.
pub fn tapo_rtsp_url(ip: &str) -> Result<String, DeviceError> {
    url::Url::parse(&format!("rtsp://{ip}:554/stream1"))
        .map(|url| url.to_string())
        .map_err(|e| DeviceError::InvalidInput(format!("bad camera ip '{ip}': {e}")))
}

/// Strip credentials from a stream URI (e.g. ONVIF GetStreamUri responses
/// that embed `rtsp://user:pass@host/…`). retina refuses credentialed URLs;
/// login travels via session options, and this also keeps secrets out of
/// logs and error strings.
pub fn strip_creds(raw_url: &str) -> Result<String, DeviceError> {
    let mut url = url::Url::parse(raw_url)
        .map_err(|e| DeviceError::Protocol(format!("bad stream uri: {e}")))?;
    url.set_username("")
        .map_err(|_| DeviceError::Protocol("bad stream uri userinfo".to_string()))?;
    url.set_password(None)
        .map_err(|_| DeviceError::Protocol("bad stream uri userinfo".to_string()))?;
    Ok(url.to_string())
}

/// Length-prefixed (AVCC) access units → Annex B. Passes Annex B through.
/// retina's default framing is FourByteLength; openh264 wants Annex B.
fn to_annex_b(data: &[u8]) -> Result<Vec<u8>, DeviceError> {
    if data.len() >= 4
        && data[0] == 0
        && data[1] == 0
        && (data[2] == 1 || (data[2] == 0 && data[3] == 1))
    {
        return Ok(data.to_vec());
    }
    let mut out = Vec::with_capacity(data.len() + 64);
    let mut pos = 0usize;
    while pos + 4 <= data.len() {
        let len =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;
        let end = pos
            .checked_add(len)
            .ok_or_else(|| DeviceError::Protocol("rtsp: corrupt NAL length prefix".to_string()))?;
        if end > data.len() {
            return Err(DeviceError::Protocol(
                "rtsp: NAL overruns its frame".to_string(),
            ));
        }
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(&data[pos..end]);
        pos = end;
    }
    if out.is_empty() {
        return Err(DeviceError::Protocol(
            "rtsp: empty picture data".to_string(),
        ));
    }
    Ok(out)
}

fn decode_rgb(
    decoder: &mut openh264::decoder::Decoder,
    annexb: &[u8],
) -> Result<Option<(u32, u32, Vec<u8>)>, DeviceError> {
    use openh264::formats::YUVSource;
    let picture = decoder
        .decode(annexb)
        .map_err(|e| DeviceError::Protocol(format!("h264 decode: {e}")))?;
    let Some(yuv) = picture else {
        return Ok(None);
    };
    let (width, height) = yuv.dimensions();
    if width == 0 || height == 0 || width > 7680 || height > 4320 {
        return Err(DeviceError::Protocol(format!(
            "h264: absurd dimensions {width}x{height}"
        )));
    }
    let mut rgb = vec![0u8; width * height * 3];
    yuv.write_rgb8(&mut rgb);
    Ok(Some((width as u32, height as u32, rgb)))
}

/// ONVIF camera to RTSP pump: profiles → stream URI → credentials → pump.
pub async fn pump_onvif(
    ip: String,
    port: u16,
    username: String,
    password: String,
    frames: tokio::sync::mpsc::Sender<RgbFrame>,
) -> Result<(), DeviceError> {
    let uri = super::cam_onvif::stream_uri(&ip, port, &username, &password).await?;
    let url = strip_creds(&uri)?;
    pump_rtsp(
        StreamRequest {
            url,
            username: Some(username),
            password: Some(password),
        },
        frames,
    )
    .await
}

/// Run an RTSP session until the frame channel closes (UI stopped). Errors
/// end the pump with a typed error the UI surfaces; the session tears down
/// on drop.
pub async fn pump_rtsp(
    req: StreamRequest,
    frames: tokio::sync::mpsc::Sender<RgbFrame>,
) -> Result<(), DeviceError> {
    use retina::client::{Credentials, PlayOptions, Session, SessionOptions, SetupOptions};
    let url = url::Url::parse(&req.url)
        .map_err(|e| DeviceError::InvalidInput(format!("bad rtsp url: {e}")))?;
    let mut options = SessionOptions::default();
    if let (Some(user), Some(pass)) = (req.username.clone(), req.password.clone()) {
        options = options.creds(Some(Credentials {
            username: user,
            password: pass,
        }));
    }
    let described = with_timeout_secs("rtsp.describe", 15, Session::describe(url, options))
        .await?
        .map_err(|e| classify_vendor_error(format!("rtsp describe: {e}")))?;
    let video_index = described
        .streams()
        .iter()
        .position(|stream| stream.media() == "video")
        .ok_or_else(|| DeviceError::Unsupported("rtsp: no video stream offered".to_string()))?;
    let mut session = described;
    with_timeout_secs(
        "rtsp.setup",
        15,
        session.setup(video_index, SetupOptions::default()),
    )
    .await?
    .map_err(|e| classify_vendor_error(format!("rtsp setup: {e}")))?;
    let playing = with_timeout_secs("rtsp.play", 15, session.play(PlayOptions::default()))
        .await?
        .map_err(|e| classify_vendor_error(format!("rtsp play: {e}")))?;
    let mut demuxed = playing
        .demuxed()
        .map_err(|e| classify_vendor_error(format!("rtsp demux: {e}")))?;
    let mut decoder = openh264::decoder::Decoder::new()
        .map_err(|e| DeviceError::Protocol(format!("h264 decoder init: {e}")))?;
    loop {
        let item = demuxed.next().await;
        let Some(item) = item else {
            return Err(DeviceError::Protocol("rtsp: stream ended".to_string()));
        };
        let frame = item.map_err(|e| classify_vendor_error(format!("rtsp packet: {e}")))?;
        let retina::codec::CodecItem::VideoFrame(picture) = frame else {
            continue;
        };
        let annexb = to_annex_b(picture.data())?;
        let decoded = decode_rgb(&mut decoder, &annexb)?;
        if let Some((width, height, rgb)) = decoded {
            if frames.send(RgbFrame { width, height, rgb }).await.is_err() {
                return Ok(());
            }
        }
    }
}

/// Snapshot poller for non-RTSP sources: a real frame every ~2s over the
/// existing snapshot path (Ring, USB, URLs, demo).
/// Snapshot source without extra crates: an `Arc` over a boxed future.
type GrabFn = dyn Fn()
        -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<u8>, DeviceError>> + Send>>
    + Send
    + Sync;

#[derive(Clone)]
pub struct SnapshotPoller {
    grab: std::sync::Arc<GrabFn>,
}

impl SnapshotPoller {
    pub fn new<F, Fut>(grab: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<Vec<u8>, DeviceError>> + Send + 'static,
    {
        Self {
            grab: std::sync::Arc::new(move || Box::pin(grab())),
        }
    }

    async fn grab_jpeg(&self) -> Result<Vec<u8>, DeviceError> {
        (self.grab)().await
    }
}

pub async fn pump_snapshots(
    grab: SnapshotPoller,
    frames: tokio::sync::mpsc::Sender<RgbFrame>,
) -> Result<(), DeviceError> {
    loop {
        match grab.grab_jpeg().await {
            Ok(bytes) => {
                if let Some((width, height, rgb)) = jpeg_to_rgb(&bytes) {
                    if frames.send(RgbFrame { width, height, rgb }).await.is_err() {
                        return Ok(());
                    }
                }
            }
            Err(e) => {
                // One failed grab must not kill the player (transient blips).
                // The UI shows the last good frame; errors surface on stop.
                let _ = e;
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

fn jpeg_to_rgb(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let img = image::load_from_memory(bytes).ok()?;
    let rgb = img.to_rgb8();
    let (width, height) = (rgb.width(), rgb.height());
    if width == 0 || height == 0 || width > 7680 || height > 4320 {
        return None;
    }
    Some((width, height, rgb.into_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annex_b_passthrough_and_avcc_convert() {
        // Already Annex B stays untouched.
        let annexb = vec![0, 0, 0, 1, 0x65, 0x11, 0x22];
        assert_eq!(to_annex_b(&annexb).unwrap_or_default(), annexb);
        // AVCC (u32be len + NAL) converts.
        let avcc = vec![0, 0, 0, 3, 0x65, 0x11, 0x22];
        assert_eq!(
            to_annex_b(&avcc).unwrap_or_default(),
            vec![0, 0, 0, 1, 0x65, 0x11, 0x22]
        );
        // Truncated length prefix is a typed error, not a panic.
        assert!(to_annex_b(&[0, 0, 0, 9, 0x65]).is_err());
        assert!(to_annex_b(&[]).is_err());
    }

    #[test]
    fn tapo_url_is_credential_free() {
        // retina refuses userinfo in URLs — login travels via options.
        let url = tapo_rtsp_url("192.168.0.164").unwrap_or_default();
        assert_eq!(url, "rtsp://192.168.0.164:554/stream1");
        assert!(tapo_rtsp_url("not an ip!!").is_err());
    }

    #[test]
    fn strip_creds_removes_userinfo() {
        let url = strip_creds("rtsp://a:b@host/stream").unwrap_or_default();
        assert_eq!(url, "rtsp://host/stream");
        let plain = strip_creds("rtsp://host/stream").unwrap_or_default();
        assert_eq!(plain, "rtsp://host/stream");
    }

    /// Live hardware oracle (ignored by default): streams 3 decoded frames
    /// from a real Tapo camera. Needs TAPO_CAMERA_USERNAME/PASSWORD (+ optional
    /// TAPO_CAMERA_HOST) in the environment — run explicitly on the LAN.
    #[tokio::test]
    #[ignore = "live hardware: real Tapo camera + camera env required"]
    async fn live_tapo_stream_delivers_frames() {
        let user = std::env::var("TAPO_CAMERA_USERNAME").unwrap_or_default();
        let pass = std::env::var("TAPO_CAMERA_PASSWORD").unwrap_or_default();
        let ip = std::env::var("TAPO_CAMERA_HOST").unwrap_or("192.168.0.164".to_string());
        assert!(!user.is_empty() && !pass.is_empty(), "camera env missing");
        let url = tapo_rtsp_url(&ip).unwrap_or_default();
        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        let worker = tokio::spawn(pump_rtsp(
            StreamRequest {
                url,
                username: Some(user),
                password: Some(pass),
            },
            tx,
        ));
        let mut got = 0u32;
        let deadline = tokio::time::sleep(std::time::Duration::from_secs(60));
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                frame = rx.recv() => {
                    if frame.is_some() {
                        got += 1;
                    }
                    if got >= 3 {
                        break;
                    }
                }
                _ = &mut deadline => break,
            }
        }
        worker.abort();
        assert!(got >= 3, "only {got} frames in 60s");
    }

    #[tokio::test]
    async fn rtsp_to_closed_port_fails_typed_fast() {
        // Discard port: refusal must surface as a typed error, not a hang.
        let req = StreamRequest {
            url: "rtsp://127.0.0.1:9/stream1".to_string(),
            username: None,
            password: None,
        };
        let (tx, _rx) = tokio::sync::mpsc::channel(2);
        let result = with_timeout_secs("test", 20, pump_rtsp(req, tx)).await;
        assert!(matches!(result, Ok(Err(_))));
    }
}
