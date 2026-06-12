use serde_json::json;

use crate::probe::{Probe, ProbeResult, Report};
use crate::probes::gh::{GhClient, RunView};

pub const DIGEST_LINE_LIMIT: usize = 20;

pub struct RunProbe {
    pub gh: Box<dyn GhClient>,
    pub repo: Option<String>,
    pub branch: Option<String>,
    pub workflow: Option<String>,
    pub run_id: Option<u64>,
}

impl RunProbe {
    fn build_digest(&self, run_id: u64, view: &RunView) -> (String, bool) {
        let mut lines: Vec<String> = view
            .failed_steps
            .iter()
            .map(|(job, step)| format!("job: {job}, step: {step}"))
            .collect();
        let mut truncated = false;
        match self.gh.failed_log(run_id, self.repo.as_deref()) {
            Ok(log) => {
                let log_lines: Vec<&str> = log.lines().collect();
                if log_lines.len() > DIGEST_LINE_LIMIT {
                    truncated = true;
                }
                let start = log_lines.len().saturating_sub(DIGEST_LINE_LIMIT);
                lines.extend(log_lines[start..].iter().map(|l| l.to_string()));
            }
            Err(e) => lines.push(format!("(failed log unavailable: {e})")),
        }
        (lines.join("\n"), truncated)
    }
}

impl Probe for RunProbe {
    fn name(&self) -> &'static str {
        "run"
    }

    fn poll(&mut self) -> ProbeResult {
        let run_id = match self.run_id {
            Some(id) => id,
            None => {
                match self.gh.latest_run_id(
                    self.repo.as_deref(),
                    self.branch.as_deref(),
                    self.workflow.as_deref(),
                ) {
                    Ok(id) => {
                        self.run_id = Some(id);
                        id
                    }
                    Err(e) => {
                        return ProbeResult::Pending {
                            note: Some(e.to_string()),
                        };
                    }
                }
            }
        };

        let view = match self.gh.view_run(run_id, self.repo.as_deref()) {
            Ok(v) => v,
            Err(e) => {
                return ProbeResult::Pending {
                    note: Some(e.to_string()),
                };
            }
        };

        if view.status != "completed" {
            return ProbeResult::Pending {
                note: Some(format!("run {run_id} is {}", view.status)),
            };
        }

        let conclusion = view.conclusion.as_deref().unwrap_or("unknown");
        if conclusion == "success" {
            return ProbeResult::Met(Report {
                summary: format!("run {run_id} concluded success"),
                detail: json!({"run_id": run_id, "conclusion": "success", "url": view.url}),
            });
        }

        let (digest, truncated) = self.build_digest(run_id, &view);
        ProbeResult::Failed(Report {
            summary: format!("run {run_id} concluded {conclusion}"),
            detail: json!({
                "run_id": run_id,
                "conclusion": conclusion,
                "url": view.url,
                "digest": digest,
                "truncated": truncated,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probes::gh::GhError;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    struct FakeGh {
        latest: Result<u64, &'static str>,
        views: RefCell<VecDeque<RunView>>,
        log: String,
    }

    impl GhClient for FakeGh {
        fn latest_run_id(
            &self,
            _repo: Option<&str>,
            _branch: Option<&str>,
            _workflow: Option<&str>,
        ) -> Result<u64, GhError> {
            self.latest.map_err(|e| GhError::Failed(e.into()))
        }
        fn view_run(&self, _id: u64, _repo: Option<&str>) -> Result<RunView, GhError> {
            self.views
                .borrow_mut()
                .pop_front()
                .ok_or_else(|| GhError::Failed("no scripted view".into()))
        }
        fn failed_log(&self, _id: u64, _repo: Option<&str>) -> Result<String, GhError> {
            Ok(self.log.clone())
        }
    }

    fn view(status: &str, conclusion: Option<&str>) -> RunView {
        RunView {
            status: status.into(),
            conclusion: conclusion.map(String::from),
            url: "https://github.com/rvben/x/actions/runs/123".into(),
            failed_steps: if conclusion == Some("failure") {
                vec![("build".into(), "Run tests".into())]
            } else {
                vec![]
            },
        }
    }

    fn probe(gh: FakeGh, run_id: Option<u64>) -> RunProbe {
        RunProbe {
            gh: Box::new(gh),
            repo: None,
            branch: None,
            workflow: None,
            run_id,
        }
    }

    #[test]
    fn resolves_latest_run_id_on_first_poll() {
        let gh = FakeGh {
            latest: Ok(123),
            views: RefCell::new(VecDeque::from([view("completed", Some("success"))])),
            log: String::new(),
        };
        let mut p = probe(gh, None);
        assert!(matches!(p.poll(), ProbeResult::Met(_)));
        assert_eq!(p.run_id, Some(123));
    }

    #[test]
    fn in_progress_is_pending() {
        let gh = FakeGh {
            latest: Ok(123),
            views: RefCell::new(VecDeque::from([view("in_progress", None)])),
            log: String::new(),
        };
        let mut p = probe(gh, Some(123));
        assert!(matches!(p.poll(), ProbeResult::Pending { .. }));
    }

    #[test]
    fn success_reports_conclusion_and_url() {
        let gh = FakeGh {
            latest: Ok(123),
            views: RefCell::new(VecDeque::from([view("completed", Some("success"))])),
            log: String::new(),
        };
        let mut p = probe(gh, Some(123));
        match p.poll() {
            ProbeResult::Met(Report { summary, detail }) => {
                assert!(summary.contains("success"));
                assert_eq!(detail["run_id"], 123);
                assert_eq!(detail["conclusion"], "success");
            }
            _ => panic!("expected Met"),
        }
    }

    #[test]
    fn failure_is_terminal_with_bounded_digest() {
        let log: String = (1..=30).map(|i| format!("line {i}\n")).collect();
        let gh = FakeGh {
            latest: Ok(123),
            views: RefCell::new(VecDeque::from([view("completed", Some("failure"))])),
            log,
        };
        let mut p = probe(gh, Some(123));
        match p.poll() {
            ProbeResult::Failed(Report { detail, .. }) => {
                let digest = detail["digest"].as_str().unwrap();
                assert!(digest.contains("job: build, step: Run tests"));
                assert!(digest.contains("line 30"));
                assert!(!digest.contains("line 5\n"));
                assert_eq!(detail["truncated"], true);
            }
            _ => panic!("expected Failed"),
        }
    }

    #[test]
    fn gh_error_is_pending_not_failed() {
        let gh = FakeGh {
            latest: Ok(123),
            views: RefCell::new(VecDeque::new()),
            log: String::new(),
        };
        let mut p = probe(gh, Some(123));
        match p.poll() {
            ProbeResult::Pending { note } => assert!(note.unwrap().contains("no scripted view")),
            _ => panic!("expected Pending"),
        }
    }
}
