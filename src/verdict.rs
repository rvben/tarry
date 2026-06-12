use std::time::Duration;

use serde_json::{Value, json};

use crate::engine::EngineOutcome;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerdictKind {
    Met,
    Timeout,
    ConditionFailed,
}

impl VerdictKind {
    pub fn as_str(self) -> &'static str {
        match self {
            VerdictKind::Met => "met",
            VerdictKind::Timeout => "timeout",
            VerdictKind::ConditionFailed => "condition_failed",
        }
    }
}

pub struct Verdict {
    pub condition: &'static str,
    pub kind: VerdictKind,
    pub summary: String,
    pub waited: Duration,
    pub polls: u32,
    pub detail: Value,
}

impl Verdict {
    pub fn from_outcome(condition: &'static str, outcome: EngineOutcome) -> Self {
        match outcome {
            EngineOutcome::Met {
                report,
                waited,
                polls,
            } => Verdict {
                condition,
                kind: VerdictKind::Met,
                summary: report.summary,
                waited,
                polls,
                detail: report.detail,
            },
            EngineOutcome::Failed {
                report,
                waited,
                polls,
            } => Verdict {
                condition,
                kind: VerdictKind::ConditionFailed,
                summary: report.summary,
                waited,
                polls,
                detail: report.detail,
            },
            EngineOutcome::Timeout {
                last_note,
                waited,
                polls,
            } => {
                let summary = match &last_note {
                    Some(note) => format!("timed out (last: {note})"),
                    None => "timed out".to_string(),
                };
                Verdict {
                    condition,
                    kind: VerdictKind::Timeout,
                    summary,
                    waited,
                    polls,
                    detail: json!({ "last_note": last_note }),
                }
            }
        }
    }

    pub fn exit_code(&self) -> i32 {
        match self.kind {
            VerdictKind::Met => 0,
            VerdictKind::Timeout => 1,
            VerdictKind::ConditionFailed => 2,
        }
    }

    pub fn render_json(&self) -> String {
        json!({
            "condition": self.condition,
            "ok": self.kind == VerdictKind::Met,
            "kind": self.kind.as_str(),
            "waited_s": self.waited.as_secs(),
            "polls": self.polls,
            "detail": self.detail,
        })
        .to_string()
    }

    pub fn render_text(&self) -> String {
        let mark = if self.kind == VerdictKind::Met {
            "ok"
        } else {
            self.kind.as_str()
        };
        let waited = humantime::format_duration(Duration::from_secs(self.waited.as_secs()));
        let mut out = format!(
            "{mark}: {} (waited {waited}, {} polls)",
            self.summary, self.polls
        );
        if let Some(digest) = self.detail.get("digest").and_then(Value::as_str) {
            out.push('\n');
            out.push_str(digest);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::Report;

    fn met_verdict() -> Verdict {
        Verdict::from_outcome(
            "http",
            EngineOutcome::Met {
                report: Report {
                    summary: "200 OK".into(),
                    detail: json!({"status": 200}),
                },
                waited: Duration::from_secs(252),
                polls: 26,
            },
        )
    }

    #[test]
    fn met_maps_to_exit_0_and_ok_true() {
        let v = met_verdict();
        assert_eq!(v.exit_code(), 0);
        let parsed: Value = serde_json::from_str(&v.render_json()).unwrap();
        assert_eq!(parsed["condition"], "http");
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["kind"], "met");
        assert_eq!(parsed["waited_s"], 252);
        assert_eq!(parsed["polls"], 26);
        assert_eq!(parsed["detail"]["status"], 200);
    }

    #[test]
    fn timeout_maps_to_exit_1_with_note_in_detail() {
        let v = Verdict::from_outcome(
            "tcp",
            EngineOutcome::Timeout {
                last_note: Some("connection refused".into()),
                waited: Duration::from_secs(600),
                polls: 40,
            },
        );
        assert_eq!(v.exit_code(), 1);
        let parsed: Value = serde_json::from_str(&v.render_json()).unwrap();
        assert_eq!(parsed["ok"], false);
        assert_eq!(parsed["kind"], "timeout");
        assert_eq!(parsed["detail"]["last_note"], "connection refused");
    }

    #[test]
    fn failed_maps_to_exit_2() {
        let v = Verdict::from_outcome(
            "run",
            EngineOutcome::Failed {
                report: Report {
                    summary: "run 123 concluded failure".into(),
                    detail: json!({"conclusion": "failure"}),
                },
                waited: Duration::from_secs(60),
                polls: 7,
            },
        );
        assert_eq!(v.exit_code(), 2);
        let parsed: Value = serde_json::from_str(&v.render_json()).unwrap();
        assert_eq!(parsed["kind"], "condition_failed");
    }

    #[test]
    fn text_rendering_is_one_line_for_met() {
        let v = met_verdict();
        let text = v.render_text();
        assert_eq!(text.lines().count(), 1);
        assert!(text.contains("200 OK"));
        assert!(text.contains("26 polls"));
    }

    #[test]
    fn text_rendering_appends_digest_lines_when_present() {
        let v = Verdict::from_outcome(
            "run",
            EngineOutcome::Failed {
                report: Report {
                    summary: "run 123 concluded failure".into(),
                    detail: json!({"digest": "job: build, step: test\nline one\nline two"}),
                },
                waited: Duration::from_secs(60),
                polls: 7,
            },
        );
        let text = v.render_text();
        assert!(text.lines().count() > 1);
        assert!(text.contains("line two"));
    }
}
