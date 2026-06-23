use std::process::Command;

#[derive(Debug)]
pub enum GhError {
    NotInstalled,
    Failed(String),
}

impl std::fmt::Display for GhError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GhError::NotInstalled => write!(f, "gh is not installed or not on PATH"),
            GhError::Failed(msg) => write!(f, "gh: {msg}"),
        }
    }
}

pub struct RunView {
    pub status: String,
    pub conclusion: Option<String>,
    pub url: String,
    /// (job name, failed step name) pairs for jobs that concluded failure.
    pub failed_steps: Vec<(String, String)>,
}

/// Lightweight identity of the most recent run, used to decide whether it is
/// the run we should wait on or a stale prior one.
pub struct RunSummary {
    pub id: u64,
    pub status: String,
    /// RFC3339 creation timestamp as reported by `gh` (e.g. `2026-06-23T12:34:56Z`).
    pub created_at: String,
}

pub trait GhClient {
    fn latest_run(
        &self,
        repo: Option<&str>,
        branch: Option<&str>,
        workflow: Option<&str>,
    ) -> Result<RunSummary, GhError>;
    fn view_run(&self, id: u64, repo: Option<&str>) -> Result<RunView, GhError>;
    fn failed_log(&self, id: u64, repo: Option<&str>) -> Result<String, GhError>;
}

pub struct GhCli;

impl GhCli {
    /// Preflight: confirm gh exists before entering the poll loop.
    pub fn check() -> Result<(), GhError> {
        run_gh(&["--version"]).map(|_| ())
    }
}

fn run_gh(args: &[&str]) -> Result<String, GhError> {
    let output = Command::new("gh").args(args).output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            GhError::NotInstalled
        } else {
            GhError::Failed(e.to_string())
        }
    })?;
    if !output.status.success() {
        return Err(GhError::Failed(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn with_repo<'a>(mut args: Vec<&'a str>, repo: Option<&'a str>) -> Vec<&'a str> {
    if let Some(repo) = repo {
        args.push("-R");
        args.push(repo);
    }
    args
}

impl GhClient for GhCli {
    fn latest_run(
        &self,
        repo: Option<&str>,
        branch: Option<&str>,
        workflow: Option<&str>,
    ) -> Result<RunSummary, GhError> {
        let mut args = vec![
            "run",
            "list",
            "--limit",
            "1",
            "--json",
            "databaseId,status,createdAt",
        ];
        if let Some(branch) = branch {
            args.push("--branch");
            args.push(branch);
        }
        if let Some(workflow) = workflow {
            args.push("--workflow");
            args.push(workflow);
        }
        let args = with_repo(args, repo);
        let stdout = run_gh(&args)?;
        let runs: serde_json::Value =
            serde_json::from_str(&stdout).map_err(|e| GhError::Failed(e.to_string()))?;
        let run = runs
            .get(0)
            .ok_or_else(|| GhError::Failed("no runs found".into()))?;
        let id = run["databaseId"]
            .as_u64()
            .ok_or_else(|| GhError::Failed("run has no databaseId".into()))?;
        Ok(RunSummary {
            id,
            status: run["status"].as_str().unwrap_or("").to_string(),
            created_at: run["createdAt"].as_str().unwrap_or("").to_string(),
        })
    }

    fn view_run(&self, id: u64, repo: Option<&str>) -> Result<RunView, GhError> {
        let id_string = id.to_string();
        let args = with_repo(
            vec![
                "run",
                "view",
                &id_string,
                "--json",
                "status,conclusion,url,jobs",
            ],
            repo,
        );
        let stdout = run_gh(&args)?;
        let v: serde_json::Value =
            serde_json::from_str(&stdout).map_err(|e| GhError::Failed(e.to_string()))?;
        Ok(parse_run_view(&v))
    }

    fn failed_log(&self, id: u64, repo: Option<&str>) -> Result<String, GhError> {
        let id_string = id.to_string();
        let args = with_repo(vec!["run", "view", &id_string, "--log-failed"], repo);
        run_gh(&args)
    }
}

/// Current git branch of the working directory, if inside a repo and not
/// detached. Used so a bare `tarry run` waits on the current branch's runs.
pub fn current_git_branch() -> Option<String> {
    current_git_branch_in(std::path::Path::new("."))
}

fn current_git_branch_in(dir: &std::path::Path) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!branch.is_empty() && branch != "HEAD").then_some(branch)
}

/// Parse an RFC3339 UTC timestamp (`YYYY-MM-DDTHH:MM:SS...`) into seconds since
/// the Unix epoch. Any fractional seconds and timezone suffix are ignored;
/// `gh` reports run timestamps in UTC. Returns `None` on a malformed prefix.
///
/// Uses the civil-from-date algorithm (no leap seconds, proleptic Gregorian),
/// which avoids pulling in a date/time dependency for a single comparison.
pub fn parse_rfc3339_to_epoch(s: &str) -> Option<i64> {
    let bytes = s.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    // Validate the fixed-width separators so we never misread a field.
    if bytes[4] != b'-'
        || bytes[7] != b'-'
        || (bytes[10] != b'T' && bytes[10] != b' ')
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    let field = |a: usize, b: usize| -> Option<i64> { s.get(a..b)?.parse().ok() };
    let (year, month, day) = (field(0, 4)?, field(5, 7)?, field(8, 10)?);
    let (hour, min, sec) = (field(11, 13)?, field(14, 16)?, field(17, 19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || min > 59 || sec > 60 {
        return None;
    }
    // days_from_civil (Howard Hinnant): days since 1970-01-01.
    let y = if month <= 2 { year - 1 } else { year };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + hour * 3600 + min * 60 + sec)
}

pub fn parse_run_view(v: &serde_json::Value) -> RunView {
    let mut failed_steps = Vec::new();
    if let Some(jobs) = v["jobs"].as_array() {
        for job in jobs {
            if job["conclusion"].as_str() == Some("failure") {
                let job_name = job["name"].as_str().unwrap_or("?").to_string();
                let step_name = job["steps"]
                    .as_array()
                    .and_then(|steps| {
                        steps
                            .iter()
                            .find(|s| s["conclusion"].as_str() == Some("failure"))
                    })
                    .and_then(|s| s["name"].as_str())
                    .unwrap_or("?")
                    .to_string();
                failed_steps.push((job_name, step_name));
            }
        }
    }
    RunView {
        status: v["status"].as_str().unwrap_or("").to_string(),
        conclusion: v["conclusion"]
            .as_str()
            .filter(|c| !c.is_empty())
            .map(String::from),
        url: v["url"].as_str().unwrap_or("").to_string(),
        failed_steps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_run_view_extracts_failed_job_and_step() {
        let v = json!({
            "status": "completed",
            "conclusion": "failure",
            "url": "https://github.com/rvben/x/actions/runs/123",
            "jobs": [
                {
                    "name": "build",
                    "conclusion": "failure",
                    "steps": [
                        {"name": "Checkout", "conclusion": "success"},
                        {"name": "Run tests", "conclusion": "failure"}
                    ]
                },
                {"name": "lint", "conclusion": "success", "steps": []}
            ]
        });
        let view = parse_run_view(&v);
        assert_eq!(view.status, "completed");
        assert_eq!(view.conclusion.as_deref(), Some("failure"));
        assert_eq!(
            view.failed_steps,
            vec![("build".to_string(), "Run tests".to_string())]
        );
    }

    fn git(dir: &std::path::Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?} failed");
    }

    #[test]
    fn current_git_branch_returns_branch_on_a_branch() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-b", "feature-x"]);
        git(dir.path(), &["config", "user.email", "t@example.com"]);
        git(dir.path(), &["config", "user.name", "t"]);
        git(dir.path(), &["config", "commit.gpgsign", "false"]);
        git(dir.path(), &["commit", "--allow-empty", "-m", "init"]);
        assert_eq!(
            current_git_branch_in(dir.path()).as_deref(),
            Some("feature-x")
        );
    }

    #[test]
    fn current_git_branch_is_none_when_detached() {
        // Release workflows check out tags, which detaches HEAD; branch
        // inference must yield None there rather than the literal "HEAD".
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-b", "main"]);
        git(dir.path(), &["config", "user.email", "t@example.com"]);
        git(dir.path(), &["config", "user.name", "t"]);
        git(dir.path(), &["config", "commit.gpgsign", "false"]);
        git(dir.path(), &["commit", "--allow-empty", "-m", "init"]);
        git(dir.path(), &["checkout", "--detach"]);
        assert_eq!(current_git_branch_in(dir.path()), None);
    }

    #[test]
    fn parse_rfc3339_known_epochs() {
        assert_eq!(parse_rfc3339_to_epoch("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_rfc3339_to_epoch("2024-01-01T00:00:00Z"),
            Some(1_704_067_200)
        );
        // Time-of-day and a space separator both parse; fractional seconds ignored.
        assert_eq!(
            parse_rfc3339_to_epoch("2024-01-01T01:02:03Z"),
            Some(1_704_067_200 + 3723)
        );
        assert_eq!(
            parse_rfc3339_to_epoch("2026-06-23 12:34:56Z"),
            parse_rfc3339_to_epoch("2026-06-23T12:34:56.999Z")
        );
        // Ordering is preserved, which is what the staleness check relies on.
        assert!(
            parse_rfc3339_to_epoch("2026-06-23T11:00:00Z")
                < parse_rfc3339_to_epoch("2026-06-23T12:00:00Z")
        );
    }

    #[test]
    fn parse_rfc3339_rejects_malformed() {
        assert_eq!(parse_rfc3339_to_epoch(""), None);
        assert_eq!(parse_rfc3339_to_epoch("not-a-date"), None);
        assert_eq!(parse_rfc3339_to_epoch("2026/06/23T12:00:00Z"), None);
        assert_eq!(parse_rfc3339_to_epoch("2026-13-01T00:00:00Z"), None);
    }

    #[test]
    fn parse_run_view_empty_conclusion_is_none() {
        let v = json!({
            "status": "in_progress",
            "conclusion": "",
            "url": "u",
            "jobs": []
        });
        let view = parse_run_view(&v);
        assert!(view.conclusion.is_none());
    }
}
