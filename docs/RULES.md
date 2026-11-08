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
| `duplicate-track-position` | error | 2+ files in a unit share a position (effective disc known, track number present). One issue per position. Not emitted in units where `likely-missing-disc-numbers` fires. | `disc`, `track` |
| `likely-missing-disc-numbers` | error | No file in the unit has a disc tag or folder hint, and some track numbers occur more than once. One issue per unit, `paths` = files with repeated numbers. | `duplicated_tracks` (sorted list of numbers) |
| `missing-disc-number` | warning | Unit is multi-disc and some tracks have no disc tag (whether or not a folder hint exists). `paths` = untagged files. | `count` |
| `disc-folder-mismatch` | error | Tag disc ≠ folder disc hint. One issue per (folder, tag disc) combination. | `tag_disc`, `folder_disc`, `folder` |
| `track-number-gap` | warning | On a disc with known effective number, numbers missing in `1..=N`, where N = max(highest track number, track total if all tracks on that disc agree). One issue per disc. `paths` = the disc's files. | `disc`, `missing` (sorted list), `expected_total` (int or null) |
| `disc-number-gap` | warning | Known effective discs missing in `1..=D`, where D = max(highest disc, disc total if all tracks agree). | `missing` (sorted list), `expected_total` (int or null) |
| `inconsistent-track-total` | warning | Tracks on the same disc carry 2+ different track totals. One issue per disc. | `disc`, `values` (sorted list) |
| `inconsistent-disc-total` | warning | Tracks in the unit carry 2+ different disc totals. | `values` (sorted list) |

False positives: `track-number-gap` / `disc-number-gap` for intentionally
incomplete albums (partial downloads, a single disc of a box set);
`track-number-exceeds-total` for vinyl-style sides numbering. Bonus discs with
their own numbering (`Disc 1`, `Disc 2`, `Bonus`) produce a separate unit for the
non-matching folder. Tracks without a track number are ignored by position rules
(they get `missing-track-number`).

## Album scope: album-level tags

| Code | Sev | Trigger | `details` |
|---|---|---|---|
| `mixed-album-directory` | warning | 2+ distinct normalised album titles, or 2+ distinct normalised album artists, in one unit. One issue per field. | `field` (`"album"`/`"album_artist"`), `values` (one original spelling per normalised value, sorted) |
| `inconsistent-tag-spelling` | warning | Values equal after normalisation but different as text (case/whitespace), for album or album artist. One issue per field and normalised value. | `field`, `values` (distinct original spellings, sorted) |
| `partial-album-artist` | warning | Some tracks have an album artist, others do not. `paths` = files without it. | `album_artist` (most common value) |
| `missing-album-artist` | warning | No track has an album artist and the tracks have 2+ distinct normalised artists. | `artists` (sorted, original spelling) |
| `inconsistent-year` | info | Tracks have 2+ different years. | `values` (sorted list) |

False positives: `mixed-album-directory` for a deliberately flat "Singles" or
"Misc" folder (a future config ignore-list could address it; not in scope now);
`missing-album-artist` for albums with featured artists in the artist tag
(`A feat. B`) — often a real problem for players anyway; `inconsistent-year` for
compilations/reissues that keep original track years.

## Library scope

| Code | Sev | Trigger | `details` |
|---|---|---|---|
| `split-album` | warning | The same `album_key` (normalised album artist or unambiguous shared artist, album) occurs in 2+ units. `album_id` null; `paths` = all files of those units. | `album`, `album_artist` (original spellings from the first unit), `album_ids` (sorted) |

False positives: two different releases with the same title by the same artist
(e.g. `Live` albums, deluxe and standard editions in separate folders), or
disc folders not named `CD/Disc/Disk N`.

## Not issues

Unreadable, unsupported and corrupt files appear in `errors[]`, not `issues[]`, and
do not affect the exit code.
