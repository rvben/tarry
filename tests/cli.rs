use std::time::Duration;

use assert_cmd::Command;
use predicates::prelude::*;

fn tarry() -> Command {
    Command::cargo_bin("tarry").unwrap()
}

#[test]
fn file_already_present_exits_0_with_json_when_piped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ready.txt");
    std::fs::write(&path, "done").unwrap();
    tarry()
        .args(["file", path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ok\":true"))
        .stdout(predicate::str::contains("\"condition\":\"file\""));
}

#[test]
fn file_missing_times_out_with_exit_1() {
    tarry()
        .args([
            "--timeout",
            "1s",
            "--interval",
            "200ms",
            "file",
            "/nonexistent/tarry-test.txt",
        ])
        .timeout(Duration::from_secs(10))
        .assert()
        .code(1)
        .stdout(predicate::str::contains("\"kind\":\"timeout\""));
}

#[test]
fn cmd_succeeds_immediately() {
    tarry()
        .args(["cmd", "--", "true"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"condition\":\"cmd\""));
}

#[test]
fn cmd_flapping_succeeds_on_second_poll() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("marker");
    let script = format!(
        "if [ -f {m} ]; then exit 0; else touch {m}; exit 1; fi",
        m = marker.display()
    );
    tarry()
        .args(["--interval", "100ms", "cmd", "--", "sh", "-c", &script])
        .timeout(Duration::from_secs(10))
        .assert()
        .success()
        .stdout(predicate::str::contains("\"polls\":2"));
}

#[test]
fn text_output_forced_with_o_flag() {
    tarry()
        .args(["-o", "text", "cmd", "--", "true"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("ok:"));
}

#[test]
fn invalid_json_path_is_usage_error_exit_3() {
    tarry()
        .args(["http", "http://127.0.0.1:1/", "--json-path", "no-equals"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains("json-path"));
}

#[test]
fn unknown_flag_is_usage_error_exit_3() {
    tarry().args(["--no-such-flag"]).assert().code(3);
}

#[test]
fn tcp_open_port_succeeds() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    tarry().args(["tcp", &addr]).assert().success();
}

#[test]
fn schema_is_valid_clispec_v02_json() {
    let output = tarry().arg("schema").assert().success();
    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(doc["clispec"], "0.2");
    assert_eq!(doc["name"], "tarry");
    let commands: Vec<&str> = doc["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    for expected in ["gh run", "http", "tcp", "file", "cmd", "schema"] {
        assert!(commands.contains(&expected), "missing command {expected}");
    }
    // cmd executes arbitrary user commands; everything else is read-only.
    for c in doc["commands"].as_array().unwrap() {
        let expected_mutating = c["name"] == "cmd";
        assert_eq!(c["mutating"], expected_mutating, "command {}", c["name"]);
    }
    // Outcomes: timeout (1, retryable) and condition_failed (2, not retryable).
    let outcomes = doc["outcomes"].as_array().unwrap();
    assert!(
        outcomes
            .iter()
            .any(|o| o["kind"] == "timeout" && o["exit_code"] == 1 && o["retryable"] == true)
    );
    assert!(outcomes.iter().any(|o| o["kind"] == "condition_failed"
        && o["exit_code"] == 2
        && o["retryable"] == false));
    let errors = doc["errors"].as_array().unwrap();
    assert!(
        errors
            .iter()
            .any(|e| e["kind"] == "usage" && e["exit_code"] == 3)
    );
    assert!(
        errors
            .iter()
            .any(|e| e["kind"] == "environment" && e["exit_code"] == 4)
    );
}

#[test]
fn help_mentions_schema() {
    tarry()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("tarry schema"));
}

#[test]
fn gh_is_listed_and_run_is_namespaced() {
    // Top-level help advertises `gh`; `gh --help` lists the nested `run`.
    tarry()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("gh"));
    tarry()
        .args(["gh", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("run"));
}

#[test]
fn gh_run_parses_and_top_level_run_is_removed() {
    // `gh run` is the only spelling. `--help` short-circuits before any gh call.
    tarry()
        .args(["gh", "run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--workflow"));
    // The old top-level `run` alias is gone: clap rejects it as unknown.
    tarry()
        .args(["run", "--workflow", "Release"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("unrecognized subcommand")
                .or(predicate::str::contains("unexpected argument")),
        );
}
