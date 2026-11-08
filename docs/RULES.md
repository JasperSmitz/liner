# Rule catalogue

Codes are stable kebab-case identifiers (ADR 0003). Source of truth in code:
`rules::codes` and `rules::CATALOG`; `liner rules` prints the list. Each code has one
fixed severity and scope. Severities: **error** = positions/numbering that will
make players mis-order or merge tracks; **warning** = inconsistency that likely
needs fixing; **info** = worth knowing, often intentional.

Common definitions (see ADR 0002):
- *unit*: album unit (directory, disc folders merged); `album_id` = its id.
- *effective disc*: tag disc → folder disc hint → 1 if unit single-disc → unknown.
- *position*: `(effective disc, track_no)`.
- *normalised*: `model::normalize_for_compare` (trim, collapse whitespace,
  lowercase). Absent values never count as a distinct value.
- `paths` lists the affected files (root-relative, sorted). `details` keys below are
  contractual; all numbers are integers, values keep original spelling.
- *all tracks agree* (for totals and the shared artist): every track that has the
  value carries the same one, and at least one track has it; tracks without the
  value are ignored.
- *album partition*: in a unit whose tracks carry 2+ distinct normalised album
  titles (reported by `mixed-album-directory`), the rules marked "per album" below
  run separately for each album title (tracks without an album tag form their own
  partition) and their messages are prefixed with `Album '<title>': `. In every
  other unit the whole unit is one partition. This keeps a flat folder of unrelated
  albums or singles from also producing duplicate positions, gaps or artist issues
  across albums. Disc-level rules always look at the whole unit.

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
| `likely-missing-disc-numbers` | error | No file in the unit has a disc tag or folder hint, and (per album) track number 1 occurs more than once. One issue per album partition, `paths` = files with repeated numbers. | `duplicated_tracks` (sorted list of all repeated numbers) |
| `missing-disc-number` | warning | Unit is multi-disc and some tracks have no disc tag (whether or not a folder hint exists). `paths` = untagged files. | `count` |
| `disc-folder-mismatch` | error | Tag disc ≠ folder disc hint. One issue per (folder, tag disc) combination; `folder` = root-relative disc folder path; `paths` = those files. | `tag_disc`, `folder_disc`, `folder` |
| `track-number-gap` | warning | Per album, on a disc with known effective number and at least one track number, numbers missing in `1..=N`, where N = max(highest track number, track total if all tracks on that disc agree). One issue per disc. `paths` = the disc's files. Continuous numbering: if a disc's lowest number is the previous disc's highest + 1, its range starts at that lowest number, and a disc continued by the next one uses N = its highest number. Not evaluated when N > 999. | `disc`, `missing` (sorted list), `expected_total` (int or null) |
| `disc-number-gap` | warning | At least one effective disc is known and known discs are missing in `1..=D`, where D = max(highest disc, disc total if all tracks agree). `paths` = all files of the unit. Not evaluated when D > 999. | `missing` (sorted list), `expected_total` (int or null) |
| `inconsistent-track-total` | warning | Per album: tracks on the same (known) disc carry 2+ different track totals. One issue per disc; `paths` = the disc's files that have a total. | `disc`, `values` (sorted list) |
| `inconsistent-disc-total` | warning | Tracks in the unit carry 2+ different disc totals. `paths` = files that have a disc total. | `values` (sorted list) |

Why track 1: every disc starts at track 1, so a repeated track 1 without any disc
information is the signature of merged discs. A folder where only, say, track 7
repeats (a duplicated file) gets `duplicate-track-position` instead of a misleading
"probably several discs". Where `likely-missing-disc-numbers` fires, the
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
| `inconsistent-tag-spelling` | warning | Values equal after normalisation but different as text (case/whitespace), for album or album artist. One issue per field and normalised value; `paths` = files with one of those spellings. | `field`, `values` (distinct original spellings, sorted) |
| `partial-album-artist` | warning | Per album: some tracks have an album artist, others do not. `paths` = files without it. | `album_artist` (most common value) |
| `missing-album-artist` | warning | Per album: no track has an album artist and the tracks have 2+ distinct normalised artists. `paths` = the album's files. | `artists` (one original spelling per normalised artist, sorted) |
| `inconsistent-year` | info | Per album: tracks have 2+ different years. `paths` = files that have a year. | `values` (sorted list) |

False positives: `mixed-album-directory` for a deliberately flat "Singles" or
"Misc" folder (a future config ignore-list could address it; not in scope now;
thanks to album partitions such a folder gets only this issue, not duplicates or
gaps across albums). Trade-off of partitions: an album whose discs carry different
album titles (`Foo (Disc 1)`, `Foo (Disc 2)`) is reported as mixed, and its
per-album rules then see each title separately;
`missing-album-artist` for albums with featured artists in the artist tag
(`A feat. B`) — often a real problem for players anyway; `inconsistent-year` for
compilations/reissues that keep original track years.

## Library scope

| Code | Sev | Trigger | `details` |
|---|---|---|---|
| `split-album` | warning | The same `album_key` (normalised album artist or unambiguous shared artist, album) occurs in 2+ units. `album_id` null; `paths` = all files of those units. | `album`, `album_artist` (original spellings from the first unit by id; the unit's most common artist when it has no album artist), `album_ids` (sorted) |

False positives: two different releases with the same title by the same artist
(e.g. `Live` albums, deluxe and standard editions in separate folders), or
disc folders not named `CD/Disc/Disk N`.

## Not issues

Unreadable, unsupported and corrupt files (and walk failures) appear in `errors[]`,
not `issues[]`, and have no rule code. They are a library-health problem, though:
for the exit code they count as **error** severity, so any entry in `errors[]`
makes `liner scan` exit 1 unless `--fail-on never`. `--min-severity` never hides
them.
