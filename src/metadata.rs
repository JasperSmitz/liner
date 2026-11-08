//! Stage 2: read tags and audio properties with lofty (read-only).
//!
//! Owner: developer A (M1). Public signatures are frozen; internals are free.
//!
//! Contract (see ADR 0001):
//! - Uses only lofty's read API (`Probe`/`read_from_path` with `ParseOptions`
//!   `read_cover_art(false)`); never any `save`/`remove` API (enforced by clippy.toml).
//! - Format is detected from content (lofty probe), falling back to extension.
//! - Field source, per field: the primary tag first, then the other rich tags
//!   (ID3v2/APE/Vorbis/MP4/RIFF/AIFF) in file order, and ID3v1 (Latin-1, 30-char
//!   fields) last.
//! - A parser panic (lofty has panicked on fuzzed input) is caught and reported as
//!   `FileErrorKind::Corrupt` ("parser panicked: ..."); it is not printed to stderr.
//! - A duration over one week is treated as unknown (`None`): it can only come
//!   from a corrupt header (e.g. an absurd Ogg granule position).
//! - Empty/whitespace-only text => `None`; numeric `0` => `None`.
//! - Any failure (I/O, unsupported, corrupt, zero-byte) => a `FileError` in
//!   `errors`, never a panic. Zero-byte files are `FileErrorKind::Corrupt`.
//! - `read_all` reads in parallel (rayon) and returns both vectors sorted by
//!   `rel_path`, so the output is independent of thread scheduling.

use std::cell::Cell;
use std::io::ErrorKind;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Once;
use std::time::Duration;

use lofty::config::ParseOptions;
use lofty::error::FileParseError;
use lofty::file::{AudioFile, FileType, TaggedFile, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::{ItemKey, Tag, TagType};
use rayon::prelude::*;

use crate::model::{AudioFormat, FileError, FileErrorKind, TagKind, TrackInfo};
use crate::scan::ScannedFile;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetadataResult {
    /// Sorted by `rel_path`.
    pub tracks: Vec<TrackInfo>,
    /// Sorted by `rel_path`.
    pub errors: Vec<FileError>,
}

/// Read one file. Must not panic on any input.
pub fn read_track(file: &ScannedFile) -> Result<TrackInfo, FileError> {
    let fail = |kind: FileErrorKind, message: String| FileError {
        path: file.path.clone(),
        rel_path: file.rel_path.clone(),
        kind,
        message,
    };

    // Stat first: a zero-byte file is corrupt regardless of what the probe makes of it.
    let meta = std::fs::metadata(&file.path).map_err(|e| fail(io_kind(&e), e.to_string()))?;
    if meta.len() == 0 {
        return Err(fail(
            FileErrorKind::Corrupt,
            "empty file (0 bytes)".to_owned(),
        ));
    }

    let tagged =
        guarded_probe_and_read(&file.path).map_err(|(kind, message)| fail(kind, message))?;
    let format = map_file_type(tagged.file_type()).ok_or_else(|| {
        fail(
            FileErrorKind::Unsupported,
            format!("unsupported format {:?}", tagged.file_type()),
        )
    })?;
    Ok(build_track(file, format, &tagged))
}

/// Read all files in parallel; see module docs for ordering guarantees.
pub fn read_all(files: &[ScannedFile]) -> MetadataResult {
    let (mut tracks, mut errors): (Vec<_>, Vec<_>) =
        files.par_iter().map(read_track).partition_map(|r| match r {
            Ok(track) => rayon::iter::Either::Left(track),
            Err(err) => rayon::iter::Either::Right(err),
        });
    tracks.sort_by(|a: &TrackInfo, b| (&a.rel_path, &a.path).cmp(&(&b.rel_path, &b.path)));
    errors.sort_by(|a: &FileError, b| {
        (&a.rel_path, &a.path, &a.message).cmp(&(&b.rel_path, &b.path, &b.message))
    });
    MetadataResult { tracks, errors }
}

/// Durations above this come from corrupt headers, not real audio.
const MAX_PLAUSIBLE_DURATION: Duration = Duration::from_secs(7 * 24 * 60 * 60);

thread_local! {
    /// Set while this thread runs a guarded lofty call; the panic hook stays quiet then.
    static IN_GUARDED_READ: Cell<bool> = const { Cell::new(false) };
}

/// Install (once per process) a panic hook that suppresses the default message and
/// backtrace for panics inside [`guarded_probe_and_read`] and delegates every other
/// panic to the previously installed hook. The flag is thread-local and the panic
/// unwinds on the thread that raised it, so this is correct under rayon.
fn install_quiet_panic_hook() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !IN_GUARDED_READ.with(Cell::get) {
                previous(info);
            }
        }));
    });
}

/// [`probe_and_read`] with any parser panic turned into a `Corrupt` error.
fn guarded_probe_and_read(path: &std::path::Path) -> Result<TaggedFile, (FileErrorKind, String)> {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            IN_GUARDED_READ.with(|f| f.set(false));
        }
    }

    install_quiet_panic_hook();
    IN_GUARDED_READ.with(|f| f.set(true));
    let _reset = Reset;
    catch_unwind(AssertUnwindSafe(|| probe_and_read(path))).unwrap_or_else(|payload| {
        let detail = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("unknown panic payload");
        Err((FileErrorKind::Corrupt, format!("parser panicked: {detail}")))
    })
}

/// Probe (content first, extension as fallback) and parse with cover art disabled.
/// Read-only: lofty's `Probe::open` opens the file with `read(true)` only.
fn probe_and_read(path: &std::path::Path) -> Result<TaggedFile, (FileErrorKind, String)> {
    let options = ParseOptions::new().read_cover_art(false);
    let probe = Probe::open(path)
        .map_err(|e| classify(&e))?
        .options(options)
        .guess_file_type()
        .map_err(|e| (io_kind(&e), e.to_string()))?;
    if probe.file_type().is_none() {
        return Err((
            FileErrorKind::Unsupported,
            "unrecognised file format".to_owned(),
        ));
    }
    probe.read().map_err(|e| classify(&e))
}

/// `unsupported` for unknown formats, `io` for genuine I/O failures, `corrupt` for
/// everything else (including unexpected EOF while parsing a truncated file).
fn classify(err: &FileParseError) -> (FileErrorKind, String) {
    let kind = if err.is_unknown_format() {
        FileErrorKind::Unsupported
    } else {
        match find_io_error(err) {
            Some(io) => io_kind(io),
            None => FileErrorKind::Corrupt,
        }
    };
    (kind, error_chain(err))
}

fn find_io_error<'a>(err: &'a (dyn std::error::Error + 'static)) -> Option<&'a std::io::Error> {
    let mut current = Some(err);
    while let Some(e) = current {
        if let Some(io) = e.downcast_ref::<std::io::Error>() {
            return Some(io);
        }
        current = e.source();
    }
    None
}

fn io_kind(err: &std::io::Error) -> FileErrorKind {
    match err.kind() {
        // Parsers surface malformed content as EOF, invalid data, or invalid seeks
        // (EINVAL when seeking before the start); none of these are device problems.
        ErrorKind::UnexpectedEof | ErrorKind::InvalidData | ErrorKind::InvalidInput => {
            FileErrorKind::Corrupt
        }
        _ => FileErrorKind::Io,
    }
}

/// `outer: inner: innermost`, skipping consecutive duplicates.
fn error_chain(err: &(dyn std::error::Error + 'static)) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut current = Some(err);
    while let Some(e) = current {
        let text = e.to_string();
        if parts.last() != Some(&text) {
            parts.push(text);
        }
        current = e.source();
    }
    parts.join(": ")
}

fn map_file_type(ft: FileType) -> Option<AudioFormat> {
    Some(match ft {
        FileType::Aac => AudioFormat::Aac,
        FileType::Aiff => AudioFormat::Aiff,
        FileType::Ape => AudioFormat::Ape,
        FileType::Flac => AudioFormat::Flac,
        FileType::Mpeg => AudioFormat::Mp3,
        FileType::Mp4 => AudioFormat::Mp4,
        FileType::Mpc => AudioFormat::Musepack,
        FileType::Opus => AudioFormat::Opus,
        FileType::Vorbis => AudioFormat::OggVorbis,
        FileType::Speex => AudioFormat::Speex,
        FileType::Wav => AudioFormat::Wav,
        FileType::WavPack => AudioFormat::WavPack,
        _ => return None,
    })
}

fn map_tag_type(tt: TagType) -> Option<TagKind> {
    Some(match tt {
        TagType::Ape => TagKind::Ape,
        TagType::Id3v1 => TagKind::Id3v1,
        TagType::Id3v2 => TagKind::Id3v2,
        TagType::Mp4Ilst => TagKind::Mp4Ilst,
        TagType::VorbisComments => TagKind::VorbisComments,
        TagType::RiffInfo => TagKind::RiffInfo,
        TagType::AiffText => TagKind::AiffText,
        _ => return None,
    })
}

/// Tags in lookup order: the primary tag first, then the other rich tags in file
/// order, then ID3v1 (lossy: Latin-1 and 30-character fields).
fn ordered_tags(tagged: &TaggedFile) -> Vec<&Tag> {
    let primary = tagged.primary_tag_type();
    let mut tags: Vec<&Tag> = tagged.tags().iter().collect();
    // Stable sort keeps file order within each rank.
    tags.sort_by_key(|t| (t.tag_type() != primary, t.tag_type() == TagType::Id3v1));
    tags
}

fn build_track(file: &ScannedFile, format: AudioFormat, tagged: &TaggedFile) -> TrackInfo {
    let tags = ordered_tags(tagged);
    let text = |key: ItemKey| first_text(&tags, key);

    let (track_no, track_total) = number_pair(&tags, ItemKey::TrackNumber, ItemKey::TrackTotal);
    let (disc_no, disc_total) = number_pair(&tags, ItemKey::DiscNumber, ItemKey::DiscTotal);

    // Prefer the first date field that yields a year; otherwise keep the first raw
    // date text so rules/reports can still show what is stored.
    let dates: Vec<String> = [
        ItemKey::RecordingDate,
        ItemKey::Year,
        ItemKey::ReleaseDate,
        ItemKey::OriginalReleaseDate,
    ]
    .into_iter()
    .filter_map(|key| first_text(&tags, key))
    .collect();
    let date = dates
        .iter()
        .find(|d| parse_year(d).is_some())
        .or(dates.first())
        .cloned();
    let year = date.as_deref().and_then(parse_year);

    let mut tag_types: Vec<TagKind> = tagged
        .tags()
        .iter()
        .filter_map(|t| map_tag_type(t.tag_type()))
        .collect();
    tag_types.sort();
    tag_types.dedup();

    let duration = tagged.properties().duration();
    let duration_ms = (duration <= MAX_PLAUSIBLE_DURATION)
        .then(|| u64::try_from(duration.as_millis()).ok())
        .flatten()
        .filter(|&ms| ms > 0);

    TrackInfo {
        title: text(ItemKey::TrackTitle),
        artist: text(ItemKey::TrackArtist),
        album: text(ItemKey::AlbumTitle),
        album_artist: text(ItemKey::AlbumArtist),
        track_no,
        track_total,
        disc_no,
        disc_total,
        year,
        date,
        genre: text(ItemKey::Genre),
        compilation: tags
            .iter()
            .filter_map(|t| t.get_string(ItemKey::FlagCompilation))
            .find_map(parse_flag)
            .unwrap_or(false),
        duration_ms,
        tag_types,
        ..TrackInfo::new(file.path.clone(), file.rel_path.clone(), format)
    }
}

/// First non-blank value for `key` across `tags`, original text preserved.
fn first_text(tags: &[&Tag], key: ItemKey) -> Option<String> {
    tags.iter()
        .filter_map(|t| t.get_string(key))
        .find(|v| !v.trim().is_empty())
        .map(str::to_owned)
}

/// Number and total for track or disc. Accepts `3`, ` 03 `, and `3/12` in the
/// number field. The number is the first valid one across tags; the total is the
/// first separate total field, else the first slash total from any number field
/// (independent of which tag supplied the number, so `0/12` keeps its 12).
/// `0` and unparsable values are `None`.
fn number_pair(tags: &[&Tag], num_key: ItemKey, total_key: ItemKey) -> (Option<u32>, Option<u32>) {
    let mut number = None;
    let mut slash_total = None;
    for value in tags.iter().filter_map(|t| t.get_string(num_key)) {
        let (n, t) = split_number(value);
        number = number.or(n);
        slash_total = slash_total.or(t);
        if number.is_some() && slash_total.is_some() {
            break;
        }
    }
    let total = tags
        .iter()
        .filter_map(|t| t.get_string(total_key))
        .find_map(parse_number)
        .or(slash_total);
    (number, total)
}

fn split_number(value: &str) -> (Option<u32>, Option<u32>) {
    match value.split_once('/') {
        Some((n, t)) => (parse_number(n), parse_number(t)),
        None => (parse_number(value), None),
    }
}

fn parse_number(value: &str) -> Option<u32> {
    value.trim().parse::<u32>().ok().filter(|&n| n > 0)
}

/// Year from the start of a date string (`2003`, `2003-05-12`, `2003-05-12T10:00`).
/// Needs exactly four leading digits; `0000` is `None`.
fn parse_year(date: &str) -> Option<u32> {
    let date = date.trim();
    let digits = date.bytes().take_while(u8::is_ascii_digit).count();
    if digits != 4 {
        return None;
    }
    date[..4].parse::<u32>().ok().filter(|&y| y > 0)
}

/// Compilation flag value: `1`/`true`/`yes` (any case) or any non-zero integer.
fn parse_flag(value: &str) -> Option<bool> {
    let v = value.trim();
    if v.is_empty() {
        return None;
    }
    if let Ok(n) = v.parse::<i64>() {
        return Some(n != 0);
    }
    match v.to_ascii_lowercase().as_str() {
        "true" | "yes" => Some(true),
        "false" | "no" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_parse_leniently_and_zero_is_absent() {
        assert_eq!(split_number("3"), (Some(3), None));
        assert_eq!(split_number(" 03 "), (Some(3), None));
        assert_eq!(split_number("3/12"), (Some(3), Some(12)));
        assert_eq!(split_number("0/12"), (None, Some(12)));
        assert_eq!(split_number("3/0"), (Some(3), None));
        assert_eq!(split_number("A"), (None, None));
        assert_eq!(split_number(""), (None, None));
        assert_eq!(split_number("-1"), (None, None));
    }

    #[test]
    fn years_come_from_leading_four_digits() {
        assert_eq!(parse_year("2003"), Some(2003));
        assert_eq!(parse_year(" 2003-05-12 "), Some(2003));
        assert_eq!(parse_year("2003-05-12T10:00:00"), Some(2003));
        assert_eq!(parse_year("0000"), None);
        assert_eq!(parse_year("03"), None);
        assert_eq!(parse_year("20031"), None);
        assert_eq!(parse_year("unknown"), None);
        assert_eq!(parse_year(""), None);
    }

    #[test]
    fn compilation_flag_values() {
        assert_eq!(parse_flag("1"), Some(true));
        assert_eq!(parse_flag(" TRUE "), Some(true));
        assert_eq!(parse_flag("yes"), Some(true));
        assert_eq!(parse_flag("0"), Some(false));
        assert_eq!(parse_flag("false"), Some(false));
        assert_eq!(parse_flag(""), None);
        assert_eq!(parse_flag("maybe"), None);
    }

    #[test]
    fn slash_total_survives_when_number_comes_from_a_later_tag() {
        let mut primary = Tag::new(TagType::VorbisComments);
        primary.insert_text(ItemKey::TrackNumber, "0/12".to_owned());
        let mut v1 = Tag::new(TagType::Id3v1);
        v1.insert_text(ItemKey::TrackNumber, "5".to_owned());
        let tags = [&primary, &v1];
        assert_eq!(
            number_pair(&tags, ItemKey::TrackNumber, ItemKey::TrackTotal),
            (Some(5), Some(12))
        );

        // A separate total field still wins over a slash total.
        primary.insert_text(ItemKey::TrackTotal, "10".to_owned());
        let tags = [&primary, &v1];
        assert_eq!(
            number_pair(&tags, ItemKey::TrackNumber, ItemKey::TrackTotal),
            (Some(5), Some(10))
        );
    }

    #[test]
    fn io_errors_classified() {
        let eof = std::io::Error::new(ErrorKind::UnexpectedEof, "eof");
        assert_eq!(io_kind(&eof), FileErrorKind::Corrupt);
        let seek = std::io::Error::from(ErrorKind::InvalidInput);
        assert_eq!(io_kind(&seek), FileErrorKind::Corrupt);
        let denied = std::io::Error::new(ErrorKind::PermissionDenied, "denied");
        assert_eq!(io_kind(&denied), FileErrorKind::Io);
    }
}
