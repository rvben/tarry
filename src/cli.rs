use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Json,
    Text,
}

#[derive(Parser)]
#[command(
    name = "tarry",
    version,
    about = "Block until a condition holds, then print one compact verdict.",
    after_help = "Run `tarry schema` for the machine-readable contract (clispec v0.2).\n\
                  Exit codes: 0 met, 1 timeout, 2 condition failed, 3 usage, 4 environment."
)]
pub struct Cli {
    /// Give up after this long (e.g. 30s, 10m, 1h30m). Default 10m (30m for run).
    #[arg(long, global = true, value_parser = humantime::parse_duration)]
    pub timeout: Option<Duration>,

    /// Fixed poll interval. Default: adaptive 2s..30s (fixed 10s for run).
    #[arg(long, global = true, value_parser = humantime::parse_duration)]
    pub interval: Option<Duration>,

    /// Output format. Default: json when piped, text on a TTY.
    #[arg(short = 'o', long, global = true, value_enum)]
    pub output: Option<OutputFormat>,

    #[command(subcommand)]
    pub command: Command,
}

/// Arguments for `gh run`.
#[derive(Args)]
pub struct RunArgs {
    /// Run id. Defaults to the latest run for the current repo and branch.
    pub run_id: Option<u64>,
    /// Repository as owner/name. Defaults to the repo of the current directory.
    #[arg(short = 'R', long)]
    pub repo: Option<String>,
    /// Resolve the latest run of this workflow (name or file). Takes priority
    /// over current-branch inference, so it finds tag-triggered runs (e.g.
    /// releases); pass --branch to also scope by branch.
    #[arg(long)]
    pub workflow: Option<String>,
    /// Filter the latest-run lookup by branch.
    #[arg(long)]
    pub branch: Option<String>,
}

/// GitHub waits, namespaced to mirror the `gh` CLI (`gh run`, ...).
#[derive(Subcommand)]
pub enum GhCommand {
    /// Wait for a GitHub Actions run to complete.
    Run(RunArgs),
}

#[derive(Subcommand)]
pub enum Command {
    /// Wait on GitHub Actions, via the gh CLI. Mirrors `gh run`.
    Gh {
        #[command(subcommand)]
        command: GhCommand,
    },
    /// Wait for an HTTP endpoint to match.
    Http {
        url: String,
        /// Require this exact status code (default: any 2xx).
        #[arg(long)]
        status: Option<u16>,
        /// Require the body to contain this string (repeatable).
        #[arg(long)]
        contains: Vec<String>,
        /// Require a JSON body field to equal a value: <dot.path>=<value> (repeatable).
        #[arg(long = "json-path")]
        json_path: Vec<String>,
        /// Extra request header as 'Name: value' (repeatable).
        #[arg(long = "header")]
        headers: Vec<String>,
    },
    /// Wait for a TCP port to accept connections.
    Tcp {
        /// host:port
        addr: String,
    },
    /// Wait for a file to exist (and optionally match).
    File {
        path: PathBuf,
        /// Require the file content to contain this string.
        #[arg(long, conflicts_with = "regex")]
        contains: Option<String>,
        /// Require the file content to match this regex.
        #[arg(long)]
        regex: Option<String>,
    },
    /// Wait for a command to succeed (exit 0, or --ok-output match).
    Cmd {
        /// Treat a match of this regex against combined stdout+stderr as success,
        /// regardless of exit code.
        #[arg(long = "ok-output")]
        ok_output: Option<String>,
        /// The command and its arguments (after --).
        #[arg(required = true, last = true)]
        command: Vec<String>,
    },
    /// Print the machine-readable clispec contract.
    Schema,
}
