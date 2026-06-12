use std::time::{Duration, Instant};

use crate::probe::{Probe, ProbeResult};

pub trait Clock {
    fn now(&self) -> Instant;
    fn sleep(&mut self, d: Duration);
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn sleep(&mut self, d: Duration) {
        std::thread::sleep(d);
    }
}

#[derive(Clone, Copy)]
pub enum IntervalPolicy {
    Fixed(Duration),
    Adaptive {
        start: Duration,
        factor: f64,
        cap: Duration,
    },
}

impl IntervalPolicy {
    pub fn initial(&self) -> Duration {
        match self {
            IntervalPolicy::Fixed(d) => *d,
            IntervalPolicy::Adaptive { start, .. } => *start,
        }
    }

    pub fn next(&self, current: Duration) -> Duration {
        match self {
            IntervalPolicy::Fixed(d) => *d,
            IntervalPolicy::Adaptive { factor, cap, .. } => current.mul_f64(*factor).min(*cap),
        }
    }
}

pub struct EngineConfig {
    pub timeout: Duration,
    pub interval: IntervalPolicy,
}

pub enum EngineOutcome {
    Met {
        report: crate::probe::Report,
        waited: Duration,
        polls: u32,
    },
    Failed {
        report: crate::probe::Report,
        waited: Duration,
        polls: u32,
    },
    Timeout {
        last_note: Option<String>,
        waited: Duration,
        polls: u32,
    },
}

pub fn run(probe: &mut dyn Probe, cfg: &EngineConfig, clock: &mut dyn Clock) -> EngineOutcome {
    let start = clock.now();
    let mut polls = 0u32;
    let mut interval = cfg.interval.initial();
    let mut last_note: Option<String> = None;

    loop {
        polls += 1;
        match probe.poll() {
            ProbeResult::Met(report) => {
                return EngineOutcome::Met {
                    report,
                    waited: clock.now() - start,
                    polls,
                };
            }
            ProbeResult::Failed(report) => {
                return EngineOutcome::Failed {
                    report,
                    waited: clock.now() - start,
                    polls,
                };
            }
            ProbeResult::Pending { note } => {
                if note.is_some() {
                    last_note = note;
                }
            }
        }

        let elapsed = clock.now() - start;
        if elapsed >= cfg.timeout {
            return EngineOutcome::Timeout {
                last_note,
                waited: elapsed,
                polls,
            };
        }
        // Clamp so the final poll lands exactly on the deadline.
        let remaining = cfg.timeout - elapsed;
        clock.sleep(interval.min(remaining));
        interval = cfg.interval.next(interval);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::Report;
    use serde_json::json;
    use std::collections::VecDeque;

    struct FakeClock {
        now: Instant,
        sleeps: Vec<Duration>,
    }

    impl FakeClock {
        fn new() -> Self {
            Self {
                now: Instant::now(),
                sleeps: Vec::new(),
            }
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> Instant {
            self.now
        }
        fn sleep(&mut self, d: Duration) {
            self.now += d;
            self.sleeps.push(d);
        }
    }

    struct ScriptedProbe {
        results: VecDeque<ProbeResult>,
    }

    impl Probe for ScriptedProbe {
        fn name(&self) -> &'static str {
            "scripted"
        }
        fn poll(&mut self) -> ProbeResult {
            self.results
                .pop_front()
                .unwrap_or(ProbeResult::Pending { note: None })
        }
    }

    fn met() -> ProbeResult {
        ProbeResult::Met(Report {
            summary: "ok".into(),
            detail: json!({}),
        })
    }

    fn pending(note: Option<&str>) -> ProbeResult {
        ProbeResult::Pending {
            note: note.map(String::from),
        }
    }

    fn cfg(timeout_s: u64, interval: IntervalPolicy) -> EngineConfig {
        EngineConfig {
            timeout: Duration::from_secs(timeout_s),
            interval,
        }
    }

    #[test]
    fn already_met_returns_immediately_without_sleeping() {
        let mut probe = ScriptedProbe {
            results: VecDeque::from([met()]),
        };
        let mut clock = FakeClock::new();
        let out = run(
            &mut probe,
            &cfg(600, IntervalPolicy::Fixed(Duration::from_secs(10))),
            &mut clock,
        );
        match out {
            EngineOutcome::Met { polls, .. } => assert_eq!(polls, 1),
            _ => panic!("expected Met"),
        }
        assert!(clock.sleeps.is_empty());
    }

    #[test]
    fn pending_then_met_sleeps_between_polls() {
        let mut probe = ScriptedProbe {
            results: VecDeque::from([pending(None), pending(None), met()]),
        };
        let mut clock = FakeClock::new();
        let out = run(
            &mut probe,
            &cfg(600, IntervalPolicy::Fixed(Duration::from_secs(10))),
            &mut clock,
        );
        match out {
            EngineOutcome::Met { polls, waited, .. } => {
                assert_eq!(polls, 3);
                assert_eq!(waited, Duration::from_secs(20));
            }
            _ => panic!("expected Met"),
        }
        assert_eq!(clock.sleeps.len(), 2);
    }

    #[test]
    fn adaptive_backoff_grows_and_caps() {
        let mut probe = ScriptedProbe {
            results: VecDeque::from([
                pending(None),
                pending(None),
                pending(None),
                pending(None),
                met(),
            ]),
        };
        let mut clock = FakeClock::new();
        let policy = IntervalPolicy::Adaptive {
            start: Duration::from_secs(2),
            factor: 2.0,
            cap: Duration::from_secs(5),
        };
        run(&mut probe, &cfg(600, policy), &mut clock);
        assert_eq!(
            clock.sleeps,
            vec![
                Duration::from_secs(2),
                Duration::from_secs(4),
                Duration::from_secs(5),
                Duration::from_secs(5),
            ]
        );
    }

    #[test]
    fn timeout_returns_last_note_and_never_oversleeps() {
        let mut probe = ScriptedProbe {
            results: VecDeque::from([pending(Some("503 from server")), pending(None)]),
        };
        let mut clock = FakeClock::new();
        let out = run(
            &mut probe,
            &cfg(15, IntervalPolicy::Fixed(Duration::from_secs(10))),
            &mut clock,
        );
        match out {
            EngineOutcome::Timeout {
                last_note,
                waited,
                polls,
            } => {
                assert_eq!(last_note.as_deref(), Some("503 from server"));
                assert_eq!(waited, Duration::from_secs(15));
                assert_eq!(polls, 3);
            }
            _ => panic!("expected Timeout"),
        }
        // Second sleep is clamped to the 5s remaining, not the full 10s interval.
        assert_eq!(
            clock.sleeps,
            vec![Duration::from_secs(10), Duration::from_secs(5)]
        );
    }

    #[test]
    fn failed_is_terminal() {
        let mut probe = ScriptedProbe {
            results: VecDeque::from([
                pending(None),
                ProbeResult::Failed(Report {
                    summary: "run concluded failure".into(),
                    detail: json!({"conclusion": "failure"}),
                }),
                met(),
            ]),
        };
        let mut clock = FakeClock::new();
        let out = run(
            &mut probe,
            &cfg(600, IntervalPolicy::Fixed(Duration::from_secs(1))),
            &mut clock,
        );
        match out {
            EngineOutcome::Failed { polls, .. } => assert_eq!(polls, 2),
            _ => panic!("expected Failed"),
        }
    }
}
