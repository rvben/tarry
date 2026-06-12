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
        let resolved = match self.addr.to_socket_addrs() {
            Ok(mut addrs) => addrs.next(),
            Err(e) => {
                return ProbeResult::Pending {
                    note: Some(format!("resolve {}: {e}", self.addr)),
                };
            }
        };
        let Some(sock_addr) = resolved else {
            return ProbeResult::Pending {
                note: Some(format!("{} resolved to no addresses", self.addr)),
            };
        };
        match TcpStream::connect_timeout(&sock_addr, self.connect_timeout) {
            Ok(_) => ProbeResult::Met(Report {
                summary: format!("{} accepts connections", self.addr),
                detail: json!({"addr": self.addr}),
            }),
            Err(e) => ProbeResult::Pending {
                note: Some(format!("connect {}: {e}", self.addr)),
            },
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
