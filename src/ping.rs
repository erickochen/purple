use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::event::AppEvent;

/// Ping a single host by attempting a TCP connection on the configured port.
/// Sends the result back via the channel.
///
/// DNS resolution runs in a nested thread with a 5s timeout via `recv_timeout`.
/// If DNS hangs beyond 5s, the outer thread reports unreachable and exits,
/// but the inner thread may linger until the OS DNS resolver times out
/// (typically 30-60s). This is inherent to blocking `to_socket_addrs` with
/// no cancellation support. Repeated pings to hosts with broken DNS can
/// temporarily accumulate threads, but they will self-clean once the OS
/// resolver gives up.
pub fn ping_host(alias: String, hostname: String, port: u16, tx: mpsc::Sender<AppEvent>) {
    thread::spawn(move || {
        ping_host_inner(&alias, &hostname, port, &tx);
    });
}

/// Core ping logic shared by `ping_host` and `ping_all`.
fn ping_host_inner(alias: &str, hostname: &str, port: u16, tx: &mpsc::Sender<AppEvent>) {
    // Strip existing brackets from IPv6 addresses (e.g. "[::1]" -> "::1")
    let clean = hostname.trim_start_matches('[').trim_end_matches(']');
    let addr_str = if clean.contains(':') {
        format!("[{}]:{}", clean, port)
    } else {
        format!("{}:{}", hostname, port)
    };

    // Run DNS + TCP connect in a child thread with an overall 5s timeout
    // (to_socket_addrs has no built-in timeout and can hang on bad DNS)
    let (done_tx, done_rx) = mpsc::channel();
    let addr_str_clone = addr_str.clone();
    thread::spawn(move || {
        let result = match addr_str_clone.to_socket_addrs() {
            Ok(addrs) => addrs
                .into_iter()
                .any(|addr| TcpStream::connect_timeout(&addr, Duration::from_secs(3)).is_ok()),
            Err(_) => false,
        };
        let _ = done_tx.send(result);
    });

    let reachable = done_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap_or(false);

    let _ = tx.send(AppEvent::PingResult {
        alias: alias.to_string(),
        reachable,
    });
}

/// Ping all given hosts with a concurrency limit of 10.
/// Spawns a coordinator thread that uses a semaphore-style channel
/// to limit concurrent pings, preventing thread explosion on large host lists.
pub fn ping_all(hosts: &[(String, String, u16)], tx: mpsc::Sender<AppEvent>) {
    let hosts = hosts.to_vec();
    thread::spawn(move || {
        let max_concurrent: usize = 10;
        let (slot_tx, slot_rx) = mpsc::channel();
        for _ in 0..max_concurrent {
            let _ = slot_tx.send(());
        }
        for (alias, hostname, port) in hosts {
            let _ = slot_rx.recv(); // wait for a slot
            let slot_tx = slot_tx.clone();
            let tx = tx.clone();
            thread::spawn(move || {
                ping_host_inner(&alias, &hostname, port, &tx);
                let _ = slot_tx.send(()); // release slot
            });
        }
    });
}
