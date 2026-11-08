//! Shared integration-test fixture builder.
//!
//! Owner: developer A (M1). Signatures are frozen; other developers' tests use them.
//!
//! Builds a synthetic library in a `TempDir` by copying the committed silent
//! templates (`tests/fixtures/templates/silence.*`) and writing tags onto the COPIES
//! with lofty. This is the only code in the repository that writes tags, and it only
//! ever touches files inside its own temp directory (see ADR 0004).
#![allow(dead_code)] // each test crate uses a different subset
#![allow(clippy::disallowed_methods)] // writing tags/files into the temp library is the point

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use liner::scan::ScannedFile;
use lofty::config::WriteOptions;
use lofty::file::TaggedFileExt;
use lofty::tag::{Accessor, ItemKey, Tag, TagExt, TagType};
use tempfile::TempDir;

/// Template formats available under `tests/fixtures/templates/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fixture {
    Flac,
    Mp3,
    OggVorbis,
    Opus,
    M4a,
    Wav,
}

impl Fixture {
    pub const ALL: [Fixture; 6] = [
        Fixture::Flac,
        Fixture::Mp3,
        Fixture::OggVorbis,
        Fixture::Opus,
        Fixture::M4a,
        Fixture::Wav,
    ];

    /// File extension of the template (and default extension of copies).
    pub fn extension(self) -> &'static str {
        match self {
            Fixture::Flac => "flac",
            Fixture::Mp3 => "mp3",
            Fixture::OggVorbis => "ogg",
            Fixture::Opus => "opus",
            Fixture::M4a => "m4a",
            Fixture::Wav => "wav",
        }
    }

    pub fn template_path(self) -> PathBuf {
        fixtures_dir()
            .join("templates")
            .join(format!("silence.{}", self.extension()))
    }
}

/// Committed corrupt fixtures under `tests/fixtures/corrupt/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corrupt {
    /// Zero bytes.
    Empty,
    /// FLAC magic, cut inside STREAMINFO.
    Truncated,
    /// Pseudo-random bytes.
    Garbage,
}

impl Corrupt {
    pub const ALL: [Corrupt; 3] = [Corrupt::Empty, Corrupt::Truncated, Corrupt::Garbage];

    pub fn path(self) -> PathBuf {
        let name = match self {
            Corrupt::Empty => "empty.flac",
            Corrupt::Truncated => "truncated.flac",
            Corrupt::Garbage => "garbage.flac",
        };
        fixtures_dir().join("corrupt").join(name)
    }
}

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Tags to write. `None` = leave the field absent. Builder-style setters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track: Option<u32>,
    pub track_total: Option<u32>,
    pub disc: Option<u32>,
    pub disc_total: Option<u32>,
    pub year: Option<u32>,
    pub genre: Option<String>,
    pub compilation: bool,
}

impl Tags {
    pub fn new() -> Self {
        Self::default()
    }
    /// Common case: album + artist + track number.
    pub fn album_track(album: &str, artist: &str, track: u32) -> Self {
        Self::new().album(album).artist(artist).track(track)
    }
    pub fn title(mut self, v: &str) -> Self {
        self.title = Some(v.to_owned());
        self
    }
    pub fn artist(mut self, v: &str) -> Self {
        self.artist = Some(v.to_owned());
        self
    }
    pub fn album(mut self, v: &str) -> Self {
        self.album = Some(v.to_owned());
        self
    }
    pub fn album_artist(mut self, v: &str) -> Self {
        self.album_artist = Some(v.to_owned());
        self
    }
    pub fn track(mut self, v: u32) -> Self {
        self.track = Some(v);
        self
    }
    pub fn track_total(mut self, v: u32) -> Self {
        self.track_total = Some(v);
        self
    }
    pub fn disc(mut self, v: u32) -> Self {
        self.disc = Some(v);
        self
    }
    pub fn disc_total(mut self, v: u32) -> Self {
        self.disc_total = Some(v);
        self
    }
    pub fn year(mut self, v: u32) -> Self {
        self.year = Some(v);
        self
    }
    pub fn genre(mut self, v: &str) -> Self {
        self.genre = Some(v.to_owned());
        self
    }
    pub fn compilation(mut self, v: bool) -> Self {
        self.compilation = v;
        self
    }

    /// Write every set field into `tag` (any [`TagType`]). Unset fields are left
    /// untouched; `compilation: false` writes nothing.
    pub fn apply(&self, tag: &mut Tag) {
        if let Some(v) = &self.title {
            tag.set_title(v.clone());
        }
        if let Some(v) = &self.artist {
            tag.set_artist(v.clone());
        }
        if let Some(v) = &self.album {
            tag.set_album(v.clone());
        }
        if let Some(v) = &self.album_artist {
            tag.insert_text(ItemKey::AlbumArtist, v.clone());
        }
        if let Some(v) = self.track {
            tag.set_track(v);
        }
        if let Some(v) = self.track_total {
            tag.set_track_total(v);
        }
        if let Some(v) = self.disc {
            tag.set_disk(v);
        }
        if let Some(v) = self.disc_total {
            tag.set_disk_total(v);
        }
        if let Some(v) = self.year {
            tag.insert_text(ItemKey::RecordingDate, v.to_string());
        }
        if let Some(v) = &self.genre {
            tag.set_genre(v.clone());
        }
        if self.compilation {
            tag.insert_text(ItemKey::FlagCompilation, "1".to_owned());
        }
    }
}

/// A synthetic library rooted in a fresh temp directory (deleted on drop).
///
/// All `rel` arguments are relative to [`Library::root`]; parent directories are
/// created as needed, so nested layouts (`Artist/Album/CD1/01.flac`) need no setup.
/// The `*_at` variants take a `Path` for names that are not valid UTF-8.
///
/// ```ignore
/// let lib = Library::new();
/// let base = Tags::new().album("Album").artist("Artist").album_artist("Artist");
/// lib.add_album("Artist/Album/CD1", Fixture::Flac, &base.clone().disc(1), 10);
/// lib.add_album("Artist/Album/CD2", Fixture::Flac, &base.clone().disc(2), 8);
/// lib.add_corrupt("Broken/01.flac", Corrupt::Truncated);
/// lib.add_raw("Artist/Album/cover.jpg", b"jpeg");
/// ```
pub struct Library {
    dir: TempDir,
    /// Paths whose permissions were restricted; restored on drop so cleanup works.
    restricted: Mutex<Vec<PathBuf>>,
}

impl Library {
    pub fn new() -> Self {
        Self {
            dir: tempfile::tempdir().expect("create temp library"),
            restricted: Mutex::new(Vec::new()),
        }
    }

    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    /// Absolute path of `rel` inside the library (nothing is created).
    pub fn path(&self, rel: impl AsRef<Path>) -> PathBuf {
        self.dir.path().join(rel)
    }

    /// Copy the `fixture` template to `rel` (parent dirs created) and write `tags`
    /// into the file's primary tag. Returns the absolute path.
    pub fn add(&self, rel: &str, fixture: Fixture, tags: &Tags) -> PathBuf {
        self.add_at(rel, fixture, tags)
    }

    /// [`Library::add`] for arbitrary (e.g. non-UTF-8) relative paths.
    pub fn add_at(&self, rel: impl AsRef<Path>, fixture: Fixture, tags: &Tags) -> PathBuf {
        self.add_with_at(rel, fixture, tags, |_| {})
    }

    /// Like [`Library::add`], then let `edit` change the primary tag freely before it
    /// is saved: raw dates, `3/12` strings, odd keys, removing items, ...
    ///
    /// ```ignore
    /// lib.add_with("A/01.mp3", Fixture::Mp3, &Tags::new(), |tag| {
    ///     tag.insert_text(ItemKey::RecordingDate, "2003-05-12".into());
    /// });
    /// ```
    pub fn add_with(
        &self,
        rel: &str,
        fixture: Fixture,
        tags: &Tags,
        edit: impl FnOnce(&mut Tag),
    ) -> PathBuf {
        self.add_with_at(rel, fixture, tags, edit)
    }

    fn add_with_at(
        &self,
        rel: impl AsRef<Path>,
        fixture: Fixture,
        tags: &Tags,
        edit: impl FnOnce(&mut Tag),
    ) -> PathBuf {
        let dst = self.add_untagged_at(rel, fixture);
        let primary = lofty::probe::Probe::open(&dst)
            .and_then(|p| Ok(p.guess_file_type()?))
            .and_then(|p| p.read())
            .expect("read template copy")
            .primary_tag_type();
        let mut tag = Tag::new(primary);
        tags.apply(&mut tag);
        edit(&mut tag);
        tag.save_to_path(&dst, WriteOptions::default())
            .expect("write tags to template copy");
        dst
    }

    /// Add an additional tag block of `tag_type` to an existing file in the library
    /// (e.g. an ID3v1 or APE tag next to the ID3v2 tag of an MP3). Replaces any
    /// existing tag of that type.
    pub fn add_tag(&self, rel: impl AsRef<Path>, tag_type: TagType, tags: &Tags) {
        let path = self.path(rel);
        let mut tag = Tag::new(tag_type);
        tags.apply(&mut tag);
        tag.save_to_path(&path, WriteOptions::default())
            .expect("write extra tag");
    }

    /// Copy the template without writing any tag.
    pub fn add_untagged(&self, rel: &str, fixture: Fixture) -> PathBuf {
        self.add_untagged_at(rel, fixture)
    }

    fn add_untagged_at(&self, rel: impl AsRef<Path>, fixture: Fixture) -> PathBuf {
        let dst = self.prepare(rel);
        std::fs::copy(fixture.template_path(), &dst).expect("copy template");
        dst
    }

    /// Add `count` tracks to directory `dir`: files `01.<ext>` .. `NN.<ext>`, each
    /// with `base` plus title `Track NN`, track number and track total `count`.
    /// For a disc folder pass e.g. `"Album/CD2"` and `&base.clone().disc(2)`.
    /// Returns the absolute paths in track order.
    pub fn add_album(&self, dir: &str, fixture: Fixture, base: &Tags, count: u32) -> Vec<PathBuf> {
        (1..=count)
            .map(|n| {
                let tags = base
                    .clone()
                    .title(&format!("Track {n:02}"))
                    .track(n)
                    .track_total(count);
                let rel = format!("{dir}/{n:02}.{}", fixture.extension());
                self.add(&rel, fixture, &tags)
            })
            .collect()
    }

    /// Copy a committed corrupt fixture to `rel`.
    pub fn add_corrupt(&self, rel: &str, kind: Corrupt) -> PathBuf {
        let dst = self.prepare(rel);
        std::fs::copy(kind.path(), &dst).expect("copy corrupt fixture");
        dst
    }

    /// Write arbitrary bytes to `rel` (non-audio files, odd extensions, ...).
    pub fn add_raw(&self, rel: &str, bytes: &[u8]) -> PathBuf {
        self.add_raw_at(rel, bytes)
    }

    /// [`Library::add_raw`] for arbitrary (e.g. non-UTF-8) relative paths.
    pub fn add_raw_at(&self, rel: impl AsRef<Path>, bytes: &[u8]) -> PathBuf {
        let dst = self.prepare(rel);
        std::fs::write(&dst, bytes).expect("write raw file");
        dst
    }

    /// Create directory `rel` (and parents); for empty or hidden directories.
    pub fn mkdir(&self, rel: impl AsRef<Path>) -> PathBuf {
        let dst = self.path(rel);
        std::fs::create_dir_all(&dst).expect("create dir");
        dst
    }

    /// The [`ScannedFile`] the scanner would produce for `rel` (for calling
    /// `metadata::read_track` directly).
    pub fn scanned(&self, rel: impl AsRef<Path>) -> ScannedFile {
        let path = self.path(rel);
        let size = std::fs::symlink_metadata(&path).map_or(0, |m| m.len());
        ScannedFile {
            rel_path: liner::model::rel_path_string(self.root(), &path),
            path,
            size,
        }
    }

    /// Create a symlink at `rel` pointing to `target` (stored verbatim, so relative
    /// targets are relative to the link's directory).
    #[cfg(unix)]
    pub fn symlink(&self, rel: impl AsRef<Path>, target: impl AsRef<Path>) -> PathBuf {
        let dst = self.prepare(rel);
        std::os::unix::fs::symlink(target, &dst).expect("create symlink");
        dst
    }

    /// Set unix permission bits on `rel` (e.g. `0o000`). Permissions are restored
    /// to `0o755` when the library is dropped so the temp dir can be removed.
    #[cfg(unix)]
    pub fn chmod(&self, rel: impl AsRef<Path>, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        let path = self.path(rel);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))
            .expect("set permissions");
        self.restricted.lock().unwrap().push(path);
    }

    fn prepare(&self, rel: impl AsRef<Path>) -> PathBuf {
        let dst = self.dir.path().join(rel);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).expect("create fixture dirs");
        }
        dst
    }
}

impl Default for Library {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let paths = self
                .restricted
                .get_mut()
                .map(std::mem::take)
                .unwrap_or_default();
            for path in paths.iter().rev() {
                let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
            }
        }
    }
}

/// A file name that is not valid UTF-8: `prefix` + byte `0xFF` + `suffix`
/// (e.g. `non_utf8_name("Caf", ".flac")`). Linux/unix only.
#[cfg(unix)]
pub fn non_utf8_name(prefix: &str, suffix: &str) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    let mut bytes = prefix.as_bytes().to_vec();
    bytes.push(0xFF);
    bytes.extend_from_slice(suffix.as_bytes());
    OsString::from_vec(bytes)
}
