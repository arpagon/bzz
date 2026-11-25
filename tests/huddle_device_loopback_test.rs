//! T7: huddle audio through a real virtual audio device.
//!
//! `scripts/test-audio-loopback.sh` creates a virtual loopback device (a
//! PipeWire or PulseAudio null sink and its monitor, or BlackHole on macOS),
//! routes the default devices (or names them in `BZZ_AUDIO_INPUT` /
//! `BZZ_AUDIO_OUTPUT`) and runs this test. `BZZ_AUDIO_STREAMS_CMD`, when set,
//! prints how many audio streams the test process has open on the sound
//! server; it checks that dropping the devices releases them.
//!
//! Every wait has a bound and says what it waited for: a device that stops
//! taking or giving audio fails the test in seconds, never hangs the job.
#![cfg(feature = "huddles-audio")]

use std::{
    future::Future,
    time::{Duration, Instant},
};

const RATE: f32 = 48_000.0;

/// Waits for `future` at most `bound`; past it, fails saying what it waited for.
async fn bounded<T>(what: &str, bound: Duration, future: impl Future<Output = T>) -> T {
    match tokio::time::timeout(bound, future).await {
        Ok(value) => value,
        Err(_) => panic!("T7 waited more than {bound:?} for {what}"),
    }
}

/// How many streams the test process has open on the sound server, by
/// `BZZ_AUDIO_STREAMS_CMD`; the command gets 5 s.
fn streams() -> Option<usize> {
    let command = std::env::var("BZZ_AUDIO_STREAMS_CMD").ok()?;
    let mut child = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .env("BZZ_AUDIO_PID", std::process::id().to_string())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while child.try_wait().ok()?.is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "T7 waited more than 5 s for BZZ_AUDIO_STREAMS_CMD (the sound server's list of streams)"
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().ok()?;
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

/// Runs of 1 ms or more of near silence inside the captured tone (its first
/// and last 100 ms left out), and how long that tone lasted. A 440 Hz sine at
/// 0.5 is near zero for a sample at a time, never for 48.
fn holes(samples: &[f32]) -> (usize, usize) {
    let loud = |sample: &&f32| sample.abs() > 0.25;
    let (Some(first), Some(last)) = (
        samples.iter().position(|sample| loud(&sample)),
        samples.iter().rposition(|sample| loud(&sample)),
    ) else {
        return (0, 0);
    };
    let margin = 4_800;
    if last <= first + 2 * margin {
        return (0, (last - first) / 48);
    }
    let mut holes = 0;
    let mut quiet = 0;
    for sample in &samples[first + margin..last - margin] {
        if sample.abs() < 0.01 {
            quiet += 1;
            if quiet == 48 {
                holes += 1;
            }
        } else {
            quiet = 0;
        }
    }
    (holes, (last - first) / 48)
}

fn peak(frame: &[f32]) -> f32 {
    frame
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a virtual audio device: scripts/test-audio-loopback.sh"]
async fn tone_round_trips_through_a_virtual_device_and_drop_releases_it() {
    let test_started = Instant::now();
    let input = std::env::var("BZZ_AUDIO_INPUT")
        .ok()
        .filter(|value| !value.is_empty());
    let output = std::env::var("BZZ_AUDIO_OUTPUT")
        .ok()
        .filter(|value| !value.is_empty());
    let before = streams();
    let (devices, mut capture, playback) = bounded(
        "the virtual devices to open",
        Duration::from_secs(15),
        tokio::task::spawn_blocking(move || {
            bzz::huddle::audio::open_devices(input.as_deref(), output.as_deref())
        }),
    )
    .await
    .unwrap()
    .expect("the virtual devices open");
    // A named device must be the one opened, not the default in its place.
    assert_eq!(devices.notice(), None, "a configured device was not found");

    // Let the streams settle until the capture is live: three windows of
    // 100 ms in a row each bring 60 to 160 ms of audio. A PulseAudio null
    // sink that idled with no stream sleeps up to 2 s (its latency then) and
    // renders nothing for its monitor until it wakes, however low the new
    // streams' latency; a tone played meanwhile came back only after that
    // (1.4 s in CI's image, close to the 2 s bound below). PipeWire is live
    // at once. Then drop whatever was captured meanwhile.
    let opened = Instant::now();
    let mut captured = 0_usize;
    let live = bounded(
        "the capture to bring audio at the pace of the clock (100 ms of audio every 100 ms)",
        Duration::from_secs(6),
        async {
            let mut steady = 0;
            while steady < 3 {
                let mut window = 0_usize;
                let end = Instant::now() + Duration::from_millis(100);
                while let Ok(frame) = tokio::time::timeout_at(end.into(), capture.recv()).await {
                    window += frame
                        .expect("capture closed while the devices were open")
                        .len();
                }
                captured += window;
                steady = if (2_880..=7_680).contains(&window) {
                    steady + 1
                } else {
                    0
                };
            }
            opened.elapsed()
        },
    )
    .await;
    while capture.try_recv().is_ok() {}
    println!(
        "T7 settle: capture live after {} ms ({} ms of audio in that time)",
        live.as_millis(),
        captured / 48
    );
    let during = streams();

    // Silence first: nothing loud may arrive before the tone is played.
    let silence_until = Instant::now() + Duration::from_millis(300);
    while Instant::now() < silence_until {
        if let Ok(Some(frame)) =
            tokio::time::timeout(Duration::from_millis(100), capture.recv()).await
        {
            assert!(peak(&frame) < 0.05, "loud capture before playback");
        }
    }

    // 2 s of 440 Hz at 0.5 in the engine's 10 ms playback chunks.
    let started = Instant::now();
    let feeder = tokio::spawn(async move {
        let mut phase = 0_u64;
        for _ in 0..200 {
            let chunk = (0..480)
                .map(|_| {
                    phase += 1;
                    0.5 * (2.0 * std::f32::consts::PI * 440.0 * phase as f32 / RATE).sin()
                })
                .collect::<Vec<_>>();
            if playback.send(chunk).await.is_err() {
                break;
            }
        }
        playback
    });
    let mut heard = None;
    let mut loudest = 0.0_f32;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(200), capture.recv()).await {
            Ok(Some(frame)) => {
                assert_eq!(frame.len(), 960, "20 ms frames at 48 kHz");
                let level = peak(&frame);
                loudest = loudest.max(level);
                if level > 0.1 && heard.is_none() {
                    heard = Some(started.elapsed());
                }
                if heard.is_some() && loudest > 0.3 {
                    break;
                }
            }
            Ok(None) => panic!("capture closed while the devices were open"),
            Err(_) => {}
        }
    }
    let latency = heard.unwrap_or_else(|| panic!("tone never captured (peak {loudest})"));
    println!(
        "T7 round trip: first tone frame after {} ms, peak {loudest:.2}",
        latency.as_millis()
    );
    assert!(latency < Duration::from_secs(2), "round trip {latency:?}");
    // The device takes the 2 s of tone at its own pace; 64 chunks wait in
    // the channel, so the feeder ends about 1.4 s after the device began.
    let playback = bounded(
        "the playback device to take 2 s of tone (it stopped asking for audio)",
        Duration::from_secs(8),
        feeder,
    )
    .await
    .unwrap();

    // Paced: 10 ms chunks on a 10 ms clock with `try_send`, as the engine
    // plays out, from an empty queue. Where the device buffer is too small
    // for that pace, the tone comes back with holes of silence (an underrun
    // each).
    let drained = Instant::now() + Duration::from_secs(3);
    while playback.capacity() < playback.max_capacity() && Instant::now() < drained {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    while capture.try_recv().is_ok() {}
    let paced = tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(10));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut phase = 0_u64;
        for _ in 0..300 {
            tick.tick().await;
            let chunk = (0..480)
                .map(|_| {
                    phase += 1;
                    0.5 * (2.0 * std::f32::consts::PI * 440.0 * phase as f32 / RATE).sin()
                })
                .collect::<Vec<_>>();
            let _ = playback.try_send(chunk);
        }
        playback
    });
    let mut heard_paced = Vec::new();
    let until = Instant::now() + Duration::from_millis(3300);
    while Instant::now() < until {
        if let Ok(Some(frame)) =
            tokio::time::timeout(Duration::from_millis(100), capture.recv()).await
        {
            heard_paced.extend(frame);
        }
    }
    let playback = bounded(
        "the paced feeder to send 3 s of 10 ms chunks",
        Duration::from_secs(5),
        paced,
    )
    .await
    .unwrap();
    let (holes, tone_ms) = holes(&heard_paced);
    println!("T7 paced playout: {holes} holes in {tone_ms} ms of tone");

    // Dropping the devices stops both streams: the capture side closes and
    // the sound server no longer lists the process's streams.
    bounded(
        "the devices to drop (the audio thread to end)",
        Duration::from_secs(5),
        tokio::task::spawn_blocking(move || drop(devices)),
    )
    .await
    .unwrap();
    bounded(
        "the capture stream to close after drop",
        Duration::from_secs(3),
        async { while capture.recv().await.is_some() {} },
    )
    .await;
    assert!(
        playback.is_closed(),
        "playback stream still open after drop"
    );
    if let (Some(before), Some(during)) = (before, during) {
        assert!(during > before, "streams were visible while open");
        let mut after = streams();
        for _ in 0..20 {
            if after == Some(before) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
            after = streams();
        }
        assert_eq!(after, Some(before), "the sound server released the streams");
        println!("T7 streams: {before} before, {during} open, {before} after drop");
    }
    println!("T7 took {} ms", test_started.elapsed().as_millis());
}
