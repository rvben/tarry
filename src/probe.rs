use serde_json::Value;

/// One observation of the condition.
pub enum ProbeResult {
    /// Condition not met yet. `note` carries the most recent transient
    /// error or status, surfaced in the verdict if the timeout expires.
    Pending {
        note: Option<String>,
    },
    Met(Report),
    Failed(Report),
}

/// Condition-specific result data for the final verdict.
pub struct Report {
    /// One-line human summary, e.g. "run 123 concluded success".
    pub summary: String,
    /// Structured detail for JSON output.
    pub detail: Value,
}

pub trait Probe {
    /// Condition name as it appears in the verdict, e.g. "http".
    fn name(&self) -> &'static str;
    fn poll(&mut self) -> ProbeResult;
}
