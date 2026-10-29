// Library crate root. Hosts every module so the binary entry (src/main.rs)
// becomes a thin shim and integration tests can reach the same surface.

// `too_many_lines` (threshold 400 via clippy.toml) gates production functions
// against bloat. Test builds are exempt: long test bodies and fixtures are
// normal and splitting them adds noise, not clarity.
#![cfg_attr(test, allow(clippy::too_many_lines))]

pub mod animation;
pub mod app;
pub mod askpass;
pub(crate) mod askpass_env;
pub mod asset_gen;
pub mod changelog;
pub mod cli;
pub mod cli_args;
pub mod clipboard;
pub mod connection;
pub mod containers;
pub mod demo;
pub mod demo_flag;
pub mod event;
pub mod file_browser;
pub mod fs_util;
pub mod fuzzy;
pub mod handler;
pub mod history;
pub mod import;
pub mod key_activity;
pub mod key_push;
pub mod logging;
pub mod mcp;
pub mod messages;
pub mod onboarding;
pub mod ping;
pub mod preferences;
pub mod providers;
pub mod quick_add;
pub mod runtime;
pub mod security_key;
pub mod snippet;
pub mod snippet_impact;
pub mod snippet_runs;
pub mod ssh_config;
pub mod ssh_context;
pub mod ssh_keys;
pub mod ssh_launcher;
#[cfg(target_os = "linux")]
pub(crate) mod tcp_diag;
pub mod tui;
pub mod tui_loop;
pub mod tunnel;
pub mod tunnel_live;
pub mod ui;
pub mod update;
pub mod vault_ssh;

// Re-export runtime helpers at crate root so existing `crate::set_sync_summary`
// call sites in library modules (handler, tui_loop, cli, app/hosts) keep
// resolving without path changes.
pub use runtime::helpers::*;

#[cfg(test)]
#[path = "visual_regression_tests.rs"]
mod visual_regression_tests;

pub mod i18n;
mod i18n_format;
