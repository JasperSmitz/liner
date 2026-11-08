//! Command-line interface (clap derive) and command dispatch.
//!
//! Owner: developer C (M3). The argument surface below is frozen (it is documented
//! in docs/ARCHITECTURE.md); internals are free.
//!
//! Exit codes (see [`outcome`], [`Outcome::exit_code`] and `main.rs`):
//! 0 = no issue at/above `--fail-on` and no unreadable file; 1 = such issues found,
//! or `errors[]` is non-empty (treated as error severity, so only `--fail-on never`
//! ignores it); 2 = runtime/usage error (clap itself exits with 2 on usage errors).
//!
//! A closed stdout (`liner scan | head`) is not an error: output stops quietly and
//! the exit code is still computed from the report.

use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

use crate::config::{self, Config, FailOn};
use crate::error::{LinerError, Result};
use crate::model::Severity;
use crate::render::{self, ColorChoice, HumanOptions, OutputFormat};
use crate::report::Report;
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
    /// Colour human output: auto = only on a terminal with NO_COLOR unset.
    #[arg(long, value_enum, default_value_t = ColorChoice::Auto, value_name = "WHEN")]
    pub color: ColorChoice,
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
    pub color: ColorChoice,
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
        color: args.color,
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

/// Exit outcome for a run. Pure.
///
/// `max_issue_severity` must be taken from ALL issues (before `--min-severity`
/// filtering). Unreadable files (`errors[]`) count as error severity: they make the
/// run fail at every threshold except [`FailOn::Never`].
pub fn outcome(
    fail_on: FailOn,
    max_issue_severity: Option<Severity>,
    unreadable_files: usize,
) -> Outcome {
    let issue_fail = max_issue_severity.is_some_and(|s| fail_on.is_triggered_by(s));
    let error_fail = unreadable_files > 0 && fail_on.is_triggered_by(Severity::Error);
    if issue_fail || error_fail {
        Outcome::IssuesFound
    } else {
        Outcome::Clean
    }
}

/// [`outcome`] for an unfiltered report.
pub fn report_outcome(report: &Report, fail_on: FailOn) -> Outcome {
    outcome(fail_on, report.max_severity(), report.errors.len())
}

/// Treat a closed reader on stdout (EPIPE) as success.
fn ignore_broken_pipe(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        other => other,
    }
}

/// Run a parsed command, writing primary output to `stdout`.
pub fn run(cli: Cli, stdout: &mut dyn Write) -> Result<Outcome> {
    match cli.command {
        Command::Scan(args) => run_scan(&args, stdout),
        Command::Rules => {
            ignore_broken_pipe(write_rules(stdout)).map_err(LinerError::Output)?;
            Ok(Outcome::Clean)
        }
        Command::Schema => {
            ignore_broken_pipe(write_schema(stdout)).map_err(LinerError::Output)?;
            Ok(Outcome::Clean)
        }
    }
}

fn write_rules(out: &mut dyn Write) -> io::Result<()> {
    for r in crate::rules::CATALOG {
        writeln!(
            out,
            "{:<30} {:<8} {:<8} {}",
            r.code,
            r.severity.as_str(),
            format!("{:?}", r.scope).to_lowercase(),
            r.summary
        )?;
    }
    out.flush()
}

fn write_schema(out: &mut dyn Write) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut *out, &crate::report::json_schema())?;
    writeln!(out)?;
    out.flush()
}

fn run_scan(args: &ScanArgs, stdout: &mut dyn Write) -> Result<Outcome> {
    let config = config::load(args.config.as_deref())?;
    let settings = resolve_scan_settings(args, &config)?;

    // Cheap checks of the output target before doing any work; the file itself is
    // opened only once the report is fully rendered, so a failed run never empties
    // an existing file.
    if let Some(path) = &settings.output {
        render::check_output_path(path, &settings.root)?;
    }

    let mut report = crate::analyze(&settings.root, &settings.scan, jiff::Timestamp::now())?;

    // --fail-on is evaluated on ALL issues (and errors[]), before --min-severity.
    let result = report_outcome(&report, settings.fail_on);
    report.retain_min_severity(settings.min_severity);

    match &settings.output {
        Some(path) => {
            let human = HumanOptions {
                color: settings.color == ColorChoice::Always,
            };
            let mut rendered = Vec::new();
            render::render_with(&report, settings.format, human, &mut rendered)
                .map_err(LinerError::Output)?;
            let mut file = render::create_output_file(path)?;
            file.write_all(&rendered)
                .and_then(|()| file.flush())
                .map_err(LinerError::Output)?;
        }
        None => {
            let color = render::use_color(
                settings.color,
                io::stdout().is_terminal(),
                std::env::var_os("NO_COLOR").as_deref(),
            );
            let human = HumanOptions { color };
            ignore_broken_pipe(
                render::render_with(&report, settings.format, human, stdout)
                    .and_then(|()| stdout.flush()),
            )
            .map_err(LinerError::Output)?;
        }
    }

    Ok(result)
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

    #[test]
    fn config_fills_in_what_cli_leaves_out() {
        let config = Config {
            library_path: Some("/cfg".into()),
            include_hidden: Some(true),
            fail_on: Some(FailOn::Info),
            min_severity: Some(Severity::Warning),
            ..Config::default()
        };
        let s = resolve_scan_settings(&ScanArgs::default(), &config).unwrap();
        assert_eq!(s.root, PathBuf::from("/cfg"));
        assert!(s.scan.include_hidden);
        assert!(!s.scan.follow_symlinks);
        assert_eq!(s.fail_on, FailOn::Info);
        assert_eq!(s.min_severity, Severity::Warning);

        let args = ScanArgs {
            fail_on: Some(FailOn::Never),
            min_severity: Some(Severity::Error),
            follow_symlinks: true,
            ..ScanArgs::default()
        };
        let s = resolve_scan_settings(&args, &config).unwrap();
        assert_eq!(s.fail_on, FailOn::Never);
        assert_eq!(s.min_severity, Severity::Error);
        assert!(s.scan.follow_symlinks);
    }

    #[test]
    fn defaults_without_config() {
        let args = ScanArgs {
            path: Some("/lib".into()),
            ..ScanArgs::default()
        };
        let s = resolve_scan_settings(&args, &Config::default()).unwrap();
        assert_eq!(s.fail_on, FailOn::Error);
        assert_eq!(s.min_severity, Severity::Info);
        assert_eq!(s.format, OutputFormat::Human);
        assert_eq!(s.color, ColorChoice::Auto);
        assert_eq!(s.scan, ScanOptions::default());
    }

    #[test]
    fn outcome_issue_thresholds() {
        use Severity::*;
        // below threshold -> 0, at/above -> 1
        assert_eq!(outcome(FailOn::Error, Some(Warning), 0), Outcome::Clean);
        assert_eq!(outcome(FailOn::Error, Some(Error), 0), Outcome::IssuesFound);
        assert_eq!(outcome(FailOn::Warning, Some(Info), 0), Outcome::Clean);
        assert_eq!(
            outcome(FailOn::Warning, Some(Warning), 0),
            Outcome::IssuesFound
        );
        assert_eq!(
            outcome(FailOn::Warning, Some(Error), 0),
            Outcome::IssuesFound
        );
        assert_eq!(outcome(FailOn::Info, Some(Info), 0), Outcome::IssuesFound);
        assert_eq!(outcome(FailOn::Never, Some(Error), 0), Outcome::Clean);
        for f in [FailOn::Error, FailOn::Warning, FailOn::Info, FailOn::Never] {
            assert_eq!(outcome(f, None, 0), Outcome::Clean);
        }
    }

    #[test]
    fn unreadable_files_count_as_errors() {
        for f in [FailOn::Error, FailOn::Warning, FailOn::Info] {
            assert_eq!(outcome(f, None, 1), Outcome::IssuesFound, "{f:?}");
            assert_eq!(outcome(f, Some(Severity::Info), 3), Outcome::IssuesFound);
        }
        assert_eq!(outcome(FailOn::Never, None, 5), Outcome::Clean);
    }

    #[test]
    fn exit_codes() {
        assert_eq!(Outcome::Clean.exit_code(), ExitCode::SUCCESS);
        assert_eq!(Outcome::IssuesFound.exit_code(), ExitCode::from(1));
    }

    /// A reader that went away (`liner ... | head`).
    struct ClosedPipe;
    impl Write for ClosedPipe {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
    }

    /// Any other write failure.
    struct FullDisk;
    impl Write for FullDisk {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::StorageFull.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("liner").chain(args.iter().copied())).unwrap()
    }

    #[test]
    fn broken_pipe_is_quiet() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap();
        // `--config /dev/null` (empty config) keeps the developer's real config out.
        for args in [
            vec!["rules"],
            vec!["schema"],
            vec!["scan", root, "--config", "/dev/null"],
            vec!["scan", root, "--format", "json", "--config", "/dev/null"],
        ] {
            let r = run(parse(&args), &mut ClosedPipe);
            assert!(matches!(r, Ok(Outcome::Clean)), "{args:?}: {r:?}");
        }
    }

    #[test]
    fn other_write_errors_are_reported() {
        let r = run(parse(&["rules"]), &mut FullDisk);
        assert!(matches!(r, Err(LinerError::Output(_))));
    }

    #[test]
    fn color_flag_parses() {
        let Command::Scan(a) = parse(&["scan", "--color", "never"]).command else {
            panic!("not scan")
        };
        assert_eq!(a.color, ColorChoice::Never);
        let Command::Scan(a) = parse(&["scan"]).command else {
            panic!("not scan")
        };
        assert_eq!(a.color, ColorChoice::Auto);
    }
}
