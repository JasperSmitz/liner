//! Command-line interface (clap derive) and command dispatch.
//!
//! Owner: developer C (M3). The argument surface below is frozen (it is documented
//! in docs/ARCHITECTURE.md); internals are free.
//!
//! Exit codes (see [`Outcome::exit_code`] and `main.rs`):
//! 0 = no issue at/above `--fail-on`; 1 = such issues found; 2 = runtime/usage error
//! (clap itself exits with 2 on usage errors).

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

use crate::config::{self, Config, FailOn};
use crate::error::{LinerError, Result};
use crate::model::Severity;
use crate::render::{self, OutputFormat};
use crate::scan::ScanOptions;

#[derive(Debug, Parser)]
#[command(
    name = "liner",
    version,
    about = "Read-only music library metadata linter",
    propagate_version = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Scan a library and report metadata issues.
    Scan(ScanArgs),
    /// List all rule codes with severity, scope and description.
    Rules,
    /// Print the JSON Schema of the report.
    Schema,
}

#[derive(Debug, Clone, Default, Args)]
pub struct ScanArgs {
    /// Library root. Defaults to `library_path` from the config file.
    pub path: Option<PathBuf>,
    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
    pub format: OutputFormat,
    /// Write the report to FILE instead of stdout (never an audio file).
    #[arg(long, value_name = "FILE")]
    pub output: Option<PathBuf>,
    /// Exit with code 1 if any issue has at least this severity [default: error].
    #[arg(long, value_enum)]
    pub fail_on: Option<FailOn>,
    /// Only include issues of at least this severity in the output [default: info].
    #[arg(long, value_enum)]
    pub min_severity: Option<Severity>,
    /// Follow symbolic links while walking.
    #[arg(long)]
    pub follow_symlinks: bool,
    /// Include hidden files and directories (names starting with '.').
    #[arg(long)]
    pub include_hidden: bool,
    /// Config file [default: $XDG_CONFIG_HOME/liner/config.toml].
    #[arg(long, value_name = "FILE")]
    pub config: Option<PathBuf>,
}

/// Effective settings after merging CLI args over config over defaults.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanSettings {
    pub root: PathBuf,
    pub scan: ScanOptions,
    pub format: OutputFormat,
    pub output: Option<PathBuf>,
    pub fail_on: FailOn,
    pub min_severity: Severity,
}

/// Merge CLI args over config. Boolean flags can only switch a setting on.
pub fn resolve_scan_settings(args: &ScanArgs, config: &Config) -> Result<ScanSettings> {
    let root = args
        .path
        .clone()
        .or_else(|| config.library_path.clone())
        .ok_or(LinerError::NoLibraryPath)?;
    Ok(ScanSettings {
        root,
        scan: ScanOptions {
            follow_symlinks: args.follow_symlinks || config.follow_symlinks.unwrap_or(false),
            include_hidden: args.include_hidden || config.include_hidden.unwrap_or(false),
        },
        format: args.format,
        output: args.output.clone(),
        fail_on: args.fail_on.or(config.fail_on).unwrap_or_default(),
        min_severity: args
            .min_severity
            .or(config.min_severity)
            .unwrap_or(Severity::Info),
    })
}

/// Successful run result, mapped to an exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Exit 0.
    Clean,
    /// Exit 1: issues at/above the `--fail-on` threshold.
    IssuesFound,
}

impl Outcome {
    pub fn exit_code(self) -> ExitCode {
        match self {
            Outcome::Clean => ExitCode::SUCCESS,
            Outcome::IssuesFound => ExitCode::from(1),
        }
    }
}

/// Run a parsed command, writing primary output to `stdout`.
pub fn run(cli: Cli, stdout: &mut dyn Write) -> Result<Outcome> {
    match cli.command {
        Command::Scan(args) => run_scan(&args, stdout),
        Command::Rules => {
            for r in crate::rules::CATALOG {
                writeln!(
                    stdout,
                    "{:<30} {:<8} {:<8} {}",
                    r.code,
                    r.severity.as_str(),
                    format!("{:?}", r.scope).to_lowercase(),
                    r.summary
                )
                .map_err(LinerError::Output)?;
            }
            Ok(Outcome::Clean)
        }
        Command::Schema => {
            let schema = crate::report::json_schema();
            serde_json::to_writer_pretty(&mut *stdout, &schema)
                .map_err(|e| LinerError::Output(e.into()))?;
            writeln!(stdout).map_err(LinerError::Output)?;
            Ok(Outcome::Clean)
        }
    }
}

fn run_scan(args: &ScanArgs, stdout: &mut dyn Write) -> Result<Outcome> {
    let config = config::load(args.config.as_deref())?;
    let settings = resolve_scan_settings(args, &config)?;

    // Validate the output target before doing any work.
    let mut file_out = match &settings.output {
        Some(path) => Some(render::create_output_file(path)?),
        None => None,
    };

    let mut report = crate::analyze(&settings.root, &settings.scan, jiff::Timestamp::now())?;

    // --fail-on is evaluated on ALL issues, before --min-severity filtering.
    let failed = report
        .max_severity()
        .is_some_and(|s| settings.fail_on.is_triggered_by(s));
    report.retain_min_severity(settings.min_severity);

    let out: &mut dyn Write = match file_out.as_mut() {
        Some(f) => f,
        None => stdout,
    };
    render::render(&report, settings.format, out).map_err(LinerError::Output)?;
    out.flush().map_err(LinerError::Output)?;

    Ok(if failed {
        Outcome::IssuesFound
    } else {
        Outcome::Clean
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn clap_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn cli_overrides_config() {
        let config = Config {
            library_path: Some("/cfg".into()),
            fail_on: Some(FailOn::Warning),
            ..Config::default()
        };
        let args = ScanArgs {
            path: Some("/cli".into()),
            ..ScanArgs::default()
        };
        let s = resolve_scan_settings(&args, &config).unwrap();
        assert_eq!(s.root, PathBuf::from("/cli"));
        assert_eq!(s.fail_on, FailOn::Warning);
        assert_eq!(s.min_severity, Severity::Info);
    }

    #[test]
    fn missing_root_is_an_error() {
        let err = resolve_scan_settings(&ScanArgs::default(), &Config::default());
        assert!(matches!(err, Err(LinerError::NoLibraryPath)));
    }
}
