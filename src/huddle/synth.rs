//! Generated huddle voices, for `bzz demo` and the tests: the visualizer
//! drawn without a sound device, a relay or anybody's voice.
//!
//! A voice is a pulse train with a few harmonics, shaped into syllables of
//! four to five a second with pauses between phrases. Nothing is recorded or
//! cloned; the samples exist only long enough to measure their level, which
//! is published into the same [`Levels`] the real engine writes.

use std::sync::Arc;

use super::levels::Levels;

/// Samples a second, as in a call.
pub const RATE: u32 = 48_000;
/// One playout chunk: 10 ms.
pub const CHUNK: usize = 480;

/// One generated person: who speaks when, and how the voice sounds.
#[derive(Clone, Copy, Debug)]
pub struct Voice {
    /// Fundamental, in hertz.
    pub pitch: f32,
    pub syllables_per_second: f32,
    /// Linear gain; 1.0 is a normal speaking level (about −20 dBFS).
    pub gain: f32,
}

/// A line of the script: who speaks from `start` to `end`, in seconds.
#[derive(Clone, Copy, Debug)]
pub struct Line {
    /// The person's slot: a peer index, or [`Speaker::Mic`].
    pub speaker: Speaker,
    pub start: f32,
    pub end: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Speaker {
    Peer(u8),
    Mic,
}

/// The demo's passage: Ada (peer 1), Lin (2) and Sam (3), and you, muted.
/// Ada and Lin overlap from 5.05 to 5.85 s; you talk while muted from 6.55
/// to 7.15 s. It loops every [`DEMO_LOOP_SECONDS`].
pub const DEMO_SCRIPT: [Line; 5] = [
    Line {
        speaker: Speaker::Peer(1),
        start: 0.35,
        end: 2.55,
    },
    Line {
        speaker: Speaker::Peer(2),
        start: 3.05,
        end: 5.85,
    },
    Line {
        speaker: Speaker::Peer(1),
        start: 5.05,
        end: 6.45,
    },
    Line {
        speaker: Speaker::Mic,
        start: 6.55,
        end: 7.15,
    },
    Line {
        speaker: Speaker::Peer(3),
        start: 7.30,
        end: 9.60,
    },
];
pub const DEMO_LOOP_SECONDS: f32 = 12.0;

fn voice_of(speaker: Speaker) -> Voice {
    match speaker {
        Speaker::Peer(1) => Voice {
            pitch: 205.0,
            syllables_per_second: 4.6,
            gain: 1.0,
        },
        Speaker::Peer(2) => Voice {
            pitch: 112.0,
            syllables_per_second: 4.0,
            gain: 0.9,
        },
        Speaker::Peer(_) => Voice {
            pitch: 160.0,
            syllables_per_second: 5.0,
            gain: 0.6,
        },
        Speaker::Mic => Voice {
            pitch: 130.0,
            syllables_per_second: 4.4,
            gain: 0.7,
        },
    }
}

/// A source of generated levels, stepped by the caller's clock.
pub struct Synthetic {
    levels: Arc<Levels>,
    script: Vec<Line>,
    looped: Option<f32>,
    /// Samples generated so far.
    position: u64,
    scratch: [f32; CHUNK],
}

impl std::fmt::Debug for Synthetic {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Synthetic { .. }")
    }
}

impl Synthetic {
    /// `looped`: the script repeats every so many seconds.
    pub fn new(script: &[Line], looped: Option<f32>) -> Self {
        Self {
            levels: Arc::new(Levels::default()),
            script: script.to_vec(),
            looped,
            position: 0,
            scratch: [0.0; CHUNK],
        }
    }

    /// The demo's passage, looping.
    pub fn demo() -> Self {
        Self::new(&DEMO_SCRIPT, Some(DEMO_LOOP_SECONDS))
    }

    /// The levels it writes; read them as the engine's.
    pub fn levels(&self) -> Arc<Levels> {
        self.levels.clone()
    }

    /// The peer indices the script speaks as.
    pub fn peers(&self) -> Vec<u8> {
        let mut peers = self
            .script
            .iter()
            .filter_map(|line| match line.speaker {
                Speaker::Peer(index) => Some(index),
                Speaker::Mic => None,
            })
            .collect::<Vec<_>>();
        peers.sort_unstable();
        peers.dedup();
        peers
    }

    /// Generates every 10 ms chunk up to `seconds` and publishes its level.
    /// A long jump (the window was hidden) only publishes the last second.
    pub fn advance_to(&mut self, seconds: f64) {
        let target = (seconds.max(0.0) * f64::from(RATE)) as u64;
        let skip_before = target.saturating_sub(u64::from(RATE));
        if self.position < skip_before {
            self.position = skip_before - skip_before % CHUNK as u64;
        }
        while self.position + CHUNK as u64 <= target {
            let start = self.position;
            for speaker in self.speakers() {
                let mut any = false;
                let mut scratch = self.scratch;
                for (offset, sample) in scratch.iter_mut().enumerate() {
                    let t = (start + offset as u64) as f32 / RATE as f32;
                    *sample = self.sample(speaker, t);
                    any |= *sample != 0.0;
                }
                self.scratch = scratch;
                if !any {
                    continue;
                }
                match speaker {
                    Speaker::Peer(index) => self.levels.publish_peer(index, &self.scratch),
                    Speaker::Mic => self.levels.publish_mic(&self.scratch),
                }
            }
            self.position += CHUNK as u64;
        }
    }

    fn speakers(&self) -> Vec<Speaker> {
        let mut speakers = Vec::new();
        for line in &self.script {
            if !speakers.contains(&line.speaker) {
                speakers.push(line.speaker);
            }
        }
        speakers
    }

    fn sample(&self, speaker: Speaker, t: f32) -> f32 {
        let local = match self.looped {
            Some(period) if period > 0.0 => t % period,
            _ => t,
        };
        self.script
            .iter()
            .filter(|line| line.speaker == speaker && (line.start..line.end).contains(&local))
            .map(|line| speech(voice_of(speaker), local - line.start, line.end - line.start))
            .sum()
    }
}

/// A deterministic hash of a syllable number into 0..1.
fn noise(n: u32) -> f32 {
    let mut x = n.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f32 / 65_535.0
}

/// One sample of a generated phrase `local` seconds in, of `length` seconds.
fn speech(voice: Voice, local: f32, length: f32) -> f32 {
    use std::f32::consts::PI;
    let syllable = local * voice.syllables_per_second;
    let index = syllable as u32;
    let within = syllable.fract();
    // Some syllables are short words with a gap after them.
    let gap = noise(index.wrapping_add(7)) < 0.2 && within > 0.75;
    if gap {
        return 0.0;
    }
    let shape = (PI * within).sin().powf(0.6);
    let amp = 0.6 + 0.4 * noise(index);
    let edges = (local / 0.03).min(1.0) * ((length - local) / 0.05).clamp(0.0, 1.0);
    // Pitch falls a little over the phrase and moves with each syllable.
    let pitch = voice.pitch
        * (1.06 - 0.12 * (local / length.max(0.1)))
        * (1.0 + 0.08 * (noise(index.wrapping_add(3)) - 0.5));
    let phase = 2.0 * PI * pitch * local;
    let mut wave = 0.0;
    for harmonic in 1..=6 {
        // A vowel-like emphasis that moves from syllable to syllable.
        let formant = 2.0 + 2.0 * noise(index.wrapping_add(harmonic));
        let weight = 1.0 / (1.0 + (harmonic as f32 - formant).abs());
        wave += weight * (phase * harmonic as f32).sin();
    }
    0.09 * voice.gain * shape * amp * edges * wave
}

/// A pure tone at `db` dBFS RMS, `samples` long, starting at sample `from`.
pub fn tone(frequency: f32, db: f32, from: u64, samples: usize) -> Vec<f32> {
    let amplitude = 10_f32.powf(db / 20.0) * std::f32::consts::SQRT_2;
    (0..samples)
        .map(|i| {
            let t = (from + i as u64) as f32 / RATE as f32;
            amplitude * (2.0 * std::f32::consts::PI * frequency * t).sin()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_demo_speaks_when_its_script_says() {
        let mut synth = Synthetic::demo();
        let levels = synth.levels();
        assert_eq!(synth.peers(), vec![1, 2, 3]);
        // 1.0 to 1.5 s: Ada speaks, nobody else.
        synth.advance_to(1.0);
        let _ = (levels.take_peer(1), levels.take_peer(2), levels.take_mic());
        synth.advance_to(1.5);
        let ada = levels.take_peer(1).unwrap();
        assert!(ada > -35.0, "{ada}");
        assert_eq!(levels.take_peer(2), None);
        assert_eq!(levels.take_mic(), None);
        // 5.2 to 5.6 s: Ada and Lin at once.
        synth.advance_to(5.2);
        let _ = (levels.take_peer(1), levels.take_peer(2));
        synth.advance_to(5.6);
        assert!(levels.take_peer(1).unwrap() > -40.0);
        assert!(levels.take_peer(2).unwrap() > -40.0);
        // 6.6 to 7.1 s: you, muted.
        synth.advance_to(6.6);
        let _ = levels.take_mic();
        synth.advance_to(7.1);
        assert!(levels.take_mic().unwrap() > -40.0);
        // It loops.
        synth.advance_to(13.0);
        let _ = levels.take_peer(1);
        synth.advance_to(13.5);
        assert!(levels.take_peer(1).unwrap() > -40.0);
        assert!(!format!("{synth:?}").chars().any(|c| c.is_ascii_digit()));
    }

    #[test]
    fn a_tone_has_its_level() {
        let db = super::super::levels::chunk_db(&tone(1000.0, -20.0, 0, 480));
        assert!((db + 20.0).abs() < 0.1, "{db}");
    }
}
