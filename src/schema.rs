use serde_json::json;

pub fn document() -> String {
    let verdict_fields = json!([
        {"name": "condition", "type": "string", "description": "Condition type that was awaited"},
        {"name": "ok", "type": "boolean", "description": "True when the condition was met"},
        {"name": "kind", "type": "string", "enum": ["met", "timeout", "condition_failed"], "description": "Outcome kind"},
        {"name": "waited_s", "type": "integer", "description": "Seconds spent waiting"},
        {"name": "polls", "type": "integer", "description": "Number of polls performed"},
        {"name": "detail", "type": "object", "description": "Condition-specific detail; failure digests are bounded to 20 log lines and set detail.truncated=true when clipped"}
    ]);

    let doc = json!({
        "clispec": "0.2",
        "name": "tarry",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Block until a condition holds, then print one compact verdict.",
        "global_args": [
            {"name": "--timeout", "type": "duration", "required": false, "default": "10m (30m for run)", "description": "Give up after this long (humantime syntax, e.g. 30s, 10m, 1h30m)"},
            {"name": "--interval", "type": "duration", "required": false, "default": "adaptive 2s..30s (fixed 10s for run)", "description": "Fixed poll interval"},
            {"name": "--output", "type": "string", "required": false, "enum": ["json", "text"], "default": "json when piped, text on TTY", "description": "Output format (-o)"}
        ],
        "commands": [
            {
                "name": "gh run",
                "description": "Wait for a GitHub Actions run to complete. A red run is a terminal outcome: polling stops immediately and the verdict carries a bounded failure digest. Also accepts the back-compat alias `run`.",
                "mutating": false,
                "args": [
                    {"name": "run_id", "type": "integer", "required": false, "description": "Run id; defaults to the latest run for the current repo and branch"},
                    {"name": "--repo", "type": "string", "required": false, "description": "Repository as owner/name (-R); defaults to the repo of the current directory"},
                    {"name": "--workflow", "type": "string", "required": false, "description": "Resolve the latest run of this workflow (name or file); takes priority over current-branch inference so it finds tag-triggered runs such as releases"},
                    {"name": "--branch", "type": "string", "required": false, "description": "Filter the latest-run lookup by branch"}
                ],
                "output_fields": verdict_fields,
                "notes": "Requires the gh CLI, installed and authenticated. Missing gh exits 4 (environment)."
            },
            {
                "name": "http",
                "description": "Wait for an HTTP endpoint to match status and content conditions.",
                "mutating": false,
                "args": [
                    {"name": "url", "type": "string", "required": true, "description": "URL to poll with GET requests"},
                    {"name": "--status", "type": "integer", "required": false, "description": "Require this exact status code (default: any 2xx)"},
                    {"name": "--contains", "type": "string", "required": false, "repeatable": true, "description": "Require the body to contain this string"},
                    {"name": "--json-path", "type": "string", "required": false, "repeatable": true, "description": "Require a JSON body field to equal a value: <dot.path>=<value>, e.g. status.state=ready or jobs.0.state=done"},
                    {"name": "--header", "type": "string", "required": false, "repeatable": true, "description": "Extra request header as 'Name: value'"}
                ],
                "output_fields": verdict_fields
            },
            {
                "name": "tcp",
                "description": "Wait for a TCP port to accept connections.",
                "mutating": false,
                "args": [
                    {"name": "addr", "type": "string", "required": true, "description": "host:port"}
                ],
                "output_fields": verdict_fields
            },
            {
                "name": "file",
                "description": "Wait for a file to exist and optionally match content.",
                "mutating": false,
                "args": [
                    {"name": "path", "type": "string", "required": true, "description": "File path to watch"},
                    {"name": "--contains", "type": "string", "required": false, "description": "Require the file content to contain this string (conflicts with --regex)"},
                    {"name": "--regex", "type": "string", "required": false, "description": "Require the file content to match this regex"}
                ],
                "output_fields": verdict_fields
            },
            {
                "name": "cmd",
                "description": "Wait for a command to succeed: exit 0, or --ok-output match. The command and its arguments follow a -- separator.",
                "mutating": true,
                "args": [
                    {"name": "--ok-output", "type": "string", "required": false, "description": "Treat a match of this regex against combined stdout+stderr as success, regardless of exit code"},
                    {"name": "command", "type": "array", "required": true, "description": "The command and its arguments, after --"}
                ],
                "output_fields": verdict_fields,
                "notes": "Executes the supplied command once per poll; mutating if the wrapped command is."
            },
            {
                "name": "schema",
                "description": "Print this machine-readable clispec contract. Needs no network, auth, or config.",
                "mutating": false,
                "args": [],
                "output_fields": []
            }
        ],
        "outcomes": [
            {"kind": "timeout", "exit_code": 1, "retryable": true, "description": "Timeout expired before the condition was met; detail.last_note carries the most recent transient error"},
            {"kind": "condition_failed", "exit_code": 2, "retryable": false, "description": "The condition failed terminally (e.g. the run concluded red)"}
        ],
        "errors": [
            {"kind": "usage", "exit_code": 3, "retryable": false, "message": "Invalid arguments", "hint": "Run tarry --help"},
            {"kind": "environment", "exit_code": 4, "retryable": false, "message": "Missing dependency", "hint": "Install and authenticate the gh CLI"}
        ]
    });
    serde_json::to_string_pretty(&doc).expect("schema document serializes")
}
