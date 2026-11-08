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
//! - [`OutputTarget`] is the ONLY place in the crate that creates a file: the
//!   user-requested `--output` report, written to a temporary file in the validated
//!   output directory and renamed into place (raw `libc` `openat`/`renameat`/
//!   `unlinkat` relative to the directory handle; the std write APIs and libc
//!   rename/unlink stay banned elsewhere by clippy `disallowed-methods`). [`check_output_path`] validates an `--output`
//!   path up front and creates nothing.
//! - Human output escapes control characters in every tag- or path-derived string
//!   ([`escape_control`]), so file names and tags cannot inject terminal escapes or
//!   fake report lines.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::ffi::{CStr, CString, OsStr};
use std::io::{self, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

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
            let path = escape_control(&e.path);
            writeln!(out, "  {}{pad}{path}", style.paint(RED, &kind))?;
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

/// Replace every control character (`char::is_control`: C0, DEL, C1) with a
/// visible `\u{XX}` escape, so untrusted tag values and file names cannot emit
/// terminal escape sequences or break a line. Borrows when there is nothing to do.
pub fn escape_control(text: &str) -> Cow<'_, str> {
    if !text.chars().any(char::is_control) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        if c.is_control() {
            out.push_str(&format!("\\u{{{:x}}}", u32::from(c)));
        } else {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

/// `Artist/Album  "Title" by Artist  (2 discs, 20 tracks)`; pieces that are unknown
/// are left out.
fn album_header(id: &str, album: Option<&AlbumEntry>) -> String {
    let mut header = escape_control(id).into_owned();
    let Some(album) = album else {
        return header;
    };
    let mut desc = Vec::new();
    if let Some(title) = &album.title {
        desc.push(format!("\"{}\"", escape_control(title)));
    }
    if let Some(artist) = &album.album_artist {
        desc.push(format!("by {}", escape_control(artist)));
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
    let code = escape_control(&issue.code);
    writeln!(out, "  {}{code}", style.severity(issue.severity))?;
    write_wrapped(out, &issue.message)?;
    let indent = " ".repeat(INDENT);
    for path in issue.paths.iter().take(MAX_PATHS) {
        let shown = path.strip_prefix(strip).unwrap_or(path);
        writeln!(out, "{indent}- {}", escape_control(shown))?;
    }
    if issue.paths.len() > MAX_PATHS {
        let more = format!("... and {} more", issue.paths.len() - MAX_PATHS);
        writeln!(out, "{indent}{}", style.paint(DIM, &more))?;
    }
    Ok(())
}

/// Write `text` word-wrapped at [`WIDTH`], every line indented by [`INDENT`].
/// Control characters are escaped first ([`escape_control`]).
fn write_wrapped(out: &mut dyn Write, text: &str) -> io::Result<()> {
    let indent = " ".repeat(INDENT);
    for line in wrap(&escape_control(text), WIDTH - INDENT) {
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

/// A validated `--output` destination: the opened parent directory plus the file
/// name inside it. Produced by [`check_output_path`] before the scan, consumed by
/// [`OutputTarget::write`] after the report has been rendered.
///
/// Every later operation works relative to the directory *handle*, so renaming or
/// replacing the parent path (e.g. by a symlink into the library) after the check
/// cannot redirect the write.
#[derive(Debug)]
pub struct OutputTarget {
    /// The `--output` path as given (for messages).
    path: PathBuf,
    /// `O_DIRECTORY` handle of the output's parent directory.
    dir: OwnedFd,
    /// Final path component.
    name: CString,
    /// `(st_dev, st_ino)` of the library root, if it could be stat'ed.
    library: Option<(u64, u64)>,
}

/// Validate an `--output` path, before the (slow) scan so a mistake is reported at
/// once. Creates nothing. Refuses, with [`LinerError::UnsafeOutput`]:
/// - a name with an audio extension (`scan::is_audio_extension`, any case);
/// - a path without a file name (`dir/..`, `/`);
/// - an existing path that is not a regular file (directory, symlink, FIFO, ...);
/// - a parent directory that is the library root or lies anywhere below it. This is
///   decided by walking up from the *opened* parent directory (`..` relative to the
///   handle) to the filesystem root and comparing `(st_dev, st_ino)` with the
///   library root's, so symlinked parents and bind-mount aliases are caught too.
///
/// A missing parent directory is an [`LinerError::Io`] error. If `library_root`
/// cannot be stat'ed the library check is skipped; the scan then fails on the root
/// anyway.
pub fn check_output_path(path: &Path, library_root: &Path) -> Result<OutputTarget> {
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
    let name = match path.file_name() {
        // `dir/` names a directory, even if it does not exist yet.
        _ if path.as_os_str().as_bytes().ends_with(b"/") => {
            return Err(unsafe_output(path, "output path has no file name"));
        }
        Some(n) => CString::new(n.as_bytes())
            .map_err(|_| unsafe_output(path, "output file name contains a NUL byte"))?,
        None => return Err(unsafe_output(path, "output path has no file name")),
    };
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let io_err = |source| LinerError::Io {
        path: parent.to_path_buf(),
        source,
    };
    let dir = open_dir(parent).map_err(|e| {
        if e.raw_os_error() == Some(libc::ENOTDIR) {
            unsafe_output(path, "output directory is not a directory")
        } else {
            io_err(e)
        }
    })?;
    let library = std::fs::metadata(library_root)
        .ok()
        .map(|m| (m.dev(), m.ino()));
    let target = OutputTarget {
        path: path.to_path_buf(),
        dir,
        name,
        library,
    };
    target.check_outside_library()?;
    Ok(target)
}

impl OutputTarget {
    /// Refuse if the output directory is the library root or below it.
    fn check_outside_library(&self) -> Result<()> {
        let Some(library) = self.library else {
            return Ok(());
        };
        let io_err = |source| LinerError::Io {
            path: self.path.clone(),
            source,
        };
        let mut cur = dup_dir(&self.dir).map_err(io_err)?;
        let mut id = fstat_id(&cur).map_err(io_err)?;
        loop {
            if id == library {
                return Err(unsafe_output(
                    &self.path,
                    "output file would be inside the library being scanned (liner never writes there)",
                ));
            }
            let parent = open_parent(&cur).map_err(io_err)?;
            let parent_id = fstat_id(&parent).map_err(io_err)?;
            if parent_id == id {
                return Ok(()); // `..` of the filesystem root is itself
            }
            cur = parent;
            id = parent_id;
        }
    }

    /// Write `bytes` as the report, atomically: a new temporary file
    /// `.<name>.liner-tmp-<pid>` is created in the output directory (exclusive,
    /// no symlink following, mode 0644), written, fsync'ed and renamed over
    /// `<name>`. The destination, if present, must still be a regular file with no
    /// other hard links. On any failure the temporary file is removed and an
    /// existing report is left as it was.
    ///
    /// The library check is repeated first, in case the output directory was moved
    /// into the library during the scan.
    pub fn write(&self, bytes: &[u8]) -> Result<()> {
        self.write_with(bytes, |file, bytes| file.write_all(bytes))
    }

    /// [`Self::write`] with an injectable writer, so tests can simulate a failing
    /// write.
    #[doc(hidden)]
    // The sanctioned exemption: renameat of our temp file over the report, and
    // unlinkat of the temp file on failure (both banned elsewhere by clippy.toml).
    #[allow(clippy::disallowed_methods)]
    pub fn write_with(
        &self,
        bytes: &[u8],
        write: impl FnOnce(&mut std::fs::File, &[u8]) -> io::Result<()>,
    ) -> Result<()> {
        self.check_outside_library()?;
        let io_err = |source| LinerError::Io {
            path: self.path.clone(),
            source,
        };
        let mut tmp_name = b".".to_vec();
        tmp_name.extend_from_slice(self.name.as_bytes());
        tmp_name.extend_from_slice(format!(".liner-tmp-{}", std::process::id()).as_bytes());
        let tmp = CString::new(tmp_name).expect("no NUL: built from a CString");

        let mut file = std::fs::File::from(self.create_temp(&tmp).map_err(io_err)?);
        let result = (|| {
            write(&mut file, bytes)
                .and_then(|()| file.sync_all())
                .map_err(LinerError::Output)?;
            drop(file);
            self.check_destination()?;
            // SAFETY: valid fd and NUL-terminated names.
            let rc = unsafe {
                libc::renameat(
                    self.dir.as_raw_fd(),
                    tmp.as_ptr(),
                    self.dir.as_raw_fd(),
                    self.name.as_ptr(),
                )
            };
            if rc != 0 {
                return Err(io_err(io::Error::last_os_error()));
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                // Make the rename durable; the report is already in place, so a
                // failure here is not worth failing the run for.
                // SAFETY: valid fd.
                unsafe { libc::fsync(self.dir.as_raw_fd()) };
                Ok(())
            }
            Err(e) => {
                // SAFETY: valid fd and NUL-terminated name.
                unsafe { libc::unlinkat(self.dir.as_raw_fd(), tmp.as_ptr(), 0) };
                Err(e)
            }
        }
    }

    /// The single sanctioned file creation in the crate: the temporary report file,
    /// created exclusively inside the validated output directory.
    fn create_temp(&self, tmp: &CStr) -> io::Result<OwnedFd> {
        // SAFETY: valid fd and NUL-terminated name; the returned fd is owned below.
        let fd = unsafe {
            libc::openat(
                self.dir.as_raw_fd(),
                tmp.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o644 as libc::c_uint,
            )
        };
        owned(fd)
    }

    /// The destination may be absent; if present it must be a regular file (not a
    /// symlink) with no other hard links.
    fn check_destination(&self) -> Result<()> {
        // SAFETY: zeroed `stat` is a valid out-parameter.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: valid fd, NUL-terminated name, valid out-pointer.
        let rc = unsafe {
            libc::fstatat(
                self.dir.as_raw_fd(),
                self.name.as_ptr(),
                &mut st,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if rc != 0 {
            let e = io::Error::last_os_error();
            if e.raw_os_error() == Some(libc::ENOENT) {
                return Ok(());
            }
            return Err(LinerError::Io {
                path: self.path.clone(),
                source: e,
            });
        }
        if st.st_mode & libc::S_IFMT != libc::S_IFREG {
            return Err(unsafe_output(
                &self.path,
                "output path exists and is not a regular file",
            ));
        }
        if st.st_nlink > 1 {
            return Err(unsafe_output(
                &self.path,
                "output file has other hard links (it could be an audio file)",
            ));
        }
        Ok(())
    }
}

fn owned(fd: libc::c_int) -> io::Result<OwnedFd> {
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        // SAFETY: `fd` was just returned by the kernel and is owned by nobody else.
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    }
}

/// Open a directory (following symlinks in `path`) for use as an `*at` base.
fn open_dir(path: &Path) -> io::Result<OwnedFd> {
    let c = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    // SAFETY: NUL-terminated path; the returned fd is owned below.
    owned(unsafe {
        libc::open(
            c.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    })
}

/// A path-only handle on the same directory (for walking up without read access).
fn dup_dir(dir: &OwnedFd) -> io::Result<OwnedFd> {
    // SAFETY: valid fd and static NUL-terminated name.
    owned(unsafe {
        libc::openat(
            dir.as_raw_fd(),
            c".".as_ptr(),
            libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    })
}

/// `..` relative to `dir`.
fn open_parent(dir: &OwnedFd) -> io::Result<OwnedFd> {
    // SAFETY: valid fd and static NUL-terminated name.
    owned(unsafe {
        libc::openat(
            dir.as_raw_fd(),
            c"..".as_ptr(),
            libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    })
}

fn fstat_id(fd: &OwnedFd) -> io::Result<(u64, u64)> {
    // SAFETY: zeroed `stat` is a valid out-parameter.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: valid fd and out-pointer.
    if unsafe { libc::fstat(fd.as_raw_fd(), &mut st) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((st.st_dev, st.st_ino))
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

    #[test]
    fn control_characters_are_escaped() {
        assert!(matches!(escape_control("plain é ✓"), Cow::Borrowed(_)));
        assert_eq!(
            escape_control("a\x1b[2Jb\nc\td\u{7f}e\u{9b}"),
            "a\\u{1b}[2Jb\\u{a}c\\u{9}d\\u{7f}e\\u{9b}"
        );
    }
}
