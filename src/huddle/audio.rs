//! Live huddle audio (feature `huddles-audio`).
//!
//! Transport: `buzz-audio-client` (v2 handshake, Opus, NetEQ jitter buffer).
//! The engine moves PCM through channels so devices are a separate layer:
//! capture sends 20 ms frames (960 mono f32 samples at 48 kHz), playback
//! receives mixed 10 ms chunks (480 samples). Nothing here logs audio, levels
//! or device names.

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
        url.as_str(),
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
    pub peers: Arc<Mutex<Vec<Peer>>>,
    pub index: u8,
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
    let (leave_tx, mut leave_rx) = tokio::sync::oneshot::channel();
    let (mut sink, mut stream) = joined.socket.split();
    let task_muted = muted.clone();
    let index = joined.index;
    let task = tokio::spawn(async move {
        let mut buffers: HashMap<u8, PeerJitterBuffer> = HashMap::new();
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
                    Some(Ok(Message::Binary(bytes))) => {
                        if let Some((peer, header, opus)) = parse_relay_frame(&bytes)
                            && peer != index
                        {
                            let buffer = match buffers.entry(peer) {
                                std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                                std::collections::hash_map::Entry::Vacant(entry) => {
                                    match PeerJitterBuffer::new(peer) {
                                        Ok(buffer) => entry.insert(buffer),
                                        Err(_) => continue,
                                    }
                                }
                            };
                            if !header.is_dtx() {
                                let _ = buffer.insert_packet(header.seq, header.ts_48k, opus);
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(_)) => {}
                },
                _ = playout.tick() => {
                    if buffers.is_empty() {
                        continue;
                    }
                    let mut mixed = vec![0.0_f32; PLAYOUT_SAMPLES];
                    for buffer in buffers.values_mut() {
                        if let Ok((samples, _)) = buffer.get_audio() {
                            for (out, sample) in mixed.iter_mut().zip(samples) {
                                *out += sample;
                            }
                        }
                    }
                    for sample in &mut mixed {
                        *sample = sample.clamp(-1.0, 1.0);
                    }
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
        leave: Some(leave_tx),
        task,
    })
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
    use super::Framer;

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
