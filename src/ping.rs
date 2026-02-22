use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::event::AppEvent;

/// Ping a single host by attempting a TCP connection to port 22.
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
        let addr_str = format!("{}:{}", hostname, port);

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

        let _ = tx.send(AppEvent::PingResult { alias, reachable });
    });
}

/// Ping all given hosts. Each host gets its own thread.
/// For very large host lists this could spawn many threads, but SSH configs
/// rarely exceed a few dozen hosts, and each thread is short-lived (3s timeout).
pub fn ping_all(hosts: &[(String, String, u16)], tx: mpsc::Sender<AppEvent>) {
    for (alias, hostname, port) in hosts {
        ping_host(alias.clone(), hostname.clone(), *port, tx.clone());
    }
}
