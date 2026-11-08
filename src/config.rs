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
    // M3 placeholder.
    None
}

/// Load configuration.
/// - `explicit = Some(p)`: `p` must exist and parse (error otherwise).
/// - `explicit = None`: use [`default_config_path`]; a missing file yields `Config::default()`.
pub fn load(explicit: Option<&Path>) -> Result<Config, ConfigError> {
    // M3 placeholder.
    let _ = explicit;
    Ok(Config::default())
}

/// Parse config text (used by `load`; separate for unit tests).
pub fn parse(text: &str, path: &Path) -> Result<Config, ConfigError> {
    toml::from_str(text).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    })
}
