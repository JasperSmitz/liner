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

use std::path::{Path, PathBuf};

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
}

/// A synthetic library rooted in a fresh temp directory (deleted on drop).
pub struct Library {
    dir: TempDir,
}

impl Library {
    pub fn new() -> Self {
        Self {
            dir: tempfile::tempdir().expect("create temp library"),
        }
    }

    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    /// Copy the `fixture` template to `rel` (parent dirs created) and write `tags`
    /// into the file's primary tag. Returns the absolute path.
    pub fn add(&self, rel: &str, fixture: Fixture, tags: &Tags) -> PathBuf {
        // M1 placeholder: copies the template; tag writing via lofty to be
        // implemented by developer A.
        let _ = tags;
        let dst = self.prepare(rel);
        std::fs::copy(fixture.template_path(), &dst).expect("copy template");
        dst
    }

    /// Copy a committed corrupt fixture to `rel`.
    pub fn add_corrupt(&self, rel: &str, kind: Corrupt) -> PathBuf {
        let dst = self.prepare(rel);
        std::fs::copy(kind.path(), &dst).expect("copy corrupt fixture");
        dst
    }

    /// Write arbitrary bytes to `rel` (non-audio files, odd extensions, ...).
    pub fn add_raw(&self, rel: &str, bytes: &[u8]) -> PathBuf {
        let dst = self.prepare(rel);
        std::fs::write(&dst, bytes).expect("write raw file");
        dst
    }

    fn prepare(&self, rel: &str) -> PathBuf {
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
