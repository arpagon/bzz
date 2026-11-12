#!/usr/bin/env bash
# Build the film's audio: chosen narration (voice 002, script v2) from 0.5 s
# over ACE-Step music take 1101 with sidechain ducking, a 3 s fade, mastered to
# -14 LUFS / -1 dBTP. Writes assets/audio/mix.wav (48 kHz stereo).
set -euo pipefail
# Inputs are not committed (synthesis masters): VOICE and MUSIC default to the
# owner's experiment, runs/experiments/promo-video/voice-casting.
film=$(cd "$(dirname "$0")/.." && pwd)
cast="$film/../../runs/experiments/promo-video/voice-casting/fullscript"
voice=${VOICE:-"$cast/voice-002/001.wav"}
music=${MUSIC:-"$(cat "$cast/music-run-dir.txt")/seed-1101/take.wav"}
offset=${OFFSET:-0.5} total=${TOTAL:-47}
delay=$(python3 -c "print(int($offset*1000))"); fade=$(python3 -c "print($total-3)")
mkdir -p "$film/assets/audio"
ffmpeg -v error -y -i "$voice" -i "$music" -filter_complex "
 [0:a]aresample=48000,aformat=channel_layouts=stereo,loudnorm=I=-16:TP=-1.5,adelay=${delay}|${delay},apad=whole_dur=${total},asplit=2[v][key];
 [1:a]aresample=48000,loudnorm=I=-22:TP=-2,atrim=0:${total},afade=t=out:st=${fade}:d=3[m];
 [m][key]sidechaincompress=threshold=0.03:ratio=6:attack=20:release=400[duck];
 [duck][v]amix=inputs=2:normalize=0,loudnorm=I=-14:TP=-1:LRA=11[out]" \
 -map "[out]" -t "$total" -ar 48000 -c:a pcm_s16le "$film/assets/audio/mix.wav"
ffmpeg -hide_banner -i "$film/assets/audio/mix.wav" -af ebur128=peak=true -f null - 2>&1 | grep -E '^ *(I|Peak):'
