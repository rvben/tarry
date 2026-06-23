use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::json;

use crate::probe::{Probe, ProbeResult, Report};
use crate::probes::gh::{GhClient, RunView, parse_rfc3339_to_epoch};

pub const DIGEST_LINE_LIMIT: usize = 20;

/// How far before the wait began a *completed* run may have been created and
/// still be treated as "the run we are waiting on" rather than a stale prior
/// run. Covers clock skew and a short gap between triggering a workflow and
/// invoking `tarry`; comfortably below the age of any previous run.
pub const STALE_GRACE: Duration = Duration::from_secs(120);

pub struct RunProbe {
    pub gh: Box<dyn GhClient>,
    pub repo: Option<String>,
    pub branch: Option<String>,
    pub workflow: Option<String>,
    pub run_id: Option<u64>,
    /// Wall-clock instant the wait began. A completed run created before this
    /// (minus [`STALE_GRACE`]) is a stale prior run and is skipped while
    /// resolving the latest run, so a just-triggered run that has not yet
    /// registered does not cause `tarry` to report the previous run's verdict.
    pub started_at: SystemTime,
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

    /// True when a *completed* run created at `created_at` predates this wait
    /// (allowing for [`STALE_GRACE`]), i.e. it finished before we started
    /// waiting and is therefore a prior run, not the one we want. An
    /// unparseable timestamp is treated as not-stale so a format change can
    /// never make `tarry` hang.
    fn is_stale_completed(&self, created_at: &str) -> bool {
        let Some(created) = parse_rfc3339_to_epoch(created_at) else {
            return false;
        };
        let started = self
            .started_at
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        created < started - STALE_GRACE.as_secs() as i64
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
                let summary = match self.gh.latest_run(
                    self.repo.as_deref(),
                    self.branch.as_deref(),
                    self.workflow.as_deref(),
                ) {
                    Ok(s) => s,
                    Err(e) => {
                        return ProbeResult::Pending {
                            note: Some(e.to_string()),
                        };
                    }
                };

                // A run that has already completed and was created before this
                // wait began is a stale prior run (e.g. the previous release).
                // Skip it without latching on, so a just-triggered run that has
                // not yet registered does not make us report the old verdict.
                if summary.status == "completed" && self.is_stale_completed(&summary.created_at) {
                    return ProbeResult::Pending {
                        note: Some(format!(
                            "latest run {} predates this wait; awaiting a newer run",
                            summary.id
                        )),
                    };
                }

                self.run_id = Some(summary.id);

                // A live run is clearly current: report its status without a
                // second API call this poll.
                if summary.status != "completed" {
                    return ProbeResult::Pending {
                        note: Some(format!("run {} is {}", summary.id, summary.status)),
                    };
                }
                summary.id
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
    use crate::probes::gh::{GhError, RunSummary};
    use std::cell::RefCell;
    use std::collections::VecDeque;

    struct FakeGh {
        summaries: RefCell<VecDeque<Result<RunSummary, &'static str>>>,
        views: RefCell<VecDeque<RunView>>,
        log: String,
    }

    impl FakeGh {
        fn with_views(views: VecDeque<RunView>) -> Self {
            FakeGh {
                summaries: RefCell::new(VecDeque::new()),
                views: RefCell::new(views),
                log: String::new(),
            }
        }
    }

    impl GhClient for FakeGh {
        fn latest_run(
            &self,
            _repo: Option<&str>,
            _branch: Option<&str>,
            _workflow: Option<&str>,
        ) -> Result<RunSummary, GhError> {
            self.summaries
                .borrow_mut()
                .pop_front()
                .unwrap_or(Err("no scripted summary"))
                .map_err(|e| GhError::Failed(e.into()))
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

    fn summary(id: u64, status: &str, created_at: &str) -> RunSummary {
        RunSummary {
            id,
            status: status.into(),
            created_at: created_at.into(),
        }
    }

    /// Fixed wait-start instant so `created_at` comparisons are deterministic.
    fn started() -> SystemTime {
        UNIX_EPOCH
            + Duration::from_secs(parse_rfc3339_to_epoch("2026-06-23T12:00:00Z").unwrap() as u64)
    }

    fn probe(gh: FakeGh, run_id: Option<u64>) -> RunProbe {
        RunProbe {
            gh: Box::new(gh),
            repo: None,
            branch: None,
            workflow: None,
            run_id,
            started_at: started(),
        }
    }

    #[test]
    fn resolves_latest_run_on_first_poll() {
        // A completed run created at the wait start is current, not stale.
        let mut gh = FakeGh::with_views(VecDeque::from([view("completed", Some("success"))]));
        gh.summaries = RefCell::new(VecDeque::from([Ok(summary(
            123,
            "completed",
            "2026-06-23T12:00:00Z",
        ))]));
        let mut p = probe(gh, None);
        assert!(matches!(p.poll(), ProbeResult::Met(_)));
        assert_eq!(p.run_id, Some(123));
    }

    #[test]
    fn stale_completed_run_is_skipped_then_new_run_resolves() {
        // Regression: right after triggering a workflow, `gh run list` still
        // returns the previous (completed) run because the new one has not
        // registered yet. tarry must NOT report that stale run's verdict; it
        // must keep polling and latch onto the new run once it appears.
        let gh = FakeGh {
            summaries: RefCell::new(VecDeque::from([
                Ok(summary(100, "completed", "2026-06-23T11:00:00Z")), // an hour before the wait
                Ok(summary(999, "in_progress", "2026-06-23T12:00:30Z")), // the just-triggered run
            ])),
            views: RefCell::new(VecDeque::new()),
            log: String::new(),
        };
        let mut p = probe(gh, None);

        // Poll 1: the stale completed run is skipped, not latched, not reported.
        match p.poll() {
            ProbeResult::Pending { note } => {
                assert!(note.unwrap().contains("predates this wait"));
            }
            _ => panic!("expected Pending on the stale run"),
        }
        assert_eq!(p.run_id, None, "must not latch onto the stale run");

        // Poll 2: the new run has registered (in progress) and is latched.
        assert!(matches!(p.poll(), ProbeResult::Pending { .. }));
        assert_eq!(p.run_id, Some(999));
    }

    #[test]
    fn recently_completed_run_is_accepted() {
        // A run that finished moments before the wait began (within the grace
        // window) is the run we want, not a stale prior run.
        let gh = FakeGh {
            summaries: RefCell::new(VecDeque::from([Ok(summary(
                500,
                "completed",
                "2026-06-23T11:59:30Z", // 30s before start, inside STALE_GRACE
            ))])),
            views: RefCell::new(VecDeque::from([view("completed", Some("success"))])),
            log: String::new(),
        };
        let mut p = probe(gh, None);
        assert!(matches!(p.poll(), ProbeResult::Met(_)));
        assert_eq!(p.run_id, Some(500));
    }

    #[test]
    fn unparseable_created_at_is_not_treated_stale() {
        // Safety valve: if the timestamp cannot be parsed we fall back to the
        // old behaviour (latch on the latest run) rather than hang forever.
        let gh = FakeGh {
            summaries: RefCell::new(VecDeque::from([Ok(summary(7, "completed", ""))])),
            views: RefCell::new(VecDeque::from([view("completed", Some("success"))])),
            log: String::new(),
        };
        let mut p = probe(gh, None);
        assert!(matches!(p.poll(), ProbeResult::Met(_)));
        assert_eq!(p.run_id, Some(7));
    }

    #[test]
    fn in_progress_is_pending() {
        let gh = FakeGh::with_views(VecDeque::from([view("in_progress", None)]));
        let mut p = probe(gh, Some(123));
        assert!(matches!(p.poll(), ProbeResult::Pending { .. }));
    }

    #[test]
    fn success_reports_conclusion_and_url() {
        let gh = FakeGh::with_views(VecDeque::from([view("completed", Some("success"))]));
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
        let mut gh = FakeGh::with_views(VecDeque::from([view("completed", Some("failure"))]));
        gh.log = log;
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
        let gh = FakeGh::with_views(VecDeque::new());
        let mut p = probe(gh, Some(123));
        match p.poll() {
            ProbeResult::Pending { note } => assert!(note.unwrap().contains("no scripted view")),
            _ => panic!("expected Pending"),
        }
    }
}
