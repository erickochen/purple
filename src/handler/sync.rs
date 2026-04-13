use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;

use log::{error, info, warn};

use crate::event::AppEvent;

pub fn spawn_provider_sync(
    section: &crate::providers::config::ProviderSection,
    tx: mpsc::Sender<AppEvent>,
    cancel: Arc<AtomicBool>,
) {
    let name = section.provider.clone();
    let token = section.token.clone();
    let section_clone = section.clone();
    let tx_fallback = tx.clone();
    let name_fallback = name.clone();
    if std::thread::Builder::new()
        .name(format!("sync-{}", name))
        .spawn(move || {
            let provider = match crate::providers::get_provider_with_config(&name, &section_clone) {
                Some(p) => p,
                None => {
                    let _ = tx.send(AppEvent::SyncError {
                        provider: name,
                        message: "Unknown provider.".to_string(),
                    });
                    return;
                }
            };
            info!("Provider sync started: {name}");
            let progress_tx = tx.clone();
            let progress_name = name.clone();
            let progress = move |msg: &str| {
                let _ = progress_tx.send(AppEvent::SyncProgress {
                    provider: progress_name.clone(),
                    message: msg.to_string(),
                });
            };
            match provider.fetch_hosts_with_progress(&token, &cancel, &progress) {
                Ok(hosts) => {
                    if hosts.is_empty() {
                        warn!("[config] Provider sync returned 0 hosts: {name} (check API token permissions)");
                    } else {
                        info!("Provider sync completed: {name}, {} hosts found", hosts.len());
                    }
                    let _ = tx.send(AppEvent::SyncComplete {
                        provider: name,
                        hosts,
                    });
                }
                Err(crate::providers::ProviderError::PartialResult {
                    hosts,
                    failures,
                    total,
                }) => {
                    warn!("[external] Provider sync partial: {name}, {} hosts, {} failures", hosts.len(), failures);
                    let _ = tx.send(AppEvent::SyncPartial {
                        provider: name,
                        hosts,
                        failures,
                        total,
                    });
                }
                Err(e) => {
                    error!("[external] Provider sync failed: {name}: {e}");
                    let _ = tx.send(AppEvent::SyncError {
                        provider: name,
                        message: e.to_string(),
                    });
                }
            }
        })
        .is_err()
    {
        let _ = tx_fallback.send(AppEvent::SyncError {
            provider: name_fallback,
            message: "Failed to start sync thread.".to_string(),
        });
    }
}
