use std::process::Command;

use regex::Regex;
use serde_json::json;

use crate::probe::{Probe, ProbeResult, Report};

pub struct CmdProbe {
    pub argv: Vec<String>,
    pub ok_output: Option<Regex>,
}

impl Probe for CmdProbe {
    fn name(&self) -> &'static str {
        "cmd"
    }

    fn poll(&mut self) -> ProbeResult {
        let output = match Command::new(&self.argv[0]).args(&self.argv[1..]).output() {
            Ok(o) => o,
            Err(e) => {
                return ProbeResult::Pending {
                    note: Some(format!("spawn {}: {e}", self.argv[0])),
                };
            }
        };
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let met = match &self.ok_output {
            Some(re) => re.is_match(&combined),
            None => output.status.success(),
        };
        let exit = output.status.code();
        if met {
            ProbeResult::Met(Report {
                summary: format!("command succeeded (exit {})", exit.unwrap_or(-1)),
                detail: json!({"argv": self.argv, "exit": exit}),
            })
        } else {
            ProbeResult::Pending {
                note: Some(format!("exit {}", exit.unwrap_or(-1))),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(argv: &[&str]) -> CmdProbe {
        CmdProbe {
            argv: argv.iter().map(|s| s.to_string()).collect(),
            ok_output: None,
        }
    }

    #[test]
    fn exit_zero_is_met() {
        let mut p = probe(&["true"]);
        assert!(matches!(p.poll(), ProbeResult::Met(_)));
    }

    #[test]
    fn nonzero_exit_is_pending() {
        let mut p = probe(&["false"]);
        assert!(matches!(p.poll(), ProbeResult::Pending { .. }));
    }

    #[test]
    fn ok_output_overrides_exit_code() {
        // Command exits 1 but output matches.
        let mut p = probe(&["sh", "-c", "echo state=ready; exit 1"]);
        p.ok_output = Some(Regex::new("state=ready").unwrap());
        assert!(matches!(p.poll(), ProbeResult::Met(_)));
    }

    #[test]
    fn ok_output_not_matching_is_pending_even_on_exit_zero() {
        let mut p = probe(&["sh", "-c", "echo state=starting; exit 0"]);
        p.ok_output = Some(Regex::new("state=ready").unwrap());
        assert!(matches!(p.poll(), ProbeResult::Pending { .. }));
    }

    #[test]
    fn missing_binary_is_pending_with_note() {
        let mut p = probe(&["tarry-test-no-such-binary-zz"]);
        match p.poll() {
            ProbeResult::Pending { note } => assert!(note.is_some()),
            _ => panic!("expected Pending"),
        }
    }
}
