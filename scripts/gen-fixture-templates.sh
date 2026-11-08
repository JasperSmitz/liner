#!/usr/bin/env bash
# Regenerates the committed audio fixture templates under tests/fixtures/.
#
# Run manually (dev-time only); the output is committed so tests never need
# ffmpeg. Templates are ~0.2 s of mono silence at a low sample rate with ALL
# metadata stripped; integration tests copy them to a tempdir and write tags
# onto the copies (see tests/common/mod.rs). Never point this at a real library.
set -euo pipefail

command -v ffmpeg >/dev/null || { echo "ffmpeg is required" >&2; exit 1; }

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tpl="$repo_root/tests/fixtures/templates"
bad="$repo_root/tests/fixtures/corrupt"
mkdir -p "$tpl" "$bad"

dur=0.2
# -bitexact + -map_metadata -1 keep output reproducible and tag-free.
in=(-hide_banner -loglevel error -y -f lavfi -t "$dur" -i "anullsrc=r=8000:cl=mono")
out=(-map_metadata -1 -fflags +bitexact -flags:a +bitexact)

ffmpeg "${in[@]}" "${out[@]}" -c:a flac                   "$tpl/silence.flac"
ffmpeg "${in[@]}" "${out[@]}" -c:a libmp3lame -b:a 8k -id3v2_version 0 -write_xing 0 "$tpl/silence.mp3"
ffmpeg "${in[@]}" "${out[@]}" -c:a libvorbis -q:a 0       "$tpl/silence.ogg"
ffmpeg "${in[@]}" "${out[@]}" -c:a libopus -b:a 6k        "$tpl/silence.opus"
ffmpeg "${in[@]}" "${out[@]}" -c:a aac -b:a 8k -movflags +faststart "$tpl/silence.m4a"
ffmpeg "${in[@]}" "${out[@]}" -c:a pcm_s16le              "$tpl/silence.wav"

# Corrupt fixtures (read errors must be recorded, never panic):
#  - empty.flac:     zero bytes
#  - truncated.flac: valid "fLaC" magic but cut off inside STREAMINFO
#  - garbage.flac:   deterministic pseudo-random bytes with a .flac extension
: > "$bad/empty.flac"
head -c 20 "$tpl/silence.flac" > "$bad/truncated.flac"
LC_ALL=C awk 'BEGIN { srand(42); for (i = 0; i < 1024; i++) printf "%c", int(rand() * 256) }' > "$bad/garbage.flac"

ls -l "$tpl" "$bad"
