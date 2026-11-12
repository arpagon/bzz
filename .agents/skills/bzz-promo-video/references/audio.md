# Voice, music and mix

Sound comes from the user's `voice` and `music` skills (outside this
repository). Without them, make a silent film whose captions carry the script.

GPU order: ACE-Step refuses to start while `qwen3-tts-server` or ComfyUI runs.
Drama runs on Modal, so it does not compete for the local GPU.

## Narration (Drama 3 on Modal, `voice` skill)

1. **Cast.** Eight unconditioned takes of the opening lines in one batch
   (`--batch-file`, up to eight texts). Loudness-match them to −16 LUFS MP3s and
   give the owner one file with gaps plus singles. Report length and median F0
   per take; say plainly that the agent cannot hear them.
2. **Full-script test.** For each shortlisted take, clone it as the reference
   (`--reference-audio take.wav`, its exact transcript, `--voice-consent`) and
   render the whole script; mix each over a draft bed. The owner listens on a
   phone speaker and headphones.
3. Spell "bzz" as `buzz` in TTS text. Write numbers as words.
4. Verify every take with local ASR (complete text, no extra words).

Cost and licence: each batch is about USD 0.21–0.28 GPU-only estimate (not an
invoice; 1.5–2 min cold start). Drama's Community License allows commercial
output below USD 500,000 annual revenue; credit "Voice: Fish Audio Drama 3".

## Music (ACE-Step 1.5 XL Turbo, `music` skill)

Instrumental brief at the film length plus ~9 s, for example: "modern tech
product launch music, 112 BPM, warm analog pads, soft plucked arpeggio, gentle
sub bass, minimal drums after a quiet intro, steady build with a clear lift,
clean resolved ending, leaves space for a voiceover, no vocals". Three to eight
seeds, 8 steps, `batch_size: 1`; keep each request, result, image id and model
lock beside the take. Check with ASR that no words were generated. Requested BPM
and cue times are not measured beats.

## Timing and mix

- Split the chosen narration at silences (`silencedetect=noise=-40dB:d=0.18`)
  and confirm each segment with ASR; film time = voice time + the voice offset.
- `build-audio.sh` (film project): narration at −16 LUFS from 0.5 s, music
  normalised to −22 LUFS, `sidechaincompress` ducking, 3 s fade, master
  `loudnorm=I=-14:TP=-1`. Measure the result with `ebur128=peak=true`.
