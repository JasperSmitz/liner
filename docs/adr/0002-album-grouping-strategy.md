# ADR 0002: Album grouping strategy

Status: accepted (M0); amended M2 review (album partitions, multi-disc rules,
normalisation, compilations); amended final review (partitions keyed by album
artist, untitled partitions, real-path identity)

## Context
Inconsistencies are only detectable relative to an "album". Grouping purely by tags
hides exactly the problems we want to find (inconsistent tags split an album);
grouping purely by folder misses albums spread over folders.

## Decision
1. **Album unit = directory** containing audio files. Units (and disc folders)
   are identified by the real directory path, bytes included, never by its lossy
   UTF-8 display form, so directories whose names differ only in invalid UTF-8
   bytes (`Album\xfe`, `Album\xff`) stay separate units. Unit id = root-relative
   directory path, `/`-separated, lossy UTF-8 (`"."` for the root). When distinct
   directories' lossy ids collide, the one whose raw path bytes sort first keeps
   the plain id and the others get ` #2`, ` #3`, ... in raw-path-byte order (an
   id already taken by a real directory is skipped, so `Album� #2` existing on
   disk pushes the next collision to ` #3`). Disc folder paths that collide
   within a unit are suffixed the same way. Ids are therefore unique and
   deterministic, but a suffixed id is not a path that exists on disk.
2. **Disc folders merge into the parent.** A directory whose name matches
   `^(cd|disc|disk)[\s._-]*(\d{1,3})([\s._-]+.*|[(\[].*)?$` (case-insensitive,
   ASCII digits only, number ≥ 1; e.g. `CD1`, `CD 2`, `Disc 1`, `Disk02`,
   `Disc 1 - Title`, `CD1(Bonus)`, `Disc 2[Live]`) joins its parent's unit; its
   number is each track's `folder_disc` hint. Only one level is merged.
3. **Album partitions.** Each track's partition key is (normalised album title,
   normalised album artist). The artist part is used only when the unit has 2+
   distinct normalised album artists; a track without an album artist then uses
   its normalised track artist. So two "Greatest Hits" by different album artists
   in one folder are two partitions, while a compilation with one album artist and
   many track artists, or with no album artist at all, stays one partition per
   title (and the latter still gets `missing-album-artist`). If 2+ distinct keys
   with an album title exist, each is an album partition and the tracks without an
   album tag form one more; otherwise the whole unit is one partition. The
   multi-disc decision, the effective disc, all numbering rules and the
   split-album key are per partition, so one album's tags never affect another
   album in the same directory.
   A partition in which no track has an album tag (a folder of loose singles, or
   the untagged rest of a mixed folder) is never a positional album: the position,
   numbering and disc rules (`duplicate-track-position`,
   `likely-missing-disc-numbers`, `track-number-gap`, `inconsistent-track-total`,
   `missing-disc-number`, `disc-folder-mismatch`, `disc-number-gap`,
   `inconsistent-disc-total`) skip it. File-level rules (`missing-album`, ...)
   still apply to its files.
4. **Effective disc** per track: tag disc → folder hint → `1` if the track's
   partition is single-disc → unknown. A partition is multi-disc if any disc tag
   > 1, disc total > 1 or folder disc hint > 1, its tracks come from 2+ disc
   folders, or from a disc folder alongside loose files in the parent (loose files
   without a disc tag then have an unknown disc instead of being merged onto disc
   1). A unit is multi-disc if any partition is; `disc_count > 1` implies
   multi-disc, but a multi-disc unit may know only one disc (e.g. loose files next
   to a single `CD2` folder). Positions are `(effective_disc, track_no)`.
5. **Cross-checks on tags**, comparing `normalize_for_compare` values (NFC, curly
   quotes folded, whitespace collapsed, lowercased) while reporting original
   spellings; values that differ only in those respects are an
   `inconsistent-tag-spelling`, not a mix:
   - within a unit, more than one distinct album title or album artist ⇒
     `mixed-album-directory`;
   - the same `album_key` for partitions in several units ⇒ `split-album` (library
     scope), listing only those partitions' files.
6. `album_key` (per partition, from the partition's own consensus) = (the album
   artist if the partition has exactly one distinct normalised album artist, else
   the single artist shared by all tracks, album). Partitions without an album
   title or without an unambiguous artist have no key, and neither do
   various-artists compilations (every track flagged compilation, or the artist
   normalises to `various artists`, `various` or `va`), whose generic titles
   would otherwise collide. `AlbumUnit::album_key` is the key of a single-partition
   unit (`None` for a mixed unit).

### Refinement of the lead's key (documented deviation)
The original key for the mixed-directory check was (album_artist-or-artist, album)
per *file*. For a compilation without album-artist tags this yields one key per
track artist and would flag every such album as mixed. Instead, the mixed check
compares album titles and album-artist values separately, and the "various track
artists, no album artist" case gets its own rule (`missing-album-artist`). The
split-album check uses the per-partition key above, which falls back to the track
artist only when it is unambiguous. (An earlier per-unit key combined the unit's
most common title with its most common album artist independently, and could name
an album/artist pair that no track in the unit carries.)

## Consequences
- Albums stored flat in one directory with other albums are reported as mixed,
  not silently split by tags — intended.
- Nested structures like `Album/CD1/Extras/` stay separate units.
- An album whose tracks disagree on the album artist (some `A`, some `A feat. B`)
  while its title is shared is split into partitions by artist; it already gets
  `mixed-album-directory` for the album artist, and its per-album rules then see
  each artist's tracks separately (same trade-off as titles differing per disc).
- Singles folders numbered "1 of 1" no longer produce duplicate positions, but a
  real album whose files all lack the album tag is not position-checked either
  (each file gets `missing-album`).
- Box sets with per-disc folders not named `CD/Disc/Disk N` are treated as separate
  units and may show up as `split-album` (documented false positive).
