use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use eyre::{WrapErr, bail};
use octos_bus::ChannelManager;
use octos_bus::cokret::CokretChannel;
use octos_bus::cokret::applet::config::CokretAppletConfig;
use octos_bus::cokret::config::CokretChannelConfig;

use crate::config::ChannelEntry;

/// Register a Cokret v1 channel. Selects account vs applet mode from the
/// `mode` setting (default `account`).
pub fn register(
    channel_mgr: &mut ChannelManager,
    entry: &ChannelEntry,
    shutdown: &Arc<AtomicBool>,
    data_dir: &Path,
) -> eyre::Result<()> {
    let id = entry
        .settings
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("cokret")
        .to_owned();
    let mode = entry
        .settings
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("account")
        .to_ascii_lowercase();

    if mode == "applet" {
        let config = CokretAppletConfig::from_settings(&id, &entry.settings)
            .ok_or_else(|| eyre::eyre!("cokret applet channel '{id}': invalid settings"))?;
        config
            .validate()
            .wrap_err_with(|| format!("cokret applet channel '{id}' config validation failed"))?;
        let bind_addr = entry
            .settings
            .get("bind_addr")
            .or_else(|| entry.settings.get("bind"))
            .or_else(|| entry.settings.get("listen_addr"))
            .and_then(|v| v.as_str())
            .unwrap_or("127.0.0.1:8330")
            .to_owned();
        let seq_path = data_dir
            .join("cokret-applet-seq")
            .join(format!("{}.seq", sanitize_id(&id)));
        let channel = CokretChannel::new_applet(
            config,
            seq_path,
            data_dir.to_path_buf(),
            bind_addr,
            shutdown.clone(),
        )
        .wrap_err_with(|| format!("cokret applet channel '{id}' failed to initialize"))?;
        channel_mgr.register(Arc::new(channel));
        return Ok(());
    }

    let config = CokretChannelConfig::from_settings(&id, &entry.settings)
        .ok_or_else(|| eyre::eyre!("cokret channel '{id}': missing baseUrl in settings"))?;
    config
        .validate()
        .wrap_err_with(|| format!("cokret channel '{id}' config validation failed"))?;
    if !config.accounts.iter().any(|a| a.listen || a.send) {
        bail!("cokret channel '{id}': no account is configured to listen or send");
    }
    let channel = CokretChannel::new_account(config, data_dir.to_path_buf(), shutdown.clone());
    channel_mgr.register(Arc::new(channel));
    Ok(())
}

/// Sanitize a channel id for use as a filename (ids are operator-defined).
fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}
