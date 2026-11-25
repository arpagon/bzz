//! Live levels of a huddle, as the visualizer reads them.
//!
//! The audio side writes, the interface reads, and nothing else sees them.
//! Each slot holds the loudest 10 ms chunk since the interface last looked,
//! in hundredths of a decibel above −127 dBFS: the writer uses `fetch_max`,
//! the reader `swap(0)`. No lock and no allocation on the audio path, and a
//! word spoken between two frames is never lost.
//!
//! Levels are drawn and forgotten: never stored, logged, sent or put in
//! diagnostics (ADR v0.13 §6, plan v0.17.0 D8). No `Debug` here prints a
//! number.

use std::sync::atomic::{AtomicU32, Ordering};

/// The quietest level a slot can hold, in dBFS.
pub const FLOOR_DB: f32 = -127.0;

/// One slot per relay peer index, plus your microphone.
pub struct Levels {
    peers: [AtomicU32; 256],
    mic: AtomicU32,
    /// The mixed samples, for the styles that draw a spectrum; written only
    /// while one is on screen.
    mix: super::spectrum::MixTap,
}

impl Default for Levels {
    fn default() -> Self {
        Self {
            peers: std::array::from_fn(|_| AtomicU32::new(0)),
            mic: AtomicU32::new(0),
            mix: super::spectrum::MixTap::default(),
        }
    }
}

impl std::fmt::Debug for Levels {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Levels { .. }")
    }
}

/// The RMS of a chunk in dBFS, at least [`FLOOR_DB`]. One pass, no allocation.
pub fn chunk_db(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return FLOOR_DB;
    }
    let power = samples
        .iter()
        .map(|sample| {
            let sample = if sample.is_finite() { *sample } else { 0.0 };
            sample * sample
        })
        .sum::<f32>()
        / samples.len() as f32;
    if power > 0.0 {
        (10.0 * power.log10()).clamp(FLOOR_DB, 0.0)
    } else {
        FLOOR_DB
    }
}

fn encode(db: f32) -> u32 {
    // +1 so that zero means "nothing since the last read".
    ((db.clamp(FLOOR_DB, 0.0) - FLOOR_DB) * 100.0) as u32 + 1
}

fn decode(value: u32) -> Option<f32> {
    (value != 0).then(|| (value - 1) as f32 / 100.0 + FLOOR_DB)
}

impl Levels {
    /// Audio side: one decoded chunk of the person at `index`.
    pub fn publish_peer(&self, index: u8, samples: &[f32]) {
        self.publish_peer_db(index, chunk_db(samples));
    }

    /// Audio side: your microphone's chunk, before the mute check.
    pub fn publish_mic(&self, samples: &[f32]) {
        self.publish_mic_db(chunk_db(samples));
    }

    /// For generated sources that already know the level.
    pub fn publish_peer_db(&self, index: u8, db: f32) {
        self.peers[usize::from(index)].fetch_max(encode(db), Ordering::Relaxed);
    }

    pub fn publish_mic_db(&self, db: f32) {
        self.mic.fetch_max(encode(db), Ordering::Relaxed);
    }

    /// Audio side: one chunk of the mix you hear. Returns at once unless a
    /// style on screen asked for the samples.
    pub fn publish_mix(&self, samples: &[f32]) {
        self.mix.push(samples);
    }

    /// The mixed samples' ring, for the interface.
    pub fn mix(&self) -> &super::spectrum::MixTap {
        &self.mix
    }

    /// Interface side: the loudest chunk since the last call, `None` when no
    /// chunk arrived (a person whose packets do not come looks quiet).
    pub fn take_peer(&self, index: u8) -> Option<f32> {
        decode(self.peers[usize::from(index)].swap(0, Ordering::Relaxed))
    }

    pub fn take_mic(&self) -> Option<f32> {
        decode(self.mic.swap(0, Ordering::Relaxed))
    }

    /// Forgets every level (the call stopped).
    pub fn clear(&self) {
        for slot in &self.peers {
            slot.store(0, Ordering::Relaxed);
        }
        self.mic.store(0, Ordering::Relaxed);
        self.mix.want(false);
        self.mix.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn tone(db: f32, samples: usize) -> Vec<f32> {
        // A sine's RMS is its amplitude over √2.
        let amplitude = 10_f32.powf(db / 20.0) * std::f32::consts::SQRT_2;
        (0..samples)
            .map(|i| amplitude * (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / 48_000.0).sin())
            .collect()
    }

    #[test]
    fn a_generated_tone_reads_back_at_its_level() {
        let levels = Levels::default();
        levels.publish_peer(3, &tone(-20.0, 480));
        let db = levels.take_peer(3).unwrap();
        assert!((db + 20.0).abs() < 0.2, "{db}");
        assert_eq!(levels.take_peer(3), None, "a read forgets the level");
        levels.publish_mic(&[0.0; 960]);
        assert_eq!(levels.take_mic(), Some(FLOOR_DB));
        levels.publish_mic(&[f32::NAN; 4]);
        assert_eq!(levels.take_mic(), Some(FLOOR_DB));
    }

    #[test]
    fn the_loudest_chunk_between_two_reads_wins() {
        let levels = Levels::default();
        levels.publish_peer_db(0, -50.0);
        levels.publish_peer_db(0, -12.5);
        levels.publish_peer_db(0, -40.0);
        let db = levels.take_peer(0).unwrap();
        assert!((db + 12.5).abs() < 0.02, "{db}");
    }

    #[test]
    fn debug_prints_no_number() {
        let levels = Levels::default();
        levels.publish_peer_db(7, -20.0);
        let debug = format!("{levels:?}");
        assert!(!debug.chars().any(|c| c.is_ascii_digit()), "{debug}");
    }

    /// A writer thread publishes while a reader swaps: no panic, and the
    /// loudest chunk of each round is never lost between two reads.
    #[test]
    fn writer_and_reader_stress() {
        let levels = Arc::new(Levels::default());
        let rounds = 2_000_u32;
        let (go_tx, go_rx) = std::sync::mpsc::sync_channel::<u32>(0);
        let (done_tx, done_rx) = std::sync::mpsc::sync_channel::<()>(0);
        let writer = {
            let levels = levels.clone();
            std::thread::spawn(move || {
                while let Ok(round) = go_rx.recv() {
                    // Five quiet chunks around one loud one, as fast as possible.
                    let loud = -10.0 - (round % 20) as f32;
                    for chunk in 0..6 {
                        let db = if chunk == 3 { loud } else { -60.0 };
                        levels.publish_peer_db((round % 256) as u8, db);
                        levels.publish_mic_db(db);
                    }
                    if done_tx.send(()).is_err() {
                        break;
                    }
                }
            })
        };
        // A free-running writer on every slot meanwhile, to race the reads.
        let noise = {
            let levels = levels.clone();
            std::thread::spawn(move || {
                for i in 0..200_000_u32 {
                    levels.publish_peer_db((i % 256) as u8, -90.0);
                }
            })
        };
        for round in 0..rounds {
            let index = (round % 256) as u8;
            let _ = levels.take_peer(index);
            let _ = levels.take_mic();
            go_tx.send(round).unwrap();
            done_rx.recv().unwrap();
            let expected = -10.0 - (round % 20) as f32;
            let peer = levels.take_peer(index).unwrap();
            let mic = levels.take_mic().unwrap();
            assert!((peer - expected).abs() < 0.02, "round {round}: {peer}");
            assert!((mic - expected).abs() < 0.02, "round {round}: {mic}");
        }
        drop(go_tx);
        writer.join().unwrap();
        noise.join().unwrap();
    }
}
