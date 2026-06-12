use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use serde_json::json;

use crate::probe::{Probe, ProbeResult, Report};

pub struct TcpProbe {
    pub addr: String,
    pub connect_timeout: Duration,
}

impl Probe for TcpProbe {
    fn name(&self) -> &'static str {
        "tcp"
    }

    fn poll(&mut self) -> ProbeResult {
        let resolved: Vec<_> = match self.addr.to_socket_addrs() {
            Ok(addrs) => addrs.collect(),
            Err(e) => {
                return ProbeResult::Pending {
                    note: Some(format!("resolve {}: {e}", self.addr)),
                };
            }
        };
        if resolved.is_empty() {
            return ProbeResult::Pending {
                note: Some(format!("{} resolved to no addresses", self.addr)),
            };
        }
        // A host may resolve to several addresses (e.g. ::1 and 127.0.0.1);
        // any one accepting connections satisfies the condition.
        let mut last_error = String::new();
        for sock_addr in &resolved {
            match TcpStream::connect_timeout(sock_addr, self.connect_timeout) {
                Ok(_) => {
                    return ProbeResult::Met(Report {
                        summary: format!("{} accepts connections", self.addr),
                        detail: json!({"addr": self.addr, "connected": sock_addr.to_string()}),
                    });
                }
                Err(e) => last_error = format!("connect {sock_addr}: {e}"),
            }
        }
        ProbeResult::Pending {
            note: Some(last_error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn open_port_is_met() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let mut probe = TcpProbe {
            addr,
            connect_timeout: Duration::from_secs(1),
        };
        assert!(matches!(probe.poll(), ProbeResult::Met(_)));
    }

    #[test]
    fn closed_port_is_pending() {
        // Bind then drop to get a port that is very likely closed.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        drop(listener);
        let mut probe = TcpProbe {
            addr,
            connect_timeout: Duration::from_secs(1),
        };
        assert!(matches!(probe.poll(), ProbeResult::Pending { .. }));
    }

    #[test]
    fn later_resolved_address_is_tried() {
        // Bind IPv4 only, then probe via "localhost", which on dual-stack
        // systems resolves ::1 first. The probe must try every address.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let addr = format!("localhost:{port}");
        let mut probe = TcpProbe {
            addr,
            connect_timeout: Duration::from_secs(1),
        };
        assert!(matches!(probe.poll(), ProbeResult::Met(_)));
    }

    #[test]
    fn unresolvable_host_is_pending_with_note() {
        let mut probe = TcpProbe {
            addr: "host.invalid:1".into(),
            connect_timeout: Duration::from_secs(1),
        };
        match probe.poll() {
            ProbeResult::Pending { note } => assert!(note.is_some()),
            _ => panic!("expected Pending"),
        }
    }
}
