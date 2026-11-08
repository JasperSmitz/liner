# ADR 0002: Album grouping strategy

Status: accepted (M0); amended M2 review (album partitions, multi-disc rules,
normalisation, compilations)

## Context
Inconsistencies are only detectable relative to an "album". Grouping purely by tags
hides exactly the problems we want to find (inconsistent tags split an album);
grouping purely by folder misses albums spread over folders.

## Decision
1. **Album unit = directory** containing audio files. Unit id = root-relative
   directory path (`"."` for the root).
2. **Disc folders merge into the parent.** A directory whose name matches
   `^(cd|disc|disk)[\s._-]*(\d{1,3})([\s._-]+.*|[(\[].*)?$` (case-insensitive,
   ASCII digits only, number ≥ 1; e.g. `CD1`, `CD 2`, `Disc 1`, `Disk02`,
   `Disc 1 - Title`, `CD1(Bonus)`, `Disc 2[Live]`) joins its parent's unit; its
   number is each track's `folder_disc` hint. Only one level is merged.
3. **Album partitions.** If a unit's tracks carry 2+ distinct normalised album
   titles, each title is an album partition (tracks without an album tag form their
   own); otherwise the whole unit is one partition. The multi-disc decision, the
   effective disc, all numbering rules and the split-album key are per partition, so
   one album's tags never affect another album in the same directory.
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
- Box sets with per-disc folders not named `CD/Disc/Disk N` are treated as separate
  units and may show up as `split-album` (documented false positive).
