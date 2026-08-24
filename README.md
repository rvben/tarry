# tarry

Block until a condition holds, then print one compact verdict.

`tarry` exists to kill sleep-poll loops. Instead of `sleep 60; check; sleep 60; check; ...` (each iteration a fresh command, a fresh read, a fresh decision), you run one blocking command that returns when the condition is met, timed out, or terminally failed, with a single machine-readable result. It is built for agents and scripts: JSON when piped, text on a TTY, stable exit codes, and a `tarry schema` contract (clispec v0.2).

## Install

```sh
cargo install tarry
```

## Conditions

Wait for a GitHub Actions run to complete (defaults to the latest run for the current repo and branch; requires the `gh` CLI):

```sh
tarry gh run                       # latest run, current repo and branch
tarry gh run 27336187124           # specific run id
tarry gh run --workflow Release    # latest Release run (any branch; finds tag-triggered runs)
tarry gh run -R rvben/upd --workflow Release
```

`gh run` mirrors GitHub's own `gh run` CLI. `--workflow` resolves the latest run of that workflow regardless of branch, so it works for release workflows triggered by tags; pass `--branch` to also scope by branch.

A red run stops polling immediately (exit 2) and the verdict carries a bounded digest: failed job, failed step, and the last 20 lines of the failed step's log.

Wait for an HTTP endpoint:

```sh
tarry http https://example.com/health
tarry http https://example.com/version --contains 1.2.3
tarry http https://api.example.com/status --json-path status.state=ready
```

Wait for a TCP port, a file, or any command:

```sh
tarry tcp 192.0.2.10:22
tarry file /tmp/review.txt --contains "verdict:"
tarry cmd -- vership verify                  # composition point
tarry cmd --ok-output 'state=ready' -- mytool status
```

## Flags

- `--timeout <dur>`: give up after this long (humantime syntax: `30s`, `10m`, `1h30m`). Default 10m, `run` defaults to 30m.
- `--interval <dur>`: fixed poll interval. Default: adaptive from 2s up to 30s; `run` polls every 10s.
- `-o, --output <json|text>`: output format. Default: JSON when piped, text on a TTY.

## Output

Silent while waiting; exactly one verdict on stdout when done:

```json
{"condition":"run","ok":true,"kind":"met","waited_s":252,"polls":26,"detail":{"run_id":27336187124,"conclusion":"success","url":"..."}}
```

## Exit codes

| Code | Kind | Meaning |
|------|------|---------|
| 0 | met | Condition met |
| 1 | timeout | Timeout expired before the condition was met (retryable) |
| 2 | condition_failed | Terminal failure, e.g. the run concluded red |
| 3 | usage | Invalid arguments |
| 4 | environment | Missing dependency, e.g. `gh` not installed |

Transient errors (connection refused, 5xx, gh hiccups) never fail a wait; they count as "not yet" and surface in the verdict only if the timeout expires.

## Agent integration

`tarry schema` prints the full machine-readable contract (commands, arguments, output fields, outcomes, errors) following clispec v0.2. It needs no network, auth, or config.

## Releasing

Vership owns versioning, changelog generation, release commits, and tags. See
[the release runbook](docs/releases.md) for the verified workflow and recovery policy.
