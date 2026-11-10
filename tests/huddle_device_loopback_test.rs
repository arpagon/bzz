//! T7: huddle audio through a real virtual audio device.
//!
//! `scripts/test-audio-loopback.sh` creates a virtual loopback device (a
//! PipeWire or PulseAudio null sink and its monitor, or BlackHole on macOS),
//! routes the default devices (or names them in `BZZ_AUDIO_INPUT` /
//! `BZZ_AUDIO_OUTPUT`) and runs this test. `BZZ_AUDIO_STREAMS_CMD`, when set,
//! prints how many audio streams the test process has open on the sound
//! server; it checks that dropping the devices releases them.
#![cfg(feature = "huddles-audio")]

use std::time::{Duration, Instant};

const RATE: f32 = 48_000.0;

fn streams() -> Option<usize> {
    let command = std::env::var("BZZ_AUDIO_STREAMS_CMD").ok()?;
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .env("BZZ_AUDIO_PID", std::process::id().to_string())
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

fn peak(frame: &[f32]) -> f32 {
    frame
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a virtual audio device: scripts/test-audio-loopback.sh"]
async fn tone_round_trips_through_a_virtual_device_and_drop_releases_it() {
    let input = std::env::var("BZZ_AUDIO_INPUT")
        .ok()
        .filter(|value| !value.is_empty());
    let output = std::env::var("BZZ_AUDIO_OUTPUT")
        .ok()
        .filter(|value| !value.is_empty());
    let before = streams();
    let (devices, mut capture, playback) =
        bzz::huddle::audio::open_devices(input.as_deref(), output.as_deref())
            .expect("the virtual devices open");
    // Let the streams settle, then drop whatever was captured meanwhile.
    tokio::time::sleep(Duration::from_millis(300)).await;
    while capture.try_recv().is_ok() {}
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
    let playback = feeder.await.unwrap();

    // Dropping the devices stops both streams: the capture side closes and
    // the sound server no longer lists the process's streams.
    drop(devices);
    let closed = tokio::time::timeout(Duration::from_secs(3), async {
        while capture.recv().await.is_some() {}
    })
    .await;
    assert!(closed.is_ok(), "capture stream still open after drop");
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
}
