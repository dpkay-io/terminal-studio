//! Streaming download of a release asset with progress reporting.

use std::sync::Arc;

use parking_lot::Mutex;

use super::{UpdateState, UpdateStatus};

pub(super) fn download(
    shared: &Arc<Mutex<UpdateState>>,
    ctx: &egui::Context,
    download_url: &str,
) -> anyhow::Result<Vec<u8>> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("terminal-studio-updater")
        .timeout(std::time::Duration::from_secs(300))
        .build()?;

    let mut resp = client.get(download_url).send()?;
    if !resp.status().is_success() {
        return Err(anyhow::anyhow!("HTTP {}", resp.status()));
    }

    let content_length = resp.content_length();
    let total = content_length.unwrap_or(0);
    let mut bytes = Vec::with_capacity(total as usize);
    let mut downloaded: u64 = 0;
    let mut buf = [0u8; 32768];
    loop {
        let n = std::io::Read::read(&mut resp, &mut buf)?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&buf[..n]);
        downloaded += n as u64;
        if total > 0 {
            let pct = (downloaded as f32 / total as f32) * 100.0;
            let mut s = shared.lock();
            s.status = UpdateStatus::Downloading { progress_pct: pct };
            ctx.request_repaint();
        }
    }

    if bytes.is_empty() {
        return Err(anyhow::anyhow!("Downloaded update is empty"));
    }
    if let Some(expected) = content_length {
        if bytes.len() as u64 != expected {
            return Err(anyhow::anyhow!(
                "Truncated: got {} of {} bytes",
                bytes.len(),
                expected
            ));
        }
    }

    {
        let mut s = shared.lock();
        s.status = UpdateStatus::Downloading {
            progress_pct: 100.0,
        };
    }
    ctx.request_repaint();

    Ok(bytes)
}
