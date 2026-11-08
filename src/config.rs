//! Optional TOML configuration (`$XDG_CONFIG_HOME/liner/config.toml`).
//!
//! Owner: developer C (M3). Public signatures are frozen; internals are free.
//!
//! Example:
//! ```toml
//! library_path = "/storage/music"
//! include_hidden = false
//! follow_symlinks = false
//! fail_on = "error"
//! min_severity = "info"
//! ```
//! All keys optional; unknown keys are an error (catches typos). CLI arguments
//! override config values; config values override built-in defaults.
//!
//! A relative `library_path` is resolved against the directory containing the
//! config file (not the current directory), so the configured library does not
//! depend on where liner is run from. `~` is not expanded.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::model::Severity;

/// `--fail-on` threshold. Exit code 1 if any issue has severity >= threshold.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum FailOn {
    #[default]
    Error,
    Warning,
    Info,
    Never,
}

impl FailOn {
    /// `true` if an issue of `severity` should make the run fail.
    pub fn is_triggered_by(self, severity: Severity) -> bool {
        match self {
            FailOn::Never => false,
            FailOn::Error => severity >= Severity::Error,
            FailOn::Warning => severity >= Severity::Warning,
            FailOn::Info => true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub library_path: Option<PathBuf>,
    pub include_hidden: Option<bool>,
    pub follow_symlinks: Option<bool>,
    pub fail_on: Option<FailOn>,
    pub min_severity: Option<Severity>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read config file {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid config file {path}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
}

/// `$XDG_CONFIG_HOME/liner/config.toml`, falling back to `$HOME/.config/liner/config.toml`.
/// `None` if neither variable is set (non-empty, absolute).
pub fn default_config_path() -> Option<PathBuf> {
    config_path_from(
        std::env::var_os("XDG_CONFIG_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
}

/// Pure core of [`default_config_path`]. Empty or relative values are ignored, as
/// the XDG base directory spec requires.
fn config_path_from(xdg_config_home: Option<&OsStr>, home: Option<&OsStr>) -> Option<PathBuf> {
    fn usable(v: Option<&OsStr>) -> Option<&Path> {
        v.map(Path::new)
            .filter(|p| !p.as_os_str().is_empty() && p.is_absolute())
    }
    usable(xdg_config_home)
        .map(Path::to_path_buf)
        .or_else(|| usable(home).map(|h| h.join(".config")))
        .map(|dir| dir.join("liner").join("config.toml"))
}

/// Load configuration.
/// - `explicit = Some(p)`: `p` must exist and parse (error otherwise).
/// - `explicit = None`: use [`default_config_path`]; a missing file yields `Config::default()`.
pub fn load(explicit: Option<&Path>) -> Result<Config, ConfigError> {
    let (path, required) = match explicit {
        Some(p) => (p.to_path_buf(), true),
        None => match default_config_path() {
            Some(p) => (p, false),
            None => return Ok(Config::default()),
        },
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let mut config = parse(&text, &path)?;
            config.library_path = config
                .library_path
                .map(|lib| resolve_library_path(&path, lib));
            Ok(config)
        }
        Err(e) if !required && e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(source) => Err(ConfigError::Read { path, source }),
    }
}

/// Resolve a relative `library_path` against the config file's directory.
fn resolve_library_path(config_path: &Path, library_path: PathBuf) -> PathBuf {
    if library_path.is_absolute() {
        return library_path;
    }
    let config_path = std::path::absolute(config_path).unwrap_or_else(|_| config_path.into());
    match config_path.parent() {
        Some(dir) => dir.join(library_path),
        None => library_path,
    }
}

/// Parse config text (used by `load`; separate for unit tests). Paths are returned
/// as written; `load` resolves a relative `library_path`.
pub fn parse(text: &str, path: &Path) -> Result<Config, ConfigError> {
    toml::from_str(text).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as _;

    fn os(s: &str) -> Option<&OsStr> {
        Some(OsStr::new(s))
    }

    #[test]
    fn default_path_prefers_xdg_then_home() {
        assert_eq!(
            config_path_from(os("/x"), os("/h")),
            Some(PathBuf::from("/x/liner/config.toml"))
        );
        assert_eq!(
            config_path_from(None, os("/h")),
            Some(PathBuf::from("/h/.config/liner/config.toml"))
        );
        // Empty or relative XDG_CONFIG_HOME is ignored.
        assert_eq!(
            config_path_from(os(""), os("/h")),
            Some(PathBuf::from("/h/.config/liner/config.toml"))
        );
        assert_eq!(
            config_path_from(os("rel"), os("/h")),
            Some(PathBuf::from("/h/.config/liner/config.toml"))
        );
        assert_eq!(config_path_from(None, os("")), None);
        assert_eq!(config_path_from(os("rel"), os("rel")), None);
        assert_eq!(config_path_from(None, None), None);
    }

    #[test]
    fn parses_all_keys() {
        let c = parse(
            "library_path = \"/m\"\ninclude_hidden = true\nfollow_symlinks = true\n\
             fail_on = \"never\"\nmin_severity = \"warning\"\n",
            Path::new("c.toml"),
        )
        .unwrap();
        assert_eq!(
            c,
            Config {
                library_path: Some("/m".into()),
                include_hidden: Some(true),
                follow_symlinks: Some(true),
                fail_on: Some(FailOn::Never),
                min_severity: Some(Severity::Warning),
            }
        );
        assert_eq!(parse("", Path::new("c.toml")).unwrap(), Config::default());
    }

    #[test]
    fn unknown_key_and_bad_value_are_errors() {
        assert!(matches!(
            parse("libary_path = \"/m\"", Path::new("c.toml")),
            Err(ConfigError::Parse { .. })
        ));
        assert!(matches!(
            parse("fail_on = \"sometimes\"", Path::new("c.toml")),
            Err(ConfigError::Parse { .. })
        ));
    }

    #[test]
    fn relative_library_path_is_resolved_against_config_dir() {
        assert_eq!(
            resolve_library_path(Path::new("/etc/liner/c.toml"), "music".into()),
            PathBuf::from("/etc/liner/music")
        );
        assert_eq!(
            resolve_library_path(Path::new("/etc/liner/c.toml"), "/m".into()),
            PathBuf::from("/m")
        );
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(
            resolve_library_path(Path::new("c.toml"), "music".into()),
            cwd.join("music")
        );
    }

    #[test]
    fn explicit_missing_config_is_an_error() {
        let err = load(Some(Path::new("/nonexistent/liner/config.toml"))).unwrap_err();
        assert!(matches!(err, ConfigError::Read { .. }));
    }

    #[test]
    fn display_does_not_repeat_source() {
        let parse_err = parse("x = 1", Path::new("c.toml")).unwrap_err();
        let read_err = load(Some(Path::new("/nonexistent/c.toml"))).unwrap_err();
        for err in [parse_err, read_err] {
            let source = err.source().expect("has a source").to_string();
            assert!(!err.to_string().contains(&source), "{err}");
        }
    }

    #[test]
    fn fail_on_thresholds() {
        use Severity::*;
        assert!(FailOn::Error.is_triggered_by(Error));
        assert!(!FailOn::Error.is_triggered_by(Warning));
        assert!(FailOn::Warning.is_triggered_by(Warning));
        assert!(!FailOn::Warning.is_triggered_by(Info));
        assert!(FailOn::Info.is_triggered_by(Info));
        assert!(!FailOn::Never.is_triggered_by(Error));
    }
}
