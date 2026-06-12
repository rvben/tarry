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
