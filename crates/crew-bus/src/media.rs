//! Shared media download helper for channels.

use std::path::{Path, PathBuf};

use eyre::{Result, WrapErr};
use reqwest::Client;
use tracing::debug;

/// Download a file from a URL to the media directory.
/// Returns the absolute path of the saved file.
pub async fn download_media(
    client: &Client,
    url: &str,
    headers: &[(&str, &str)],
    dest_dir: &Path,
    filename: &str,
) -> Result<PathBuf> {
    std::fs::create_dir_all(dest_dir)
        .wrap_err_with(|| format!("failed to create media dir: {}", dest_dir.display()))?;

    let dest = dest_dir.join(filename);

    let mut req = client.get(url);
    for &(key, value) in headers {
        req = req.header(key, value);
    }

    let response = req
        .send()
        .await
        .wrap_err_with(|| format!("failed to download: {url}"))?;

    if !response.status().is_success() {
        eyre::bail!("download failed (HTTP {}): {url}", response.status());
    }

    let bytes = response
        .bytes()
        .await
        .wrap_err("failed to read download body")?;
    std::fs::write(&dest, &bytes)
        .wrap_err_with(|| format!("failed to write: {}", dest.display()))?;

    debug!(path = %dest.display(), bytes = bytes.len(), "media downloaded");
    Ok(dest)
}

/// Check if a file path looks like an audio file.
pub fn is_audio(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".ogg")
        || lower.ends_with(".mp3")
        || lower.ends_with(".m4a")
        || lower.ends_with(".wav")
        || lower.ends_with(".oga")
        || lower.ends_with(".opus")
}

/// Check if a file path looks like an image file.
pub fn is_image(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".png")
        || lower.ends_with(".gif")
        || lower.ends_with(".webp")
}
