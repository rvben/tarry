mod cli;
mod engine;
mod probe;
mod probes;
mod schema;
mod verdict;

use std::io::IsTerminal;
use std::time::Duration;

use clap::Parser;
use clap::error::ErrorKind;

use cli::{Cli, Command, OutputFormat};
use engine::{EngineConfig, IntervalPolicy, SystemClock};
use probe::Probe;
use verdict::Verdict;

const EXIT_USAGE: i32 = 3;
const EXIT_ENVIRONMENT: i32 = 4;

fn main() {
    std::process::exit(real_main());
}

fn real_main() -> i32 {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            let code = match e.kind() {
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => 0,
                _ => EXIT_USAGE,
            };
            let _ = e.print();
            return code;
        }
    };

    if matches!(cli.command, Command::Schema) {
        println!("{}", schema::document());
        return 0;
    }

    let config = engine_config(&cli);
    let mut probe: Box<dyn Probe> = match build_probe(cli.command) {
        Ok(p) => p,
        Err(BuildError::Usage(msg)) => {
            eprintln!("error: {msg}");
            return EXIT_USAGE;
        }
        Err(BuildError::Environment(msg)) => {
            eprintln!("error: {msg}");
            return EXIT_ENVIRONMENT;
        }
    };

    let condition = probe.name();
    let mut clock = SystemClock;
    let outcome = engine::run(probe.as_mut(), &config, &mut clock);
    let verdict = Verdict::from_outcome(condition, outcome);

    let json = match cli.output {
        Some(OutputFormat::Json) => true,
        Some(OutputFormat::Text) => false,
        None => !std::io::stdout().is_terminal(),
    };
    if json {
        println!("{}", verdict.render_json());
    } else {
        println!("{}", verdict.render_text());
    }
    verdict.exit_code()
}

fn engine_config(cli: &Cli) -> EngineConfig {
    let is_run = matches!(cli.command, Command::Run { .. });
    let timeout = cli.timeout.unwrap_or(if is_run {
        Duration::from_secs(30 * 60)
    } else {
        Duration::from_secs(10 * 60)
    });
    let interval = match cli.interval {
        Some(d) => IntervalPolicy::Fixed(d),
        None if is_run => IntervalPolicy::Fixed(Duration::from_secs(10)),
        None => IntervalPolicy::Adaptive {
            start: Duration::from_secs(2),
            factor: 1.5,
            cap: Duration::from_secs(30),
        },
    };
    EngineConfig { timeout, interval }
}

enum BuildError {
    Usage(String),
    Environment(String),
}

fn build_probe(command: Command) -> Result<Box<dyn Probe>, BuildError> {
    match command {
        Command::Run {
            run_id,
            repo,
            workflow,
            branch,
        } => {
            probes::gh::GhCli::check().map_err(|e| BuildError::Environment(e.to_string()))?;
            // A bare `tarry run` waits on the current branch, as documented.
            // With an explicit run id or another repo, branch inference from
            // the working directory would be wrong.
            let branch = branch.or_else(|| {
                (run_id.is_none() && repo.is_none())
                    .then(probes::gh::current_git_branch)
                    .flatten()
            });
            Ok(Box::new(probes::run::RunProbe {
                gh: Box::new(probes::gh::GhCli),
                repo,
                branch,
                workflow,
                run_id,
                started_at: std::time::SystemTime::now(),
            }))
        }
        Command::Http {
            url,
            status,
            contains,
            json_path,
            headers,
        } => {
            let json_paths = json_path
                .iter()
                .map(|s| probes::http::parse_json_path(s))
                .collect::<Result<Vec<_>, _>>()
                .map_err(BuildError::Usage)?;
            let headers = headers
                .iter()
                .map(|h| {
                    h.split_once(':')
                        .map(|(name, value)| (name.trim().to_string(), value.trim().to_string()))
                        .ok_or_else(|| {
                            BuildError::Usage(format!("--header '{h}' must be 'Name: value'"))
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Box::new(probes::http::HttpProbe {
                url,
                status,
                contains,
                json_paths,
                headers,
                request_timeout: Duration::from_secs(10),
            }))
        }
        Command::Tcp { addr } => Ok(Box::new(probes::tcp::TcpProbe {
            addr,
            connect_timeout: Duration::from_secs(5),
        })),
        Command::File {
            path,
            contains,
            regex,
        } => {
            let matcher = match (contains, regex) {
                (Some(s), None) => probes::file::FileMatcher::Contains(s),
                (None, Some(r)) => probes::file::FileMatcher::Regex(
                    regex::Regex::new(&r)
                        .map_err(|e| BuildError::Usage(format!("--regex: {e}")))?,
                ),
                (None, None) => probes::file::FileMatcher::None,
                (Some(_), Some(_)) => unreachable!("clap conflicts_with prevents this"),
            };
            Ok(Box::new(probes::file::FileProbe { path, matcher }))
        }
        Command::Cmd { ok_output, command } => {
            let ok_output = ok_output
                .map(|r| regex::Regex::new(&r))
                .transpose()
                .map_err(|e| BuildError::Usage(format!("--ok-output: {e}")))?;
            Ok(Box::new(probes::cmd::CmdProbe {
                argv: command,
                ok_output,
            }))
        }
        Command::Schema => unreachable!("schema handled before probe construction"),
    }
}
