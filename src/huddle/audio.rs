//! Live huddle audio (feature `huddles-audio`).
//!
//! Transport: `buzz-audio-client` (v2 handshake, Opus, NetEQ jitter buffer).
//! The engine moves PCM through channels so devices are a separate layer:
//! capture sends 20 ms frames (960 mono f32 samples at 48 kHz), playback
//! receives mixed 10 ms chunks (480 samples). Nothing here logs audio, levels
//! or device names.
//!
//! The visualizer's levels ([`super::levels::Levels`]) are taken here: each
//! person's after their audio is decoded and before the mix, your
//! microphone's before the mute check. The relay's `joined`, `left` and
//! `roster` messages keep [`AudioCall::peers`] current, so a person who joins
//! after you has a name.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use buzz_audio_client::{
    connection::{Joined, Peer},
    encoder::{AudioEncoder, FRAME_SAMPLES},
    jitter::{PLAYOUT_SAMPLES, PeerJitterBuffer},
    wire::parse_relay_frame,
};
use futures_util::{SinkExt as _, StreamExt as _};
use tokio::{sync::mpsc, task::JoinHandle};
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

use super::levels::Levels;
use crate::{
    auth::signer::SignerHandle,
    error::{Error, Result},
};

/// Joins the backing channel's audio room. The NIP-42 AUTH inside the socket
/// names the relay's base websocket URL, not the huddle path.
pub async fn join(
    relay: &url::Url,
    backing: Uuid,
    parent: Uuid,
    signer: &SignerHandle,
) -> Result<Joined> {
    let url = crate::protocol::huddle::audio_url(relay, backing)?;
    let mut base = relay.clone();
    base.set_path("");
    base.set_query(None);
    let relay_tag = nostr::RelayUrl::parse(base.as_str().trim_end_matches('/'))
        .map_err(|error| Error::Protocol(error.to_string()))?;
    let signer = signer.clone();
    let parent = parent.to_string();
    buzz_audio_client::connection::connect(
        crate::realtime::session::upgrade_request(url.as_str())?,
        Some(&parent),
        None,
        move |challenge| async move {
            let event = signer
                .auth(&challenge, relay_tag)
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(&event).map_err(|error| error.to_string())
        },
    )
    .await
    .map_err(|error| Error::Protocol(format!("huddle audio: {}", sanitize(&error))))
}

/// Keeps only the fixed part of an upstream error (no device or peer data).
fn sanitize(error: &str) -> String {
    crate::protocol::artifact::display(error, 160)
}

/// A running call. Dropping it (or [`AudioCall::stop`]) leaves the room and
/// stops the engine; device streams are owned by the caller's device layer.
pub struct AudioCall {
    pub muted: Arc<AtomicBool>,
    /// The room as the relay last described it, you included.
    pub peers: Arc<Mutex<Vec<Peer>>>,
    pub index: u8,
    /// Live levels for the visualizer; drawn and forgotten.
    pub levels: Arc<Levels>,
    leave: Option<tokio::sync::oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl AudioCall {
    /// Mute on join; the caller unmutes or uses push-to-talk.
    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
    }

    pub fn is_muted(&self) -> bool {
        self.muted.load(Ordering::Relaxed)
    }

    pub fn is_running(&self) -> bool {
        !self.task.is_finished()
    }

    /// The other people in the room: relay index and public key.
    pub fn others(&self) -> Vec<(u8, String)> {
        self.peers
            .lock()
            .map(|peers| {
                peers
                    .iter()
                    .filter(|peer| peer.index != self.index)
                    .map(|peer| (peer.index, peer.pubkey.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Sends `{"type":"leave"}` and waits briefly for the engine to stop.
    pub async fn stop(mut self) {
        if let Some(leave) = self.leave.take() {
            let _ = leave.send(());
        }
        let _ = tokio::time::timeout(Duration::from_secs(2), &mut self.task).await;
        self.task.abort();
    }
}

impl Drop for AudioCall {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Starts the engine on a joined socket. `capture` yields 960-sample frames;
/// mixed 480-sample chunks go to `playback`. Starts muted.
pub fn start(
    joined: Joined,
    mut capture: mpsc::Receiver<Vec<f32>>,
    playback: mpsc::Sender<Vec<f32>>,
) -> Result<AudioCall> {
    let mut encoder = AudioEncoder::new().map_err(Error::Protocol)?;
    let muted = Arc::new(AtomicBool::new(true));
    let peers = Arc::new(Mutex::new(joined.peers.clone()));
    let levels = Arc::new(Levels::default());
    let (leave_tx, mut leave_rx) = tokio::sync::oneshot::channel();
    let (mut sink, mut stream) = joined.socket.split();
    let task_muted = muted.clone();
    let task_peers = peers.clone();
    let task_levels = levels.clone();
    let index = joined.index;
    let task = tokio::spawn(async move {
        let mut mixer = PeerMixer::default();
        let mut playout = tokio::time::interval(Duration::from_millis(10));
        playout.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = &mut leave_rx => {
                    let _ = sink.send(Message::Text(r#"{"type":"leave"}"#.into())).await;
                    let _ = sink.close().await;
                    break;
                }
                frame = capture.recv() => {
                    let Some(frame) = frame else { break };
                    // Before the mute check: the lane shows that the
                    // microphone works while nothing is sent.
                    task_levels.publish_mic(&frame);
                    if task_muted.load(Ordering::Relaxed) || frame.len() != FRAME_SAMPLES {
                        continue;
                    }
                    if let Ok(bytes) = encoder.encode(&frame)
                        && sink.send(Message::Binary(bytes.into())).await.is_err()
                    {
                        break;
                    }
                }
                message = stream.next() => match message {
                    Some(Ok(Message::Binary(bytes))) => mixer.insert(&bytes, index),
                    Some(Ok(Message::Text(text))) => {
                        apply_control(&text, index, &task_peers, &mut mixer);
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(_)) => {}
                },
                _ = playout.tick() => {
                    let Some(mixed) = mixer.tick(&task_levels) else {
                        continue;
                    };
                    if playback.try_send(mixed).is_err() && playback.is_closed() {
                        break;
                    }
                }
            }
        }
    });
    Ok(AudioCall {
        muted,
        peers,
        index,
        levels,
        leave: Some(leave_tx),
        task,
    })
}

/// One jitter buffer per person heard; the 10 ms mix of all of them.
#[derive(Default)]
pub struct PeerMixer {
    buffers: HashMap<u8, PeerJitterBuffer>,
}

impl PeerMixer {
    /// A media frame from the relay (`peer_index` + v2 header + Opus).
    /// Your own echo is ignored.
    pub fn insert(&mut self, bytes: &[u8], own_index: u8) {
        let Some((peer, header, opus)) = parse_relay_frame(bytes) else {
            return;
        };
        if peer == own_index {
            return;
        }
        let buffer = match self.buffers.entry(peer) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => match PeerJitterBuffer::new(peer) {
                Ok(buffer) => entry.insert(buffer),
                Err(_) => return,
            },
        };
        if !header.is_dtx() {
            let _ = buffer.insert_packet(header.seq, header.ts_48k, opus);
        }
    }

    /// Drops a person's decoder state (they left, or their index was given
    /// to someone else), as the audio client's README asks.
    pub fn forget(&mut self, index: u8) {
        self.buffers.remove(&index);
    }

    /// Keeps only the people in `indices`.
    pub fn retain(&mut self, indices: &[u8]) {
        self.buffers.retain(|index, _| indices.contains(index));
    }

    /// One 10 ms playout: each person decoded, their level published before
    /// the mix, then the mix. `None` when nobody is heard.
    pub fn tick(&mut self, levels: &Levels) -> Option<Vec<f32>> {
        if self.buffers.is_empty() {
            return None;
        }
        let mut mixed = vec![0.0_f32; PLAYOUT_SAMPLES];
        for (index, buffer) in &mut self.buffers {
            if let Ok((samples, _)) = buffer.get_audio() {
                levels.publish_peer(*index, &samples);
                for (out, sample) in mixed.iter_mut().zip(samples) {
                    *out += sample;
                }
            }
        }
        for sample in &mut mixed {
            *sample = sample.clamp(-1.0, 1.0);
        }
        // For the spectrum styles, only while one is on screen: atomics in a
        // fixed ring, no lock, no allocation.
        levels.publish_mix(&mixed);
        Some(mixed)
    }
}

/// The relay's control messages during a call: `roster` replaces the room,
/// `joined` adds (or re-seats) one person, `left` removes one. Anything else,
/// or anything malformed, is ignored. Returns whether the room changed.
pub fn apply_control(
    text: &str,
    own_index: u8,
    peers: &Mutex<Vec<Peer>>,
    mixer: &mut PeerMixer,
) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return false;
    };
    let Ok(mut peers) = peers.lock() else {
        return false;
    };
    let one = |value: &serde_json::Value| {
        buzz_audio_client::connection::parse_peers(&serde_json::json!({
            "peers": [{
                "peer_index": value["peer_index"],
                "pubkey": value["pubkey"],
                "epoch": value.get("epoch").cloned().unwrap_or(serde_json::json!(0)),
            }]
        }))
        .ok()
        .and_then(|mut list| list.pop())
    };
    match value["type"].as_str() {
        Some("roster") => {
            let Ok(room) = buzz_audio_client::connection::parse_peers(&value) else {
                return false;
            };
            for peer in room.iter() {
                // An index now held by someone else starts a fresh decoder.
                let reseated = peers
                    .iter()
                    .any(|old| old.index == peer.index && old.pubkey != peer.pubkey);
                if reseated {
                    mixer.forget(peer.index);
                }
            }
            let indices = room.iter().map(|peer| peer.index).collect::<Vec<_>>();
            mixer.retain(&indices);
            *peers = room;
            true
        }
        Some("joined") => {
            let Some(peer) = one(&value) else {
                return false;
            };
            let reseated = peers
                .iter()
                .any(|old| old.index == peer.index && old.pubkey != peer.pubkey);
            if reseated {
                mixer.forget(peer.index);
            }
            peers.retain(|old| old.index != peer.index && old.pubkey != peer.pubkey);
            peers.push(peer);
            peers.sort_by_key(|peer| peer.index);
            true
        }
        Some("left") => {
            let Some(peer) = one(&value) else {
                return false;
            };
            if peer.index == own_index {
                return false;
            }
            mixer.forget(peer.index);
            let before = peers.len();
            peers.retain(|old| old.index != peer.index);
            peers.len() != before
        }
        _ => false,
    }
}

/// Fixed, actionable device errors; device names never appear.
pub const MICROPHONE_DENIED: &str = "microphone unavailable: allow microphone access for your terminal app (macOS: System Settings › Privacy & Security › Microphone), check the input device, then join again";
pub const SPEAKER_UNAVAILABLE: &str =
    "speaker unavailable: check the output device, then join again";

/// Owns cpal streams on a dedicated thread (they are not `Send`). Dropping it
/// stops both streams and releases the devices.
pub struct Devices {
    stop: std::sync::mpsc::Sender<()>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Devices {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// ALSA reports plugin and configuration errors (for example a `pulse`
/// default whose plugin is not installed) by writing to fd 2, on top of the
/// full-screen TUI. Device enumeration and opening run with fd 2 pointed at
/// the null device; it is restored right after.
fn quiet_stderr<T>(operation: impl FnOnce() -> T) -> T {
    #[cfg(unix)]
    {
        use std::os::fd::AsFd as _;
        // Redirection is process-wide: serialize our callers and restore on
        // unwinding too, so a failed device probe cannot leave stderr muted.
        static REDIRECT: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _lock = REDIRECT.lock().unwrap_or_else(|error| error.into_inner());
        struct Restore(Option<rustix::fd::OwnedFd>);
        impl Drop for Restore {
            fn drop(&mut self) {
                if let Some(saved) = &self.0 {
                    let _ = rustix::stdio::dup2_stderr(saved);
                }
            }
        }
        let saved = rustix::io::dup(std::io::stderr().as_fd()).ok();
        let null = std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/null")
            .ok();
        let silenced = match (&saved, &null) {
            (Some(_), Some(null)) => rustix::stdio::dup2_stderr(null).is_ok(),
            _ => false,
        };
        let _restore = Restore(if silenced { saved } else { None });
        operation()
    }
    #[cfg(not(unix))]
    {
        operation()
    }
}

/// Device names for the pickers (local UI only; never logged).
pub fn device_names() -> (Vec<String>, Vec<String>) {
    quiet_stderr(device_names_inner)
}

fn device_names_inner() -> (Vec<String>, Vec<String>) {
    use cpal::traits::{DeviceTrait as _, HostTrait as _};
    let host = cpal::default_host();
    let names = |devices: Option<Vec<cpal::Device>>| {
        devices
            .unwrap_or_default()
            .iter()
            .filter_map(|device| device.name().ok())
            .collect::<Vec<_>>()
    };
    (
        names(host.input_devices().ok().map(Iterator::collect)),
        names(host.output_devices().ok().map(Iterator::collect)),
    )
}

/// Open devices plus the engine's capture receiver and playback sender.
pub type DeviceBridge = (Devices, mpsc::Receiver<Vec<f32>>, mpsc::Sender<Vec<f32>>);

/// Opens input and output (by name, else the defaults) and bridges them to
/// the engine's channels, converting to 48 kHz mono f32.
pub fn open_devices(input: Option<&str>, output: Option<&str>) -> Result<DeviceBridge> {
    let (capture_tx, capture_rx) = mpsc::channel::<Vec<f32>>(64);
    let (playback_tx, playback_rx) = mpsc::channel::<Vec<f32>>(64);
    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<()>>();
    let input = input.map(str::to_owned);
    let output = output.map(str::to_owned);
    let thread = std::thread::Builder::new()
        .name("bzz-huddle-audio".into())
        .spawn(move || {
            match quiet_stderr(|| {
                device_streams(input.as_deref(), output.as_deref(), capture_tx, playback_rx)
            }) {
                Ok(streams) => {
                    let _ = ready_tx.send(Ok(()));
                    let _ = stop_rx.recv();
                    drop(streams);
                }
                Err(error) => {
                    let _ = ready_tx.send(Err(error));
                }
            }
        })
        .map_err(|error| Error::io("audio thread", error))?;
    match ready_rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(())) => Ok((
            Devices {
                stop: stop_tx,
                thread: Some(thread),
            },
            capture_rx,
            playback_tx,
        )),
        Ok(Err(error)) => {
            let _ = thread.join();
            Err(error)
        }
        Err(_) => Err(Error::Unsupported(MICROPHONE_DENIED.into())),
    }
}

fn device_streams(
    input: Option<&str>,
    output: Option<&str>,
    capture: mpsc::Sender<Vec<f32>>,
    playback: mpsc::Receiver<Vec<f32>>,
) -> Result<(cpal::Stream, cpal::Stream)> {
    use cpal::traits::{DeviceTrait as _, HostTrait as _, StreamTrait as _};
    let host = cpal::default_host();
    let find = |devices: Option<Vec<cpal::Device>>, name: &str| {
        devices.and_then(|devices| {
            devices
                .into_iter()
                .find(|device| device.name().is_ok_and(|n| n == name))
        })
    };
    // With no device configured: the system default when it actually
    // opens, else the first device that does (`pipewire` is tried first, as
    // a broken `default` is usually a stale `~/.asoundrc`).
    let usable_input = |device: &cpal::Device| device.default_input_config().is_ok();
    let usable_output = |device: &cpal::Device| device.default_output_config().is_ok();
    let fallback = |devices: Option<Vec<cpal::Device>>, usable: &dyn Fn(&cpal::Device) -> bool| {
        let mut devices = devices.unwrap_or_default();
        devices.sort_by_key(|device| device.name().map_or(true, |name| name != "pipewire"));
        devices.into_iter().find(|device| usable(device))
    };
    let mic = match input {
        Some(name) => find(host.input_devices().ok().map(Iterator::collect), name),
        None => host
            .default_input_device()
            .filter(|device| usable_input(device))
            .or_else(|| {
                fallback(
                    host.input_devices().ok().map(Iterator::collect),
                    &usable_input,
                )
            }),
    }
    .ok_or_else(|| Error::Unsupported(MICROPHONE_DENIED.into()))?;
    let speaker = match output {
        Some(name) => find(host.output_devices().ok().map(Iterator::collect), name),
        None => host
            .default_output_device()
            .filter(|device| usable_output(device))
            .or_else(|| {
                fallback(
                    host.output_devices().ok().map(Iterator::collect),
                    &usable_output,
                )
            }),
    }
    .ok_or_else(|| Error::Unsupported(SPEAKER_UNAVAILABLE.into()))?;
    let input_config = mic
        .default_input_config()
        .map_err(|_| Error::Unsupported(MICROPHONE_DENIED.into()))?;
    let output_config = speaker
        .default_output_config()
        .map_err(|_| Error::Unsupported(SPEAKER_UNAVAILABLE.into()))?;

    let in_rate = input_config.sample_rate().0;
    let in_channels = usize::from(input_config.channels());
    let mut framer = Framer::new(in_rate);
    let input_stream = mic
        .build_input_stream(
            &input_config.config(),
            move |data: &[f32], _| {
                for frame in data.chunks(in_channels.max(1)) {
                    let mono = frame.iter().sum::<f32>() / frame.len() as f32;
                    if let Some(chunk) = framer.push(mono) {
                        let _ = capture.try_send(chunk);
                    }
                }
            },
            |_| {},
            None,
        )
        .map_err(|_| Error::Unsupported(MICROPHONE_DENIED.into()))?;

    let out_rate = output_config.sample_rate().0;
    let out_channels = usize::from(output_config.channels());
    let mut playback = playback;
    let mut pending: std::collections::VecDeque<f32> = std::collections::VecDeque::new();
    let step = 48_000.0 / f64::from(out_rate);
    let mut position = 0.0_f64;
    let output_stream = speaker
        .build_output_stream(
            &output_config.config(),
            move |data: &mut [f32], _| {
                for frame in data.chunks_mut(out_channels.max(1)) {
                    while pending.len() < 2 {
                        match playback.try_recv() {
                            Ok(chunk) => pending.extend(chunk),
                            Err(_) => break,
                        }
                    }
                    let sample = pending.front().copied().unwrap_or(0.0);
                    frame.fill(sample);
                    position += step;
                    while position >= 1.0 {
                        position -= 1.0;
                        pending.pop_front();
                    }
                }
            },
            |_| {},
            None,
        )
        .map_err(|_| Error::Unsupported(SPEAKER_UNAVAILABLE.into()))?;
    input_stream
        .play()
        .map_err(|_| Error::Unsupported(MICROPHONE_DENIED.into()))?;
    output_stream
        .play()
        .map_err(|_| Error::Unsupported(SPEAKER_UNAVAILABLE.into()))?;
    Ok((input_stream, output_stream))
}

/// Resamples (nearest) device-rate mono to 48 kHz and cuts 960-sample frames.
pub struct Framer {
    step: f64,
    position: f64,
    frame: Vec<f32>,
}

impl Framer {
    pub fn new(device_rate: u32) -> Self {
        Self {
            step: 48_000.0 / f64::from(device_rate.max(1)),
            position: 0.0,
            frame: Vec::with_capacity(FRAME_SAMPLES),
        }
    }

    /// Feeds one device-rate sample; returns a full frame when one is ready.
    pub fn push(&mut self, sample: f32) -> Option<Vec<f32>> {
        self.position += self.step;
        let mut ready = None;
        while self.position >= 1.0 {
            self.position -= 1.0;
            self.frame.push(sample);
            if self.frame.len() == FRAME_SAMPLES {
                ready = Some(std::mem::replace(
                    &mut self.frame,
                    Vec::with_capacity(FRAME_SAMPLES),
                ));
            }
        }
        ready
    }
}

#[cfg(test)]
mod tests {
    use super::{Framer, Levels, PeerMixer, apply_control};
    use crate::huddle::{meter::Meter, synth};
    use buzz_audio_client::{connection::Peer, encoder::AudioEncoder};
    use std::sync::Mutex;

    fn key(c: char) -> String {
        c.to_string().repeat(64)
    }

    #[test]
    fn the_relays_control_messages_keep_every_name() {
        let peers = Mutex::new(vec![
            Peer {
                index: 0,
                pubkey: key('a'),
                epoch: 0,
            },
            Peer {
                index: 1,
                pubkey: key('b'),
                epoch: 0,
            },
        ]);
        let mut mixer = PeerMixer::default();
        // Someone joins after you: they have a name.
        let joined = format!(
            r#"{{"type":"joined","revision":2,"pubkey":"{}","peer_index":2,"epoch":0,"peers":[]}}"#,
            key('c')
        );
        assert!(apply_control(&joined, 0, &peers, &mut mixer));
        let names = |peers: &Mutex<Vec<Peer>>| {
            peers
                .lock()
                .unwrap()
                .iter()
                .map(|peer| (peer.index, peer.pubkey.chars().next().unwrap()))
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&peers), vec![(0, 'a'), (1, 'b'), (2, 'c')]);
        // Someone leaves.
        let left = format!(
            r#"{{"type":"left","revision":3,"pubkey":"{}","peer_index":1,"epoch":0}}"#,
            key('b')
        );
        assert!(apply_control(&left, 0, &peers, &mut mixer));
        assert_eq!(names(&peers), vec![(0, 'a'), (2, 'c')]);
        // A full roster replaces the room; a seat taken by someone new.
        let roster = format!(
            r#"{{"type":"roster","revision":4,"peers":[{{"pubkey":"{}","peer_index":0,"epoch":0}},{{"pubkey":"{}","peer_index":2,"epoch":1}}]}}"#,
            key('a'),
            key('d')
        );
        assert!(apply_control(&roster, 0, &peers, &mut mixer));
        assert_eq!(names(&peers), vec![(0, 'a'), (2, 'd')]);
        // Malformed or unknown messages change nothing.
        for text in [
            "not json",
            r#"{"type":"ping"}"#,
            r#"{"type":"joined","pubkey":"short","peer_index":5}"#,
            r#"{"type":"roster","peers":[{"pubkey":"x","peer_index":900}]}"#,
        ] {
            assert!(!apply_control(text, 0, &peers, &mut mixer), "{text}");
        }
        assert_eq!(names(&peers), vec![(0, 'a'), (2, 'd')]);
    }

    /// 1 kHz at −20 dBFS through the real path: Opus encoder, relay framing,
    /// NetEQ and Opus decoder, the level taken before the mix, the meter at
    /// 30 frames a second. The gate opens once the jitter buffer plays the
    /// tone and closes 300 to 400 ms after it stops playing.
    #[test]
    fn a_tone_through_the_decode_path_opens_and_closes_the_gate() {
        let mut encoder = AudioEncoder::new().unwrap();
        let mut mixer = PeerMixer::default();
        let levels = Levels::default();
        let mut meter = Meter::default();
        let (tone_from, tone_to) = (0.5_f64, 1.5_f64);
        let mut opened = None;
        let mut closed = None;
        let mut next_frame = 0.0;
        // 10 ms steps; a 20 ms frame is sent every other step.
        for step in 0..300_u64 {
            let t = step as f64 / 100.0;
            if step % 2 == 0 {
                let sample = step * 480;
                let frame = if (tone_from..tone_to).contains(&t) {
                    synth::tone(1000.0, -20.0, sample, 960)
                } else {
                    vec![0.0; 960]
                };
                let mut relayed = vec![7_u8];
                relayed.extend(encoder.encode(&frame).unwrap());
                mixer.insert(&relayed, 0);
            }
            let _ = mixer.tick(&levels);
            let t_end = (step + 1) as f64 / 100.0;
            while next_frame <= t_end + 1e-9 {
                meter.update(&levels, &[7], next_frame);
                let speaking = meter.peer(7).unwrap().speaking();
                if speaking && opened.is_none() {
                    opened = Some(next_frame);
                }
                if !speaking && opened.is_some() && closed.is_none() {
                    closed = Some(next_frame);
                }
                next_frame += 1.0 / 30.0;
            }
        }
        let opened = opened.expect("the gate opened") - tone_from;
        let closed = closed.expect("the gate closed") - tone_to;
        println!("gate opened {opened:.3} s after the tone, closed {closed:.3} s after it");
        // The jitter buffer holds 40 to 200 ms, a frame is 33 ms: the gate
        // opens within 250 ms of the tone, and closes 300 to 450 ms after the
        // tone stops playing (the hang, then up to a frame and a chunk).
        assert!((0.0..=0.25).contains(&opened), "opened {opened}");
        let played_for = closed - opened;
        assert!((0.3..=0.45).contains(&played_for), "closed {closed}");
    }

    #[test]
    fn framer_resamples_to_48k_frames() {
        for rate in [44_100_u32, 48_000, 16_000, 96_000] {
            let mut framer = Framer::new(rate);
            let frames = (0..rate).filter_map(|_| framer.push(0.5)).count();
            // One second of input is fifty 20 ms frames (±1 for rounding).
            assert!((49..=50).contains(&frames), "{rate}: {frames}");
        }
    }
}
