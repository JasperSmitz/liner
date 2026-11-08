# ADR 0002: Album grouping strategy

Status: accepted (M0)

## Context
Inconsistencies are only detectable relative to an "album". Grouping purely by tags
hides exactly the problems we want to find (inconsistent tags split an album);
grouping purely by folder misses albums spread over folders.

## Decision
1. **Album unit = directory** containing audio files. Unit id = root-relative
   directory path (`"."` for the root).
2. **Disc folders merge into the parent.** A directory whose name matches
   `^(cd|disc|disk)[\s._-]*(\d{1,3})([\s._-]+.*)?$` (case-insensitive, number ≥ 1;
   e.g. `CD1`, `CD 2`, `Disc 1`, `Disk02`, `Disc 1 - Title`) joins its parent's unit;
   its number is each track's `folder_disc` hint. Only one level is merged.
3. **Effective disc** per track: tag disc → folder hint → `1` if the unit is
   single-disc → unknown. A unit is multi-disc if any disc tag > 1, any disc total
   > 1, or 2+ disc folders. Positions are `(effective_disc, track_no)`.
4. **Cross-checks on tags**, comparing `normalize_for_compare` values while reporting
   original spellings:
   - within a unit, more than one distinct album title or album artist ⇒
     `mixed-album-directory`;
   - the same `album_key` in several units ⇒ `split-album` (library scope).
5. `album_key` = (album_artist, or the single artist shared by all tracks, album).
   Units without an album title or without an unambiguous artist have no key and
   are not considered for `split-album`.

### Refinement of the lead's key (documented deviation)
The original key for the mixed-directory check was (album_artist-or-artist, album)
per *file*. For a compilation without album-artist tags this yields one key per
track artist and would flag every such album as mixed. Instead, the mixed check
compares album titles and album-artist values separately, and the "various track
artists, no album artist" case gets its own rule (`missing-album-artist`). The
split-album check uses the per-unit key above, which falls back to the track artist
only when it is unambiguous.

## Consequences
- Albums stored flat in one directory with other albums are reported as mixed,
  not silently split by tags — intended.
- Nested structures like `Album/CD1/Extras/` stay separate units.
- Box sets with per-disc folders not named `CD/Disc/Disk N` are treated as separate
  units and may show up as `split-album` (documented false positive).
