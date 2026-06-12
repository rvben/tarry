use std::path::PathBuf;

use regex::Regex;
use serde_json::json;

use crate::probe::{Probe, ProbeResult, Report};

pub enum FileMatcher {
    None,
    Contains(String),
    Regex(Regex),
}

pub struct FileProbe {
    pub path: PathBuf,
    pub matcher: FileMatcher,
}

impl Probe for FileProbe {
    fn name(&self) -> &'static str {
        "file"
    }

    fn poll(&mut self) -> ProbeResult {
        // Bare existence needs no read: a non-UTF-8 file still satisfies it.
        if matches!(self.matcher, FileMatcher::None) {
            return match std::fs::metadata(&self.path) {
                Ok(meta) => ProbeResult::Met(Report {
                    summary: format!("{} exists", self.path.display()),
                    detail: json!({"path": self.path.display().to_string(), "bytes": meta.len()}),
                }),
                Err(_) => ProbeResult::Pending {
                    note: Some(format!("{} does not exist", self.path.display())),
                },
            };
        }
        let bytes = match std::fs::read(&self.path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return ProbeResult::Pending {
                    note: Some(format!("{} does not exist", self.path.display())),
                };
            }
            Err(e) => {
                return ProbeResult::Pending {
                    note: Some(format!("read {}: {e}", self.path.display())),
                };
            }
        };
        let content = String::from_utf8_lossy(&bytes);
        let matched = match &self.matcher {
            FileMatcher::None => unreachable!("handled above"),
            FileMatcher::Contains(s) => content.contains(s.as_str()),
            FileMatcher::Regex(re) => re.is_match(&content),
        };
        if matched {
            ProbeResult::Met(Report {
                summary: format!("{} matched", self.path.display()),
                detail: json!({"path": self.path.display().to_string(), "bytes": bytes.len()}),
            })
        } else {
            ProbeResult::Pending {
                note: Some("file exists but does not match yet".into()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn missing_file_is_pending() {
        let dir = tempfile::tempdir().unwrap();
        let mut probe = FileProbe {
            path: dir.path().join("absent.txt"),
            matcher: FileMatcher::None,
        };
        assert!(matches!(probe.poll(), ProbeResult::Pending { .. }));
    }

    #[test]
    fn existing_file_without_matcher_is_met() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.txt");
        fs::write(&path, "hello").unwrap();
        let mut probe = FileProbe {
            path,
            matcher: FileMatcher::None,
        };
        assert!(matches!(probe.poll(), ProbeResult::Met(_)));
    }

    #[test]
    fn contains_matcher_pending_until_content_appears() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("log.txt");
        fs::write(&path, "starting").unwrap();
        let mut probe = FileProbe {
            path: path.clone(),
            matcher: FileMatcher::Contains("done".into()),
        };
        assert!(matches!(probe.poll(), ProbeResult::Pending { .. }));
        fs::write(&path, "starting\ndone").unwrap();
        assert!(matches!(probe.poll(), ProbeResult::Met(_)));
    }

    #[test]
    fn non_utf8_file_without_matcher_is_met() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("binary.dat");
        fs::write(&path, [0xff, 0xfe, 0x00, 0x80]).unwrap();
        let mut probe = FileProbe {
            path,
            matcher: FileMatcher::None,
        };
        assert!(matches!(probe.poll(), ProbeResult::Met(_)));
    }

    #[test]
    fn non_utf8_file_with_matcher_still_matches_lossily() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mixed.log");
        let mut bytes = vec![0xff, 0xfe];
        bytes.extend_from_slice(b"status: done\n");
        fs::write(&path, bytes).unwrap();
        let mut probe = FileProbe {
            path,
            matcher: FileMatcher::Contains("status: done".into()),
        };
        assert!(matches!(probe.poll(), ProbeResult::Met(_)));
    }

    #[test]
    fn regex_matcher_matches() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("log.txt");
        fs::write(&path, "verdict: APPROVED").unwrap();
        let mut probe = FileProbe {
            path,
            matcher: FileMatcher::Regex(Regex::new(r"verdict: (APPROVED|REJECTED)").unwrap()),
        };
        assert!(matches!(probe.poll(), ProbeResult::Met(_)));
    }
}
