use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;

use log::{error, info, warn};

use crate::event::AppEvent;

pub fn spawn_provider_sync(
    section: &crate::providers::config::ProviderSection,
    tx: mpsc::Sender<AppEvent>,
    cancel: Arc<AtomicBool>,
    env: Arc<crate::runtime::env::Env>,
) {
    // Use full ProviderConfigId (provider[:label]) as the event/sync key so
    // two labeled configs of the same provider don't collide on the syncing
    // HashMap or in event payloads.
    let name = section.id.to_string();
    let token = section.token.clone();
    let section_clone = section.clone();
    let tx_fallback = tx.clone();
    let name_fallback = name.clone();
    log::debug!("[purple] Spawning provider sync thread: {name}");
    if let Err(e) = std::thread::Builder::new()
        .name(format!("sync-{}", name))
        .spawn(move || {
            let provider = match crate::providers::get_provider_with_config(&section_clone) {
                Some(p) => p,
                None => {
                    warn!("[config] Unknown provider requested for sync: {name}");
                    let _ = tx.send(AppEvent::SyncError {
                        provider: name,
                        message: crate::i18n::text(crate::messages::SYNC_UNKNOWN_PROVIDER).to_string(),
                    });
                    return;
                }
            };
            info!("[external] Provider sync started: {name}");
            let progress_tx = tx.clone();
            let progress_name = name.clone();
            let progress = move |msg: &str| {
                let _ = progress_tx.send(AppEvent::SyncProgress {
                    provider: progress_name.clone(),
                    message: msg.to_string(),
                });
            };
            match provider.fetch_hosts_with_progress(&token, &cancel, &env, &progress) {
                Ok(hosts) => {
                    if hosts.is_empty() {
                        warn!("[config] Provider sync returned 0 hosts: {name} (check API token permissions)");
                    } else {
                        info!("[external] Provider sync completed: {name}, {} hosts found", hosts.len());
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
    {
        error!(
            "[purple] Failed to spawn sync thread for {}: {}",
            name_fallback, e
        );
        let _ = tx_fallback.send(AppEvent::SyncError {
            provider: name_fallback,
            message: crate::i18n::text(crate::messages::SYNC_THREAD_SPAWN_FAILED).to_string(),
        });
    }
}
