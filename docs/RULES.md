# Rule catalogue

Codes are stable kebab-case identifiers (ADR 0003). Source of truth in code:
`rules::codes` and `rules::CATALOG`; `liner rules` prints the list. Each code has one
fixed severity and scope. Severities: **error** = positions/numbering that will
make players mis-order or merge tracks; **warning** = inconsistency that likely
needs fixing; **info** = worth knowing, often intentional.

Common definitions (see ADR 0002):
- *unit*: album unit (directory, disc folders merged); `album_id` = its id: the
  root-relative directory path in lossy UTF-8, with ` #2`, ` #3`, ... appended when
  distinct directories (names differing only in invalid UTF-8 bytes) would get the
  same id (ADR 0002). Disc folder paths (`folder`) are disambiguated the same way.
- *effective disc*: tag disc → folder disc hint → 1 if the track's album partition
  is single-disc → unknown.
- *multi-disc* (per album partition): any disc tag > 1, disc total > 1 or disc
  folder number > 1, 2+ disc folders, or a disc folder alongside loose files in the
  parent directory. A unit is multi-disc if any partition is.
- *position*: `(effective disc, track_no)`.
- *normalised*: `model::normalize_for_compare` (Unicode NFC, curly quotes folded
  to straight ones, trim, collapse whitespace, lowercase). Absent values never count
  as a distinct value.
- `paths` lists the affected files (root-relative, sorted). `details` keys below are
  contractual; all numbers are integers, values keep original spelling.
- *all tracks agree* (for totals and the shared artist): every track that has the
  value carries the same one, and at least one track has it; tracks without the
  value are ignored.
- *album partition*: tracks are keyed by (normalised album title, normalised
  album artist), where the artist part counts only in a unit with 2+ distinct
  normalised album artists (a track without an album artist then uses its track
  artist). In a unit with 2+ distinct such keys that have an album title (reported
  by `mixed-album-directory`), the rules marked "per album" below run separately
  for each key (tracks without an album tag form one more partition) and their
  messages are prefixed with `Album '<title>': `, or `Album '<title>' by
  '<artist>': ` when keyed by artist. In every other unit the whole unit is one
  partition. This keeps a flat folder of unrelated albums (even ones sharing a
  generic title like "Greatest Hits") from also producing duplicate positions,
  gaps or artist issues across albums, while a compilation with one album artist,
  or none, stays one album.
- *untitled partition*: a partition in which no track has an album tag (loose
  singles). It is not an album: none of the rules in the numbering table run for
  it, so a "Singles" folder whose files are all track 1 of 1 gets no
  `duplicate-track-position`. File-scope rules still apply. The multi-disc decision, the effective-disc fallback and the
  split-album key are also computed per partition, so one album's disc tags never
  affect another album in the same directory. All album-scope rules in the
  numbering table run per album (except for an untitled partition); `mixed-album-directory` and
  `inconsistent-tag-spelling` look at the whole unit.

## File scope

One issue per affected file; `paths` = that file.

| Code | Sev | Trigger | `details` |
|---|---|---|---|
| `missing-album` | warning | No album tag. | – |
| `missing-artist` | warning | Neither artist nor album artist tag. | – |
| `missing-title` | info | No title tag. | – |
| `missing-track-number` | warning | No (or zero) track number. | – |
| `track-number-exceeds-total` | warning | `track_no > track_total`. | `track_no`, `track_total` |
| `disc-number-exceeds-total` | warning | `disc_no > disc_total`. | `disc_no`, `disc_total` |

False positives: `missing-title`/`missing-track-number` on hidden tracks, interludes
or loose singles in a "Singles" folder; use `--min-severity` to hide info.

## Album scope: positions and numbering

| Code | Sev | Trigger | `details` |
|---|---|---|---|
| `duplicate-track-position` | error | Per album: 2+ files share a position (effective disc known, track number present). One issue per position. Not emitted where `likely-missing-disc-numbers` fires. | `disc`, `track` |
| `likely-missing-disc-numbers` | error | Per album: no file has a disc tag or folder hint, and at least 2 track numbers, including 1, are each shared by *distinct* tracks (different normalised titles; when a file at that number has no title, different file stems ignoring the extension and ` (N)` copy suffixes). One issue per album partition, `paths` = files with repeated numbers. | `duplicated_tracks` (sorted list of all repeated numbers) |
| `missing-disc-number` | warning | Per album: multi-disc and some tracks have no disc tag (whether or not a folder hint exists). `paths` = untagged files. | `count` |
| `disc-folder-mismatch` | error | Per album: tag disc ≠ folder disc hint. One issue per (folder, tag disc) combination; `folder` = root-relative disc folder path; `paths` = those files. | `tag_disc`, `folder_disc`, `folder` |
| `track-number-gap` | warning | Per album, on a disc with known effective number and at least one track number, numbers missing in `1..=N`, where N = max(highest track number, track total if all tracks on that disc agree). One issue per disc. `paths` = the disc's files. Continuous numbering: a disc continues the previous disc when its lowest number is greater than 1 and greater than the previous disc's highest; its range then starts at the previous disc's highest + 1 (so numbers missing at the boundary, e.g. the last track of disc 1 or the first of disc 2, are reported once, on the later disc), and a disc continued by the next one uses N = its highest number. Not evaluated when N > 999. | `disc`, `missing` (sorted list), `expected_total` (int or null) |
| `disc-number-gap` | warning | Per album: at least one effective disc is known and known discs are missing in `1..=D`, where D = max(highest disc, disc total if all tracks agree). `paths` = all files of the album partition. Not evaluated when D > 999. | `missing` (sorted list), `expected_total` (int or null) |
| `inconsistent-track-total` | warning | Per album: tracks on the same (known) disc carry 2+ different track totals. One issue per disc; `paths` = the disc's files that have a total. | `disc`, `values` (sorted list) |
| `inconsistent-disc-total` | warning | Per album: tracks carry 2+ different disc totals. `paths` = files that have a disc total. | `values` (sorted list) |

Why track 1: every disc starts at track 1, so distinct tracks sharing track 1 and
at least one more number, without any disc information, are the signature of
merged discs. Copies of the same track do not count: the same album in two formats
in one folder (`01.flac` + `01.mp3`), or a single `01 (1).flac` copy, gets
`duplicate-track-position` instead of a misleading "probably several discs", as
does a folder where only, say, track 7 repeats, or where only track 1 repeats (a
second disc of one track is then reported as a duplicate). Where `likely-missing-disc-numbers` fires, the
(disc, track) positions are known to be unreliable, so `duplicate-track-position`,
`track-number-gap` and `inconsistent-track-total` are not emitted for that album
partition (they would only restate the same cause, e.g. discs of 12 and 8 tracks
merged into one).

False positives: `track-number-gap` / `disc-number-gap` for intentionally
incomplete albums (partial downloads, a single disc of a box set);
`track-number-exceeds-total` for vinyl-style sides numbering. Bonus discs with
their own numbering (`Disc 1`, `Disc 2`, `Bonus`) produce a separate unit for the
non-matching folder. Tracks without a track number are ignored by position rules
(they get `missing-track-number`).

## Album scope: album-level tags

| Code | Sev | Trigger | `details` |
|---|---|---|---|
| `mixed-album-directory` | warning | 2+ distinct normalised album titles, or 2+ distinct normalised album artists, in one unit. One issue per field; `paths` = files that have the field. | `field` (`"album"`/`"album_artist"`), `values` (one original spelling — the most common — per normalised value, sorted) |
| `inconsistent-tag-spelling` | warning | Values equal after normalisation but different as text (case, whitespace, Unicode NFC/NFD form, curly vs straight quotes), for album or album artist. One issue per field and normalised value; `paths` = files with one of those spellings. | `field`, `values` (distinct original spellings, sorted) |
| `partial-album-artist` | warning | Per album: some tracks have an album artist, others do not. `paths` = files without it. | `album_artist` (most common value) |
| `missing-album-artist` | warning | Per album: no track has an album artist and the tracks have 2+ distinct normalised artists. `paths` = the album's files. | `artists` (one original spelling per normalised artist, sorted) |
| `inconsistent-year` | info | Per album: tracks have 2+ different years. `paths` = files that have a year. | `values` (sorted list) |

False positives: `mixed-album-directory` for a deliberately flat "Singles" or
"Misc" folder (a future config ignore-list could address it; not in scope now;
thanks to album partitions such a folder gets only this issue, not duplicates or
gaps across albums; singles without album tags get no position issues at all). Trade-off of partitions: an album whose discs carry different
album titles (`Foo (Disc 1)`, `Foo (Disc 2)`) is reported as mixed, and its
per-album rules then see each title separately;
`missing-album-artist` for albums with featured artists in the artist tag
(`A feat. B`) — often a real problem for players anyway; `inconsistent-year` for
compilations/reissues that keep original track years.

## Library scope

| Code | Sev | Trigger | `details` |
|---|---|---|---|
| `split-album` | warning | The same `album_key` occurs for album partitions in 2+ units. The key is computed per album partition: (normalised album artist if the partition has exactly one, else the single normalised track artist shared by all its tracks that have one; normalised album). No key, so never split, when there is no album title or no unambiguous artist, when every track of the partition is flagged as a compilation, or when the artist normalises to `various artists`, `various` or `va`. `album_id` null; `paths` = the files of the matching partitions only. | `album`, `album_artist` (original spellings from the first unit by id: the most common album artist, or else track artist, spelling of the key's artist), `album_ids` (sorted) |

False positives: two different releases with the same title by the same artist
(e.g. `Live` albums, deluxe and standard editions in separate folders), or
disc folders not named `CD/Disc/Disk N`. Various-artists compilations with
generic titles ("Greatest Hits") are skipped as described above; ones tagged with
another album artist and without compilation flags can still collide.

## Not issues

Unreadable, unsupported and corrupt files (and walk failures) appear in `errors[]`,
not `issues[]`, and have no rule code. They are a library-health problem, though:
for the exit code they count as **error** severity, so any entry in `errors[]`
makes `liner scan` exit 1 unless `--fail-on never`. `--min-severity` never hides
them.
