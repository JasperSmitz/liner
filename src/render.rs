//! Stage 6: render a [`Report`] as human-readable text or JSON.
//!
//! Owner: developer C (M3). Public signatures are frozen; internals are free.
//!
//! Contract:
//! - JSON: pretty-printed `Report` + trailing newline. Nothing else on the stream.
//! - Human: plain text (no colour, no terminal detection), deterministic, grouped by
//!   album; ends with the summary. Wording is not a contract.
//! - [`create_output_file`] is the ONLY place in the crate allowed to create a file
//!   (clippy `disallowed-methods` enforces this).

use std::io::{self, Write};
use std::path::Path;

use crate::error::{LinerError, Result};
use crate::report::Report;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputFormat {
    #[default]
    Human,
    Json,
}

pub fn render(report: &Report, format: OutputFormat, out: &mut dyn Write) -> io::Result<()> {
    match format {
        OutputFormat::Human => render_human(report, out),
        OutputFormat::Json => render_json(report, out),
    }
}

pub fn render_json(report: &Report, out: &mut dyn Write) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut *out, report)?;
    writeln!(out)
}

pub fn render_human(report: &Report, out: &mut dyn Write) -> io::Result<()> {
    // M3 placeholder: full album-grouped listing to be implemented by developer C.
    let s = &report.summary;
    writeln!(
        out,
        "{} files scanned, {} audio, {} unreadable, {} albums; {} errors, {} warnings, {} info",
        s.files_scanned,
        s.audio_files,
        s.unreadable_files,
        s.albums,
        s.issues_by_severity.error,
        s.issues_by_severity.warning,
        s.issues_by_severity.info
    )
}

/// Create (truncate) the `--output` report file.
///
/// Refuses, with [`LinerError::UnsafeOutput`], any path whose extension is an audio
/// extension (`scan::is_audio_extension`) or that exists and is not a regular file,
/// so a mistyped `--output` can never clobber music.
pub fn create_output_file(path: &Path) -> Result<std::fs::File> {
    let unsafe_output = |reason: &str| LinerError::UnsafeOutput {
        path: path.to_path_buf(),
        reason: reason.to_owned(),
    };
    if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(crate::scan::is_audio_extension)
    {
        return Err(unsafe_output("output file has an audio file extension"));
    }
    if let Ok(meta) = std::fs::symlink_metadata(path)
        && !meta.file_type().is_file()
    {
        return Err(unsafe_output(
            "output path exists and is not a regular file",
        ));
    }
    #[allow(clippy::disallowed_methods)] // the single sanctioned write: the report file
    std::fs::File::create(path).map_err(|source| LinerError::Io {
        path: path.to_path_buf(),
        source,
    })
}
