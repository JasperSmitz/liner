//! Fatal (run-aborting) errors. Per-file problems are NOT errors here: they are
//! collected as [`crate::model::FileError`] and reported, never aborting the run.
//!
//! Owner: architect (frozen interface); variants may be added by module owners.

use std::path::PathBuf;

use crate::config::ConfigError;

#[derive(Debug, thiserror::Error)]
pub enum LinerError {
    #[error("library path {0} does not exist")]
    RootNotFound(PathBuf),
    #[error("library path {0} is not a directory")]
    RootNotDirectory(PathBuf),
    #[error(
        "no library path given and none configured (pass PATH or set library_path in the config file)"
    )]
    NoLibraryPath,
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("refusing to write report to {path}: {reason}")]
    UnsafeOutput { path: PathBuf, reason: String },
    #[error("I/O error on {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write output")]
    Output(#[source] std::io::Error),
}

pub type Result<T, E = LinerError> = std::result::Result<T, E>;
