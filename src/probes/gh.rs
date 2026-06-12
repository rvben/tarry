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

pub trait GhClient {
    fn latest_run_id(
        &self,
        repo: Option<&str>,
        branch: Option<&str>,
        workflow: Option<&str>,
    ) -> Result<u64, GhError>;
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
    fn latest_run_id(
        &self,
        repo: Option<&str>,
        branch: Option<&str>,
        workflow: Option<&str>,
    ) -> Result<u64, GhError> {
        let mut args = vec!["run", "list", "--limit", "1", "--json", "databaseId"];
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
        runs.get(0)
            .and_then(|r| r["databaseId"].as_u64())
            .ok_or_else(|| GhError::Failed("no runs found".into()))
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
