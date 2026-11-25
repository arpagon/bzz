//! The call's mix as the spectrum bars and the orb see it (plan v0.17.0 B6).
//!
//! The audio side hands the mixed samples over through a [`MixTap`]: a fixed
//! ring of atomics, written with relaxed stores and one release bump of the
//! write index. No lock and no allocation on the audio path, and nothing is
//! written at all unless the interface asked for it ([`MixTap::want`]), which
//! it does only while a style that needs the spectrum is on screen.
//!
//! The analysis ([`Spectrum`]) runs where the screen is drawn: a Hann
//! window, a 1024-point radix-2 FFT written here (plan D11: no dependency),
//! log bands from 90 Hz to 7 kHz, a fixed range with no automatic gain (so
//! silence looks still), a fast rise, a fall that speeds up the longer it
//! lasts, and peak marks that are thrown up by a rise, hang, and fall. Every
//! constant is in seconds, so 30 and 60 frames a second look alike.
//!
//! Samples and bands are drawn and forgotten: never stored, logged or sent
//! (plan D8). No `Debug` here prints a number.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

/// Samples the ring holds: 85 ms at 48 kHz, more than one analysis window.
pub const TAP_LEN: usize = 4096;
/// The analysis window: 21 ms at 48 kHz.
pub const FFT_SIZE: usize = 1024;
/// Analysis bands; a style draws as many bars as fit and reads them across.
pub const BANDS: usize = 48;
pub const LOW_HZ: f32 = 90.0;
pub const HIGH_HZ: f32 = 7_000.0;
const RATE: f32 = 48_000.0;
/// The fixed range drawn, after the tilt: below is an empty bar, above full.
const FLOOR_DB: f32 = -64.0;
const CEILING_DB: f32 = -4.0;

/// The mixed samples of the call, newest last, for the interface to read.
pub struct MixTap {
    ring: Box<[AtomicU32]>,
    written: AtomicUsize,
    wanted: AtomicBool,
}

impl Default for MixTap {
    fn default() -> Self {
        Self {
            ring: (0..TAP_LEN).map(|_| AtomicU32::new(0)).collect(),
            written: AtomicUsize::new(0),
            wanted: AtomicBool::new(false),
        }
    }
}

impl std::fmt::Debug for MixTap {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("MixTap { .. }")
    }
}

impl MixTap {
    /// Interface side: whether a style on screen needs the samples. While
    /// not, [`MixTap::push`] returns at once.
    pub fn want(&self, wanted: bool) {
        if self.wanted.swap(wanted, Ordering::Relaxed) && !wanted {
            self.clear();
        }
    }

    pub fn is_wanted(&self) -> bool {
        self.wanted.load(Ordering::Relaxed)
    }

    /// Audio side: one mixed chunk. No lock, no allocation; a reader that
    /// races the write sees one window slightly wrong, never anything unsafe.
    pub fn push(&self, samples: &[f32]) {
        if !self.wanted.load(Ordering::Relaxed) {
            return;
        }
        let start = self.written.load(Ordering::Relaxed);
        for (offset, sample) in samples.iter().enumerate() {
            let sample = if sample.is_finite() { *sample } else { 0.0 };
            self.ring[(start + offset) % TAP_LEN].store(sample.to_bits(), Ordering::Relaxed);
        }
        self.written
            .store(start.wrapping_add(samples.len()), Ordering::Release);
    }

    /// Interface side: the newest `out.len()` samples, zeros before the
    /// first push. Returns how many samples were pushed in all, so a reader
    /// can tell that nothing new came.
    pub fn latest(&self, out: &mut [f32]) -> usize {
        let end = self.written.load(Ordering::Acquire);
        let len = out.len();
        let n = len.min(TAP_LEN);
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = if i < len - n || end + i < len {
                0.0
            } else {
                let index = (end + i - len) % TAP_LEN;
                f32::from_bits(self.ring[index].load(Ordering::Relaxed))
            };
        }
        end
    }

    /// Forgets every sample (the call stopped, or no style needs them).
    pub fn clear(&self) {
        for slot in &self.ring {
            slot.store(0, Ordering::Relaxed);
        }
    }
}

/// A radix-2 complex FFT with its twiddles and bit reversal precomputed;
/// it allocates only when built.
struct Fft {
    twiddle: Vec<(f32, f32)>,
    reverse: Vec<usize>,
}

impl Fft {
    fn new(n: usize) -> Self {
        assert!(n.is_power_of_two() && n >= 2);
        let bits = n.trailing_zeros();
        let twiddle = (0..n / 2)
            .map(|k| {
                let angle = -2.0 * std::f32::consts::PI * k as f32 / n as f32;
                (angle.cos(), angle.sin())
            })
            .collect();
        let reverse = (0..n)
            .map(|i| i.reverse_bits() >> (usize::BITS - bits))
            .collect();
        Self { twiddle, reverse }
    }

    /// In place; `re` and `im` are `n` long.
    fn run(&self, re: &mut [f32], im: &mut [f32]) {
        let n = self.reverse.len();
        for i in 0..n {
            let j = self.reverse[i];
            if j > i {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let mut length = 2;
        while length <= n {
            let step = n / length;
            for start in (0..n).step_by(length) {
                for k in 0..length / 2 {
                    let (wr, wi) = self.twiddle[k * step];
                    let a = start + k;
                    let b = a + length / 2;
                    let tr = re[b] * wr - im[b] * wi;
                    let ti = re[b] * wi + im[b] * wr;
                    re[b] = re[a] - tr;
                    im[b] = im[a] - ti;
                    re[a] += tr;
                    im[a] += ti;
                }
            }
            length <<= 1;
        }
    }
}

/// The bands of the call's mix, smoothed for drawing, with peak marks.
pub struct Spectrum {
    fft: Fft,
    window: Vec<f32>,
    samples: Vec<f32>,
    re: Vec<f32>,
    im: Vec<f32>,
    raw: Vec<f32>,
    /// Each band's `[from, to)` in fractional FFT bins, on a log scale.
    edges: Vec<(f32, f32)>,
    /// Power of each bin of the last window.
    power: Vec<f32>,
    bands: Vec<f32>,
    fall: Vec<f32>,
    peaks: Vec<f32>,
    hold: Vec<f32>,
    lift: Vec<f32>,
    last: Option<f64>,
    seen: usize,
    fresh_at: f64,
}

/// A window older than this with nothing new behind it reads as silence.
const STALE_SECONDS: f64 = 0.06;

impl Default for Spectrum {
    fn default() -> Self {
        let n = FFT_SIZE;
        let window = (0..n)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (n - 1) as f32).cos())
            .collect();
        let bin_hz = RATE / n as f32;
        let edges = (0..BANDS)
            .map(|band| {
                let at = |band: usize| {
                    LOW_HZ * (HIGH_HZ / LOW_HZ).powf(band as f32 / BANDS as f32) / bin_hz
                };
                (at(band), at(band + 1))
            })
            .collect();
        Self {
            fft: Fft::new(n),
            window,
            samples: vec![0.0; n],
            re: vec![0.0; n],
            im: vec![0.0; n],
            raw: vec![0.0; BANDS],
            edges,
            power: vec![0.0; n / 2 + 1],
            bands: vec![0.0; BANDS],
            fall: vec![0.0; BANDS],
            peaks: vec![0.0; BANDS],
            hold: vec![0.0; BANDS],
            lift: vec![0.0; BANDS],
            last: None,
            seen: 0,
            fresh_at: 0.0,
        }
    }
}

impl std::fmt::Debug for Spectrum {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Spectrum { .. }")
    }
}

impl Spectrum {
    /// Reads the newest window from `tap` and moves time to `now` (seconds
    /// on any monotonic scale). Nothing new in the tap counts as silence.
    pub fn update(&mut self, tap: &MixTap, now: f64) {
        let dt = self
            .last
            .map_or(1.0 / 30.0, |last| (now - last).clamp(0.0, 1.0) as f32);
        self.last = Some(now);
        let mut samples = std::mem::take(&mut self.samples);
        let seen = tap.latest(&mut samples);
        if seen != self.seen {
            self.fresh_at = now;
        } else if now - self.fresh_at > STALE_SECONDS {
            // Nothing came for a while (nobody is heard): silence.
            samples.fill(0.0);
        }
        self.seen = seen;
        self.analyse(&samples, dt);
        self.samples = samples;
    }

    /// Analyses one window (`FFT_SIZE` samples) and advances the smoothing
    /// by `dt` seconds.
    pub fn analyse(&mut self, samples: &[f32], dt: f32) {
        self.measure(samples);
        spread(&mut self.raw);
        let dt = dt.max(1e-4);
        for band in 0..BANDS {
            let target = self.raw[band];
            let value = &mut self.bands[band];
            let before = *value;
            if target >= *value {
                // A fast rise, about 25 ms.
                *value += (target - *value) * (1.0 - (-dt / 0.025).exp());
                self.fall[band] = 0.0;
            } else {
                // Gravity: the fall speeds up the longer it lasts.
                self.fall[band] += 9.0 * dt;
                *value = (*value - self.fall[band] * dt).max(target);
            }
            let value = *value;
            // A peak mark is thrown up by a rise, hangs, then falls.
            let rise = (value - before).max(0.0) / dt;
            let peak = &mut self.peaks[band];
            if value >= *peak {
                *peak = value;
                self.lift[band] = (0.25 + 0.04 * rise).min(1.2);
                self.hold[band] = 0.08;
            } else if self.lift[band] > 0.0 {
                *peak = (*peak + self.lift[band] * dt).min(1.0);
                self.lift[band] -= 6.0 * dt;
            } else if self.hold[band] > 0.0 {
                self.hold[band] -= dt;
            } else {
                self.lift[band] -= 6.0 * dt;
                *peak = (*peak + self.lift[band] * dt).max(value);
                if *peak <= value {
                    self.lift[band] = 0.0;
                }
            }
        }
    }

    /// The window's band energies, 0..1 on the fixed range, before smoothing.
    fn measure(&mut self, samples: &[f32]) {
        let n = FFT_SIZE;
        for i in 0..n {
            let sample = samples.get(i).copied().unwrap_or(0.0);
            self.re[i] = sample * self.window[i];
            self.im[i] = 0.0;
        }
        self.fft.run(&mut self.re, &mut self.im);
        // A full-scale sine reads 0 dB in its bin with this window.
        let norm = 16.0 / (n as f32 * n as f32);
        for (k, power) in self.power.iter_mut().enumerate() {
            *power = (self.re[k] * self.re[k] + self.im[k] * self.im[k]) * norm;
        }
        let last = self.power.len() - 1;
        for (out, &(from, to)) in self.raw.iter_mut().zip(&self.edges) {
            // The loudest whole bin in the band; a band narrower than a bin
            // (the low ones) reads the power between the two bins around
            // its middle, so the scale stays logarithmic.
            let first = from.ceil() as usize;
            let energy = if (first as f32) < to && first <= last {
                (first..=((to.ceil() as usize).saturating_sub(1)).min(last))
                    .map(|k| self.power[k])
                    .fold(0.0, f32::max)
            } else {
                let middle = ((from + to) * 0.5).min(last as f32);
                let below = middle.floor() as usize;
                let above = (below + 1).min(last);
                let t = middle - below as f32;
                self.power[below] * (1.0 - t) + self.power[above] * t
            };
            // Tilted +4.5 dB an octave above 300 Hz, so consonants and the
            // upper formants show, as speech meters do.
            let hz = (from + to) * 0.5 * RATE / n as f32;
            let tilt = 4.5 * (hz.max(300.0) / 300.0).log2();
            let db = 10.0 * energy.max(1e-12).log10() + tilt;
            *out = ((db - FLOOR_DB) / (CEILING_DB - FLOOR_DB)).clamp(0.0, 1.0);
        }
    }

    /// The bands before smoothing, of the last window analysed (tests).
    pub fn raw(&self) -> &[f32] {
        &self.raw
    }

    /// The band a frequency falls in, if it is in the drawn range.
    pub fn band_of(&self, hz: f32) -> Option<usize> {
        let bin = hz * FFT_SIZE as f32 / RATE;
        self.edges
            .iter()
            .position(|&(from, to)| (from..to).contains(&bin))
    }

    /// The loudest smoothed band in `[from, to)` of the 0..1 range of bands.
    pub fn level(&self, from: f32, to: f32) -> f32 {
        let (a, b) = band_range(from, to);
        self.bands[a..b].iter().copied().fold(0.0, f32::max)
    }

    /// The highest peak mark in the same range.
    pub fn peak(&self, from: f32, to: f32) -> f32 {
        let (a, b) = band_range(from, to);
        self.peaks[a..b].iter().copied().fold(0.0, f32::max)
    }

    /// Whether nothing moves: every band and mark at rest.
    pub fn is_still(&self) -> bool {
        self.bands.iter().chain(&self.peaks).all(|v| *v < 0.005)
    }

    /// Forgets everything (the call stopped, or the style was hidden).
    pub fn clear(&mut self) {
        for values in [
            &mut self.bands,
            &mut self.fall,
            &mut self.peaks,
            &mut self.hold,
            &mut self.lift,
            &mut self.raw,
            &mut self.samples,
            &mut self.power,
        ] {
            values.fill(0.0);
        }
        self.last = None;
    }
}

fn band_range(from: f32, to: f32) -> (usize, usize) {
    // Rounded, so neighbouring bars share no band.
    let a = ((from.clamp(0.0, 1.0) * BANDS as f32).round() as usize).min(BANDS - 1);
    let b = ((to.clamp(0.0, 1.0) * BANDS as f32).round() as usize).clamp(a + 1, BANDS);
    (a, b)
}

/// A 3-tap spread across neighbouring bands: resolved harmonics would
/// otherwise draw as a comb.
fn spread(values: &mut [f32]) {
    let mut previous = values.first().copied().unwrap_or(0.0);
    for i in 0..values.len() {
        let current = values[i];
        let next = values.get(i + 1).copied().unwrap_or(current);
        values[i] = (0.25 * previous + 0.5 * current + 0.25 * next).max(current * 0.8);
        previous = current;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::huddle::synth;

    fn window_of(hz: f32, db: f32) -> Vec<f32> {
        synth::tone(hz, db, 0, FFT_SIZE)
    }

    /// 300 Hz, 1 kHz and 3 kHz at −20 dBFS each land in their own band, the
    /// loudest of the window, and in rising order; silence is empty.
    #[test]
    fn known_tones_land_in_their_bands() {
        let mut spectrum = Spectrum::default();
        let mut found = Vec::new();
        for hz in [300.0, 1_000.0, 3_000.0] {
            spectrum.measure(&window_of(hz, -20.0));
            let expected = spectrum.band_of(hz).unwrap();
            let loudest = spectrum
                .raw()
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .unwrap()
                .0;
            assert!(
                loudest.abs_diff(expected) <= 1,
                "{hz} Hz: {loudest} {expected}"
            );
            assert!(spectrum.raw()[expected] > 0.6, "{hz} Hz is loud");
            // Far bands stay low.
            for (band, value) in spectrum.raw().iter().enumerate() {
                if band.abs_diff(expected) > 4 {
                    assert!(*value < 0.35, "{hz} Hz leaks into band {band}: {value}");
                }
            }
            found.push(expected);
        }
        assert!(found[0] < found[1] && found[1] < found[2], "{found:?}");
        // On the log scale 300 Hz sits about a quarter of the way up,
        // 1 kHz a little over half, 3 kHz at four fifths.
        let at = |band: usize| band as f32 / BANDS as f32;
        assert!((0.2..0.35).contains(&at(found[0])), "{found:?}");
        assert!((0.5..0.62).contains(&at(found[1])), "{found:?}");
        assert!((0.75..0.86).contains(&at(found[2])), "{found:?}");
        spectrum.measure(&[0.0; FFT_SIZE]);
        assert!(spectrum.raw().iter().all(|v| *v == 0.0));
    }

    /// As the bars draw them, at 21 bars (42 columns) and 40 (120): each
    /// tone lights the bar its frequency falls in, and that bar is the
    /// tallest.
    #[test]
    fn known_tones_light_the_right_bar() {
        for bars in [21_usize, 40] {
            for hz in [300.0_f32, 1_000.0, 3_000.0] {
                let mut spectrum = Spectrum::default();
                for _ in 0..10 {
                    spectrum.analyse(&window_of(hz, -20.0), 1.0 / 30.0);
                }
                let heights = (0..bars)
                    .map(|bar| {
                        spectrum.level(bar as f32 / bars as f32, (bar + 1) as f32 / bars as f32)
                    })
                    .collect::<Vec<_>>();
                let tallest = heights
                    .iter()
                    .enumerate()
                    .max_by(|a, b| a.1.total_cmp(b.1))
                    .unwrap()
                    .0;
                // Where the frequency sits on the log scale of the bars.
                let at = (hz / LOW_HZ).ln() / (HIGH_HZ / LOW_HZ).ln();
                let expected = (at * bars as f32) as usize;
                assert_eq!(tallest, expected, "{hz} Hz in {bars} bars: {heights:?}");
                // Bars four away or more are clearly lower (a 1024-point
                // window resolves about 47 Hz: at 300 Hz that is a third
                // of an octave either side).
                for (bar, height) in heights.iter().enumerate() {
                    if bar.abs_diff(expected) >= 4 {
                        assert!(*height < heights[expected] * 0.7, "{hz} Hz, bar {bar}");
                    }
                }
            }
        }
    }

    /// Through the tap: the bars rise within a few frames of a tone and fall
    /// back to rest after it, at 30 and 60 frames a second alike; a peak mark
    /// is left above the bar for a moment.
    #[test]
    fn bars_rise_fall_and_leave_a_peak_mark() {
        for fps in [30.0, 60.0] {
            let tap = MixTap::default();
            tap.want(true);
            let mut spectrum = Spectrum::default();
            let band = spectrum.band_of(1_000.0).unwrap();
            let span = |band: usize| {
                (
                    band as f32 / BANDS as f32,
                    (band as f32 + 1.0) / BANDS as f32,
                )
            };
            let (from, to) = span(band);
            let mut sample = 0_u64;
            let mut t = 0.0_f64;
            let mut risen_at = None;
            let mut mark_above = false;
            while t < 2.0 {
                // 10 ms chunks up to t; the tone from 0.2 to 0.8 s.
                while (sample as f64) < t * 48_000.0 {
                    let on = (0.2..0.8).contains(&(sample as f64 / 48_000.0));
                    let chunk = if on {
                        synth::tone(1_000.0, -20.0, sample, 480)
                    } else {
                        vec![0.0; 480]
                    };
                    tap.push(&chunk);
                    sample += 480;
                }
                spectrum.update(&tap, t);
                if risen_at.is_none() && spectrum.level(from, to) > 0.6 {
                    risen_at = Some(t);
                }
                if (0.85..1.0).contains(&t)
                    && spectrum.peak(from, to) > spectrum.level(from, to) + 0.05
                {
                    mark_above = true;
                }
                t += 1.0 / fps;
            }
            let risen_at = risen_at.expect("the bar rose");
            assert!((0.2..0.32).contains(&risen_at), "{fps}: {risen_at}");
            assert!(mark_above, "{fps}: a peak mark stays above the falling bar");
            assert!(spectrum.is_still(), "{fps}: at rest 1.2 s after the tone");
        }
    }

    #[test]
    fn the_tap_writes_nothing_unless_wanted() {
        let tap = MixTap::default();
        let mut out = [1.0_f32; 8];
        tap.push(&[0.5; 8]);
        assert_eq!(tap.latest(&mut out), 0);
        assert!(out.iter().all(|v| *v == 0.0));
        tap.want(true);
        tap.push(&[0.5, 0.25, f32::NAN]);
        let mut out = [9.0_f32; 4];
        assert_eq!(tap.latest(&mut out), 3);
        assert_eq!(out, [0.0, 0.5, 0.25, 0.0]);
        // Turned off: the samples are forgotten.
        tap.want(false);
        let mut out = [9.0_f32; 2];
        tap.latest(&mut out);
        assert_eq!(out, [0.0, 0.0]);
        assert!(
            !format!("{tap:?}{:?}", Spectrum::default())
                .chars()
                .any(|c| c.is_ascii_digit())
        );
    }

    /// A writer thread pushes while a reader takes windows: no panic, every
    /// window read is whole numbers the writer wrote (or zero).
    #[test]
    fn writer_and_reader_race_safely() {
        let tap = std::sync::Arc::new(MixTap::default());
        tap.want(true);
        let writer = {
            let tap = tap.clone();
            std::thread::spawn(move || {
                let chunk = [0.25_f32; 480];
                for _ in 0..20_000 {
                    tap.push(&chunk);
                }
            })
        };
        let mut spectrum = Spectrum::default();
        let mut window = vec![0.0; FFT_SIZE];
        for frame in 0..2_000 {
            tap.latest(&mut window);
            assert!(window.iter().all(|v| *v == 0.0 || *v == 0.25));
            spectrum.update(&tap, f64::from(frame) / 30.0);
        }
        writer.join().unwrap();
    }
}
