#!/usr/bin/env bash
# Build the film's audio: the narration (one Drama 3 take per line, cloned
# from the launch films' narrator, placed at each chapter's slot by
# build-narration.py) over an ACE-Step bed made for this film, with
# sidechain ducking, mastered to -14 LUFS / -1.5 dBTP (so that the AAC of the master stays under -1 dBTP). The bed is placed so
# that its own resolved ending lands at the end of the film (MUSIC_END: the
# second of the take where its music ends, measured); a 2 s fade guards it.
# Writes assets/audio/mix.wav (48 kHz stereo).
#   VOICE=<narration.wav> MUSIC=<take.wav> MUSIC_END=<s> scripts/build-audio.sh
set -euo pipefail
film=$(cd "$(dirname "$0")/.." && pwd)
voice=${VOICE:-"$film/../voice/narration.wav"}
music=${MUSIC:-"$film/../music/chosen.wav"}
total=$(python3 -c "import json;print(json.load(open('$film/timing/film.json'))['total_s'])")
music_end=${MUSIC_END:-$(python3 -c "import json;print(json.load(open('$film/../music/chosen.json'))['music_ends_s'])")}
# Skip the bed's first seconds so that its end falls 0.4 s before the film's.
skip=$(python3 -c "print(max(0.0, round($music_end - ($total - 0.4), 3)))")
delay=$(python3 -c "print(int(max(0.0, ($total - 0.4) - $music_end) * 1000))")
fade=$(python3 -c "print(round($total - 2, 3))")
out=${OUT:-"$film/assets/audio/mix.wav"}
mkdir -p "$(dirname "$out")"
ffmpeg -v error -y -i "$voice" -i "$music" -filter_complex "
 [0:a]aresample=48000,aformat=channel_layouts=stereo,loudnorm=I=-16:TP=-1.5,apad=whole_dur=${total},asplit=2[v][key];
 [1:a]aresample=48000,atrim=start=${skip},asetpts=PTS-STARTPTS,adelay=${delay}|${delay},loudnorm=I=-22:TP=-2,apad=whole_dur=${total},atrim=0:${total},afade=t=in:st=0:d=1.2,afade=t=out:st=${fade}:d=2[m];
 [m][key]sidechaincompress=threshold=0.03:ratio=6:attack=20:release=400[duck];
 [duck][v]amix=inputs=2:normalize=0,loudnorm=I=-14:TP=-1.5:LRA=11[out]" \
 -map "[out]" -t "$total" -ar 48000 -c:a pcm_s16le "$out"
echo "music: skip ${skip} s, delay ${delay} ms, film ${total} s"
ffmpeg -hide_banner -i "$out" -af ebur128=peak=true -f null - 2>&1 | grep -E '^ *(I|Peak):'
