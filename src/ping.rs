use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::event::AppEvent;

/// Ping a single host by attempting a TCP connection to port 22.
/// Sends the result back via the channel.
pub fn ping_host(alias: String, hostname: String, port: u16, tx: mpsc::Sender<AppEvent>) {
    thread::spawn(move || {
        let addr_str = format!("{}:{}", hostname, port);
        let reachable = match addr_str.to_socket_addrs() {
            Ok(mut addrs) => {
                if let Some(addr) = addrs.next() {
                    TcpStream::connect_timeout(&addr, Duration::from_secs(3)).is_ok()
                } else {
                    false
                }
            }
            Err(_) => false,
        };
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
