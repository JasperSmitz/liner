//! Stage 6: render a [`Report`] as human-readable text or JSON.
//!
//! Owner: developer C (M3). Public signatures are frozen; internals are free.
//!
//! Contract:
//! - JSON: pretty-printed `Report` + trailing newline. Nothing else on the stream.
//! - Human: deterministic text, grouped by album (only albums with issues), then
//!   library issues, then unreadable files, then a summary line. Wording is not a
//!   contract. Colour (ANSI) only when [`HumanOptions::color`] is set; the decision
//!   (`--color`, TTY, `NO_COLOR`) is made by the caller via [`use_color`], so
//!   rendering itself does no terminal detection.
//! - [`create_output_file`] is the ONLY place in the crate allowed to create a file
//!   (clippy `disallowed-methods` enforces this). [`check_output_path`] validates an
//!   `--output` path up front without touching the disk.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::io::{self, Write};
use std::path::Path;

use crate::error::{LinerError, Result};
use crate::model::{Issue, Severity};
use crate::report::{AlbumEntry, Report};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputFormat {
    #[default]
    Human,
    Json,
}

/// `--color` choice.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum ColorChoice {
    /// Colour when writing to a terminal and `NO_COLOR` is unset or empty.
    #[default]
    Auto,
    Always,
    Never,
}

/// Decide whether to emit ANSI colour. `no_color` is the value of `NO_COLOR`
/// (<https://no-color.org>: any non-empty value disables colour in auto mode).
pub fn use_color(choice: ColorChoice, is_terminal: bool, no_color: Option<&OsStr>) -> bool {
    match choice {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        ColorChoice::Auto => is_terminal && no_color.is_none_or(OsStr::is_empty),
    }
}

/// Options for the human renderer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HumanOptions {
    /// Emit ANSI colour escapes.
    pub color: bool,
}

/// Render without colour.
pub fn render(report: &Report, format: OutputFormat, out: &mut dyn Write) -> io::Result<()> {
    render_with(report, format, HumanOptions::default(), out)
}

/// Render; `human` only affects [`OutputFormat::Human`].
pub fn render_with(
    report: &Report,
    format: OutputFormat,
    human: HumanOptions,
    out: &mut dyn Write,
) -> io::Result<()> {
    match format {
        OutputFormat::Human => render_human_with(report, human, out),
        OutputFormat::Json => render_json(report, out),
    }
}

pub fn render_json(report: &Report, out: &mut dyn Write) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut *out, report)?;
    writeln!(out)
}

/// Human output without colour.
pub fn render_human(report: &Report, out: &mut dyn Write) -> io::Result<()> {
    render_human_with(report, HumanOptions::default(), out)
}

/// Target line width for wrapped text.
const WIDTH: usize = 80;
/// Column where issue messages and paths start.
const INDENT: usize = 12;
/// Paths listed per issue before eliding the rest.
const MAX_PATHS: usize = 8;

const BOLD: &str = "\x1b[1m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

struct Style {
    color: bool,
}

impl Style {
    fn paint(&self, code: &str, text: &str) -> String {
        if self.color {
            format!("{code}{text}{RESET}")
        } else {
            text.to_owned()
        }
    }

    fn severity(&self, severity: Severity) -> String {
        let code = match severity {
            Severity::Error => RED,
            Severity::Warning => YELLOW,
            Severity::Info => CYAN,
        };
        // Pad outside the escapes so columns line up with and without colour.
        let label = self.paint(code, severity.as_str());
        let pad = " ".repeat(INDENT - 2 - severity.as_str().len());
        format!("{label}{pad}")
    }
}

/// Human output: album sections, library issues, unreadable files, summary.
pub fn render_human_with(
    report: &Report,
    options: HumanOptions,
    out: &mut dyn Write,
) -> io::Result<()> {
    let style = Style {
        color: options.color,
    };

    // Album/file issues grouped by album id (BTreeMap: sorted by id); within a group
    // the report's issue order is kept. Library issues have no album id.
    let mut by_album: BTreeMap<&str, Vec<&Issue>> = BTreeMap::new();
    let mut library: Vec<&Issue> = Vec::new();
    for issue in &report.issues {
        match issue.album_id.as_deref() {
            Some(id) => by_album.entry(id).or_default().push(issue),
            None => library.push(issue),
        }
    }

    let mut first_section = true;
    let mut section_break = |out: &mut dyn Write| -> io::Result<()> {
        if !std::mem::take(&mut first_section) {
            writeln!(out)?;
        }
        Ok(())
    };

    for (id, issues) in &by_album {
        section_break(out)?;
        let album = report.albums.iter().find(|a| a.id == *id);
        writeln!(out, "{}", style.paint(BOLD, &album_header(id, album)))?;
        let prefix = if *id == "." {
            String::new()
        } else {
            format!("{id}/")
        };
        for issue in issues {
            write_issue(out, &style, issue, &prefix)?;
        }
    }

    if !library.is_empty() {
        section_break(out)?;
        writeln!(out, "{}", style.paint(BOLD, "Library"))?;
        for issue in &library {
            write_issue(out, &style, issue, "")?;
        }
    }

    if !report.errors.is_empty() {
        section_break(out)?;
        let heading = format!("Unreadable files ({})", report.errors.len());
        writeln!(out, "{}", style.paint(BOLD, &heading))?;
        for e in &report.errors {
            let kind = serde_json::to_value(e.kind)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default();
            let pad = " ".repeat((INDENT - 2).saturating_sub(kind.len()).max(1));
            writeln!(out, "  {}{pad}{}", style.paint(RED, &kind), e.path)?;
            write_wrapped(out, &e.message)?;
        }
    }

    section_break(out)?;
    let s = &report.summary;
    let min = report.options.min_severity.as_str();
    if report.issues.is_empty() && report.errors.is_empty() {
        match s.issues_hidden {
            0 => writeln!(out, "No issues found.")?,
            1 => writeln!(
                out,
                "No issues at or above {min} (1 lower-severity issue hidden)."
            )?,
            n => writeln!(
                out,
                "No issues at or above {min} ({n} lower-severity issues hidden)."
            )?,
        }
    }
    let hidden = match s.issues_hidden {
        0 => String::new(),
        n => format!(" ({n} below {min} hidden)"),
    };
    let summary = format!(
        "{} files scanned, {} audio, {} unreadable, {} albums; {} errors, {} warnings, {} info{hidden}",
        s.files_scanned,
        s.audio_files,
        s.unreadable_files,
        s.albums,
        s.issues_by_severity.error,
        s.issues_by_severity.warning,
        s.issues_by_severity.info
    );
    writeln!(out, "{}", style.paint(BOLD, &summary))
}

/// `Artist/Album  "Title" by Artist  (2 discs, 20 tracks)`; pieces that are unknown
/// are left out.
fn album_header(id: &str, album: Option<&AlbumEntry>) -> String {
    let mut header = id.to_owned();
    let Some(album) = album else {
        return header;
    };
    let mut desc = Vec::new();
    if let Some(title) = &album.title {
        desc.push(format!("\"{title}\""));
    }
    if let Some(artist) = &album.album_artist {
        desc.push(format!("by {artist}"));
    }
    if !desc.is_empty() {
        header.push_str("  ");
        header.push_str(&desc.join(" "));
    }
    let plural = |n: u32, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
    header.push_str(&format!(
        "  ({}, {})",
        plural(album.disc_count, "disc"),
        plural(album.track_count, "track")
    ));
    header
}

/// ```text
///   warning   track-number-gap
///             Disc 1 is missing track 4
///             - 01.flac
/// ```
/// Paths are shown relative to the album (`strip`) when they are inside it.
fn write_issue(out: &mut dyn Write, style: &Style, issue: &Issue, strip: &str) -> io::Result<()> {
    writeln!(out, "  {}{}", style.severity(issue.severity), issue.code)?;
    write_wrapped(out, &issue.message)?;
    let indent = " ".repeat(INDENT);
    for path in issue.paths.iter().take(MAX_PATHS) {
        let shown = path.strip_prefix(strip).unwrap_or(path);
        writeln!(out, "{indent}- {shown}")?;
    }
    if issue.paths.len() > MAX_PATHS {
        let more = format!("... and {} more", issue.paths.len() - MAX_PATHS);
        writeln!(out, "{indent}{}", style.paint(DIM, &more))?;
    }
    Ok(())
}

/// Write `text` word-wrapped at [`WIDTH`], every line indented by [`INDENT`].
fn write_wrapped(out: &mut dyn Write, text: &str) -> io::Result<()> {
    let indent = " ".repeat(INDENT);
    for line in wrap(text, WIDTH - INDENT) {
        writeln!(out, "{indent}{line}")?;
    }
    Ok(())
}

/// Greedy word wrap on whitespace; words longer than `width` stay whole.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let needed = if line.is_empty() {
            word.chars().count()
        } else {
            line.chars().count() + 1 + word.chars().count()
        };
        if needed > width && !line.is_empty() {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Cheap checks of an `--output` path, done before the (slow) scan so a mistake is
/// reported at once. Touches nothing on disk. Refuses, with
/// [`LinerError::UnsafeOutput`]:
/// - a name with an audio extension (`scan::is_audio_extension`, any case);
/// - an existing path that is not a regular file (directory, symlink, FIFO, ...);
/// - a path whose (canonicalised) directory lies inside the (canonicalised) library
///   root, since liner never creates or changes files in the library.
///
/// A missing parent directory is an [`LinerError::Io`] error. If `library_root`
/// cannot be canonicalised the library check is skipped; the scan then fails on
/// the root anyway.
pub fn check_output_path(path: &Path, library_root: &Path) -> Result<()> {
    if has_audio_extension(path) {
        return Err(unsafe_output(
            path,
            "output file has an audio file extension",
        ));
    }
    if let Ok(meta) = std::fs::symlink_metadata(path)
        && !meta.file_type().is_file()
    {
        return Err(unsafe_output(
            path,
            "output path exists and is not a regular file",
        ));
    }
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let parent = std::fs::canonicalize(parent).map_err(|source| LinerError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    if !parent.is_dir() {
        return Err(unsafe_output(path, "output directory is not a directory"));
    }
    if let Ok(root) = std::fs::canonicalize(library_root)
        && parent.starts_with(&root)
    {
        return Err(unsafe_output(
            path,
            "output file would be inside the library being scanned (liner never writes there)",
        ));
    }
    Ok(())
}

/// Open the `--output` report file for writing, emptied. Call it only once the
/// report has been fully rendered, so a failed run leaves an existing file alone.
///
/// Race-free against path swaps: the path is opened without following symlinks
/// and without truncating, the *opened handle* is checked (regular file, no other
/// hard links: truncating a hard link to an audio file would destroy it), and only
/// then is the file emptied. Audio extensions are refused as in
/// [`check_output_path`].
pub fn create_output_file(path: &Path) -> Result<std::fs::File> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    if has_audio_extension(path) {
        return Err(unsafe_output(
            path,
            "output file has an audio file extension",
        ));
    }
    let io_err = |source| LinerError::Io {
        path: path.to_path_buf(),
        source,
    };
    // The single sanctioned write in the crate: the user-requested report file.
    // O_NONBLOCK: never hang opening a FIFO. O_NOFOLLOW: never write through a symlink.
    #[allow(clippy::disallowed_methods)]
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .mode(0o644)
        .open(path);
    let file = match file {
        Ok(f) => f,
        Err(e) if e.raw_os_error() == Some(libc::ELOOP) => {
            return Err(unsafe_output(path, "output path is a symbolic link"));
        }
        Err(e) => return Err(io_err(e)),
    };
    let meta = file.metadata().map_err(io_err)?;
    if !meta.file_type().is_file() {
        return Err(unsafe_output(
            path,
            "output path exists and is not a regular file",
        ));
    }
    if meta.nlink() > 1 {
        return Err(unsafe_output(
            path,
            "output file has other hard links (it could be an audio file)",
        ));
    }
    file.set_len(0).map_err(io_err)?;
    Ok(file)
}

fn has_audio_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(crate::scan::is_audio_extension)
}

fn unsafe_output(path: &Path, reason: &str) -> LinerError {
    LinerError::UnsafeOutput {
        path: path.to_path_buf(),
        reason: reason.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_decision() {
        let set = Some(OsStr::new("1"));
        let empty = Some(OsStr::new(""));
        assert!(use_color(ColorChoice::Auto, true, None));
        assert!(use_color(ColorChoice::Auto, true, empty));
        assert!(!use_color(ColorChoice::Auto, true, set));
        assert!(!use_color(ColorChoice::Auto, false, None));
        assert!(use_color(ColorChoice::Always, false, set));
        assert!(!use_color(ColorChoice::Never, true, None));
    }

    #[test]
    fn wrap_respects_width() {
        let text = "one two three four five six seven eight nine ten";
        let lines = wrap(text, 14);
        assert!(lines.iter().all(|l| l.chars().count() <= 14), "{lines:?}");
        assert_eq!(lines.join(" "), text);
        assert_eq!(
            wrap("averyveryverylongword x", 5),
            ["averyveryverylongword", "x"]
        );
        assert!(wrap("   ", 10).is_empty());
    }
}
