//! Linux huddle devices through the PulseAudio protocol, in pure Rust (the
//! `pulseaudio` crate). PulseAudio answers it, and so does PipeWire through
//! pipewire-pulse, so the binary names no sound library: bzz starts on a
//! machine without one, and a huddle needs only a sound server in the
//! session. The server converts rate and channels; both streams are 48 kHz
//! mono f32, the engine's own format.
//!
//! What it gives up (ADR v0.13 huddle audio, 2026-11-25): a machine with ALSA
//! and no sound server has no huddle audio, and joining says so.

use std::{
    collections::{BTreeSet, VecDeque},
    ffi::{CStr, CString},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

use buzz_audio_client::encoder::FRAME_SAMPLES;
use pulseaudio::{Client, protocol};
use tokio::sync::mpsc;

use super::{DeviceBridge, Devices, MICROPHONE_DENIED, NO_SOUND_SERVER, SPEAKER_UNAVAILABLE};
use crate::error::{Error, Result};

const SPEC: protocol::SampleSpec = protocol::SampleSpec {
    format: protocol::SampleFormat::Float32Le,
    channels: 1,
    sample_rate: 48_000,
};

/// Bytes of `ms` milliseconds of 48 kHz mono f32.
const fn bytes_of_ms(ms: u32) -> u32 {
    48 * ms * 4
}

/// Playback asks the server to hold 20 ms and to request 10 ms at a time;
/// capture hands over 10 ms fragments; 10 ms of the engine's output is held
/// back before playing starts. The engine plays out every 10 ms, so this is
/// about the least that does not run dry at its pace. Measured with T7 in
/// containers (docs/validation-v0.17.0/sound.md): a round trip of 13 to
/// 26 ms through PipeWire, against 37 to 42 ms with the server's defaults
/// for the rest and silence handed over when the engine was late; 10 ms and
/// 5 ms requests were as fast but dropped audio every few hundred ms.
const PLAYBACK_TARGET: u32 = bytes_of_ms(20);
const PLAYBACK_REQUEST: u32 = bytes_of_ms(10);
const CAPTURE_FRAGMENT: u32 = bytes_of_ms(10);
/// Samples held back before playing (10 ms, one chunk of the engine).
const PLAYOUT_CUSHION: usize = 480;

/// Where the server's socket may be, in libpulse's order. `PULSE_SERVER`,
/// when set, is the only place looked at (libpulse does not fall back
/// either): its local entries, `unix:/path` or `/path`, each optionally
/// after a `{machine-id}`; network entries are skipped, bzz speaks only to a
/// local server. Otherwise the per-user socket in the runtime directory
/// (`PULSE_RUNTIME_PATH`, else `XDG_RUNTIME_DIR/pulse`, else
/// `/run/user/<uid>/pulse`), then the system-wide one.
///
/// The `pulseaudio` crate's own lookup appends `pulse/native` to
/// `PULSE_RUNTIME_PATH` and stops at the first missing candidate; under the
/// PulseAudio setup CI uses that finds nothing, so bzz looks itself.
fn socket_candidates(env: impl Fn(&str) -> Option<String>, uid: u32) -> Vec<PathBuf> {
    let set = |name: &str| env(name).filter(|value| !value.trim().is_empty());
    if let Some(servers) = set("PULSE_SERVER") {
        return servers
            .split_whitespace()
            .filter_map(|entry| {
                let entry = match entry.strip_prefix('{') {
                    Some(rest) => rest.split_once('}')?.1,
                    None => entry,
                };
                let path = entry.strip_prefix("unix:").unwrap_or(entry);
                path.starts_with('/').then(|| PathBuf::from(path))
            })
            .collect();
    }
    let runtime = set("PULSE_RUNTIME_PATH")
        .map(PathBuf::from)
        .or_else(|| set("XDG_RUNTIME_DIR").map(|dir| Path::new(&dir).join("pulse")))
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{uid}/pulse")));
    vec![
        runtime.join("native"),
        PathBuf::from("/run/pulse/native"),
        PathBuf::from("/var/run/pulse/native"),
    ]
}

/// The authentication cookies, where libpulse looks for them, each found
/// once. PipeWire ignores them. PulseAudio wants its own unless the socket
/// allows anonymous clients (libpulse also proves the user id over the
/// socket, which this crate does not), and a daemon may keep its cookie in
/// another of these places than the one `PULSE_COOKIE` names.
fn cookies() -> Vec<Vec<u8>> {
    let env = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
    let home = env("HOME").map(PathBuf::from);
    [
        env("PULSE_COOKIE").map(PathBuf::from),
        env("XDG_CONFIG_HOME").map(|dir| Path::new(&dir).join("pulse/cookie")),
        home.as_ref().map(|home| home.join(".config/pulse/cookie")),
        home.map(|home| home.join(".pulse-cookie")),
    ]
    .into_iter()
    .flatten()
    .filter_map(|path| std::fs::read(path).ok())
    .fold(Vec::new(), |mut found, cookie| {
        if !found.contains(&cookie) {
            found.push(cookie);
        }
        found
    })
}

fn connect() -> Result<Client> {
    let uid = rustix::process::getuid().as_raw();
    connect_to(socket_candidates(|name| std::env::var(name).ok(), uid))
}

/// The first candidate that accepts the handshake, else the one line that
/// says what is missing.
fn connect_to(candidates: Vec<PathBuf>) -> Result<Client> {
    let mut cookies: Vec<Option<Vec<u8>>> = cookies().into_iter().map(Some).collect();
    if cookies.is_empty() {
        cookies.push(None);
    }
    for path in candidates {
        for cookie in &cookies {
            let Ok(socket) = UnixStream::connect(&path) else {
                break;
            };
            // The handshake blocks; a hung server must not hang the join.
            let _ = socket.set_read_timeout(Some(Duration::from_secs(2)));
            let _ = socket.set_write_timeout(Some(Duration::from_secs(2)));
            // A refused cookie: the next one.
            if let Ok(client) = Client::new_unix(c"bzz", socket, cookie.as_deref()) {
                return Ok(client);
            }
        }
    }
    Err(Error::Unsupported(NO_SOUND_SERVER.into()))
}

fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .map_err(|error| Error::io("audio runtime", error))
}

/// One source or sink as bzz matches a configured name against it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Endpoint {
    name: String,
    description: Option<String>,
    /// The ALSA card and device behind it, when the server says.
    card: Option<u32>,
    device: Option<u32>,
    /// A monitor of an output: never offered as a microphone.
    monitor: bool,
}

impl Endpoint {
    fn new(
        name: &CStr,
        description: Option<&CStr>,
        props: &protocol::Props,
        monitor: bool,
    ) -> Self {
        let number = |keys: &[&CStr]| {
            keys.iter().find_map(|key| {
                std::str::from_utf8(props.get_bytes(*key)?)
                    .ok()?
                    .trim_end_matches('\0')
                    .parse()
                    .ok()
            })
        };
        Self {
            name: name.to_string_lossy().into_owned(),
            description: description.map(|text| text.to_string_lossy().into_owned()),
            card: number(&[c"alsa.card", c"api.alsa.pcm.card", c"api.alsa.card"]),
            device: number(&[c"alsa.device", c"api.alsa.pcm.device"]),
            monitor,
        }
    }

    /// What the pickers show and `:huddle input` accepts besides the name.
    fn label(&self) -> &str {
        self.description
            .as_deref()
            .filter(|text| !text.is_empty())
            .unwrap_or(&self.name)
    }
}

async fn sources(client: &Client) -> Option<Vec<Endpoint>> {
    let sources = client.list_sources().await.ok()?;
    Some(
        sources
            .iter()
            .map(|source| {
                Endpoint::new(
                    &source.name,
                    source.description.as_deref(),
                    &source.props,
                    source.monitor_of_sink_index.is_some(),
                )
            })
            .collect(),
    )
}

async fn sinks(client: &Client) -> Option<Vec<Endpoint>> {
    let sinks = client.list_sinks().await.ok()?;
    Some(
        sinks
            .iter()
            .map(|sink| Endpoint::new(&sink.name, sink.description.as_deref(), &sink.props, false))
            .collect(),
    )
}

/// Which device to open for a configured name.
#[derive(Debug, PartialEq, Eq)]
enum Choice {
    /// The server's default device.
    Default,
    /// This server name.
    Named(String),
    /// Nothing matches: the default is opened and the user is told once.
    Missing,
}

/// ALSA names that only ever meant "the system's default device". v0.16.0
/// saved names from ALSA's own list (`cpal`), and these lead to the default
/// of whichever sound server answers now.
const ALSA_DEFAULTS: &[&str] = &["default", "pipewire", "pulse", "sysdefault", "jack"];

/// Matches a configured name: a server name, then a label, then a v0.16.0
/// ALSA name (`hw:CARD=PCH,DEV=0`, `plughw:1,0`, `sysdefault:CARD=USB`...)
/// mapped to the source or sink on that card and device.
fn resolve(
    wanted: Option<&str>,
    endpoints: &[Endpoint],
    card_index: impl Fn(&str) -> Option<u32>,
) -> Choice {
    let Some(wanted) = wanted.map(str::trim).filter(|name| !name.is_empty()) else {
        return Choice::Default;
    };
    if let Some(endpoint) = endpoints
        .iter()
        .find(|endpoint| endpoint.name == wanted)
        .or_else(|| endpoints.iter().find(|endpoint| endpoint.label() == wanted))
    {
        return Choice::Named(endpoint.name.clone());
    }
    let (kind, address) = wanted.split_once(':').unwrap_or((wanted, ""));
    if address.is_empty() && ALSA_DEFAULTS.contains(&kind) {
        return Choice::Default;
    }
    let Some((card, device)) = alsa_address(address, card_index) else {
        return Choice::Missing;
    };
    endpoints
        .iter()
        .filter(|endpoint| !endpoint.monitor && endpoint.card == Some(card))
        .find(|endpoint| device.is_none() || endpoint.device == device)
        .map_or(Choice::Missing, |endpoint| {
            Choice::Named(endpoint.name.clone())
        })
}

/// The card and device of an ALSA PCM address: `CARD=PCH,DEV=0`, `0,0`, `1`.
fn alsa_address(
    address: &str,
    card_index: impl Fn(&str) -> Option<u32>,
) -> Option<(u32, Option<u32>)> {
    let mut card = None;
    let mut device = None;
    for (position, part) in address.split(',').enumerate() {
        let (key, value) = match part.split_once('=') {
            Some((key, value)) => (key.trim(), value.trim()),
            None if position == 0 => ("CARD", part.trim()),
            None if position == 1 => ("DEV", part.trim()),
            None => continue,
        };
        match key {
            "CARD" => card = value.parse().ok().or_else(|| card_index(value)),
            "DEV" => device = value.parse().ok(),
            _ => {}
        }
    }
    Some((card?, device))
}

/// An ALSA card id (`PCH`, `USB`) to its index: `/proc/asound/<id>` links to
/// `card<N>`. Read only; no sound device is opened.
fn proc_card_index(id: &str) -> Option<u32> {
    if id.is_empty() || id.contains('/') {
        return None;
    }
    let target = std::fs::read_link(Path::new("/proc/asound").join(id)).ok()?;
    target.to_str()?.strip_prefix("card")?.parse().ok()
}

/// A missing saved device is told once in a run, not at every join.
fn first_time_missing(kind: &str, name: &str) -> bool {
    static TOLD: Mutex<BTreeSet<(String, String)>> = Mutex::new(BTreeSet::new());
    TOLD.lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert((kind.to_owned(), name.to_owned()))
}

/// Device labels for the pickers (local UI only; never logged). Monitors of
/// outputs are left out: nobody means them as a microphone. With no sound
/// server both lists are empty.
pub fn device_names() -> (Vec<String>, Vec<String>) {
    let (Ok(client), Ok(runtime)) = (connect(), runtime()) else {
        return (Vec::new(), Vec::new());
    };
    let labels = |endpoints: Option<Vec<Endpoint>>| {
        endpoints
            .unwrap_or_default()
            .iter()
            .filter(|endpoint| !endpoint.monitor)
            .map(|endpoint| endpoint.label().to_owned())
            .collect()
    };
    runtime.block_on(async { (labels(sources(&client).await), labels(sinks(&client).await)) })
}

/// Opens input and output (by name, else the server's defaults) and bridges
/// them to the engine's channels.
pub fn open_devices(input: Option<&str>, output: Option<&str>) -> Result<DeviceBridge> {
    let (capture_tx, capture_rx) = mpsc::channel::<Vec<f32>>(64);
    let (playback_tx, playback_rx) = mpsc::channel::<Vec<f32>>(64);
    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<Option<String>>>();
    let input = input.map(str::to_owned);
    let output = output.map(str::to_owned);
    let thread = std::thread::Builder::new()
        .name("bzz-huddle-audio".into())
        .spawn(move || {
            match streams(input.as_deref(), output.as_deref(), capture_tx, playback_rx) {
                Ok((streams, notice)) => {
                    let _ = ready_tx.send(Ok(notice));
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
        Ok(Ok(notice)) => Ok((
            Devices {
                stop: stop_tx,
                thread: Some(thread),
                notice,
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

/// Keeps the connection and both streams alive; dropping it deletes them on
/// the server and closes the capture channel.
struct Streams {
    _playback: pulseaudio::PlaybackStream,
    _record: pulseaudio::RecordStream,
    _client: Client,
}

/// The server name to open, and the notice for a saved name not found.
fn pick(
    kind: &str,
    wanted: Option<&str>,
    endpoints: Option<Vec<Endpoint>>,
    default: &CStr,
) -> (CString, Option<String>) {
    let endpoints = endpoints.unwrap_or_default();
    match resolve(wanted, &endpoints, proc_card_index) {
        Choice::Named(name) => match CString::new(name) {
            Ok(name) => (name, None),
            Err(_) => (default.to_owned(), None),
        },
        Choice::Default => (default.to_owned(), None),
        Choice::Missing => {
            let name = wanted.unwrap_or_default().trim();
            let notice = first_time_missing(kind, name).then(|| {
                format!(
                    "saved {kind} device \"{name}\" not found; using the default (:huddle devices)"
                )
            });
            (default.to_owned(), notice)
        }
    }
}

fn streams(
    input: Option<&str>,
    output: Option<&str>,
    capture: mpsc::Sender<Vec<f32>>,
    playback: mpsc::Receiver<Vec<f32>>,
) -> Result<(Streams, Option<String>)> {
    let client = connect()?;
    let runtime = runtime()?;
    runtime.block_on(async move {
        // pipewire-pulse falls back to its default for an unknown name
        // without saying so; bzz matches the name itself and says so.
        let (source_name, input_notice) = match input {
            Some(_) => pick(
                "input",
                input,
                sources(&client).await,
                protocol::DEFAULT_SOURCE,
            ),
            None => (protocol::DEFAULT_SOURCE.to_owned(), None),
        };
        let (sink_name, output_notice) = match output {
            Some(_) => pick(
                "output",
                output,
                sinks(&client).await,
                protocol::DEFAULT_SINK,
            ),
            None => (protocol::DEFAULT_SINK.to_owned(), None),
        };
        let notice = match (input_notice, output_notice) {
            (Some(input), Some(output)) => Some(format!("{input}; {output}")),
            (input, output) => input.or(output),
        };
        let flags = protocol::stream::StreamFlags {
            adjust_latency: true,
            ..Default::default()
        };
        let playback_params = protocol::PlaybackStreamParams {
            sample_spec: SPEC,
            channel_map: protocol::ChannelMap::mono(),
            sink_name: Some(sink_name),
            buffer_attr: protocol::stream::BufferAttr {
                max_length: u32::MAX,
                target_length: PLAYBACK_TARGET,
                pre_buffering: u32::MAX,
                minimum_request_length: PLAYBACK_REQUEST,
                fragment_size: u32::MAX,
            },
            flags,
            props: stream_props(c"bzz huddle playback"),
            ..Default::default()
        };
        let record_params = protocol::RecordStreamParams {
            sample_spec: SPEC,
            channel_map: protocol::ChannelMap::mono(),
            source_name: Some(source_name),
            buffer_attr: protocol::stream::BufferAttr {
                max_length: u32::MAX,
                target_length: u32::MAX,
                pre_buffering: u32::MAX,
                minimum_request_length: u32::MAX,
                fragment_size: CAPTURE_FRAGMENT,
            },
            flags,
            props: stream_props(c"bzz huddle microphone"),
            ..Default::default()
        };

        let mut framer = ByteFramer::default();
        let take = move |data: &[u8]| {
            framer.feed(data, |frame| {
                let _ = capture.try_send(frame);
            });
        };

        let playback = client
            .create_playback_stream(playback_params, Playout::new(playback))
            .await
            .map_err(|_| Error::Unsupported(SPEAKER_UNAVAILABLE.into()))?;
        let record = client
            .create_record_stream(record_params, take)
            .await
            .map_err(|_| Error::Unsupported(MICROPHONE_DENIED.into()))?;
        Ok((
            Streams {
                _playback: playback,
                _record: record,
                _client: client,
            },
            notice,
        ))
    })
}

/// The engine's mixed chunks as the server asks for them. When the engine has
/// not sent the next chunk yet, the server waits for it (the source is
/// pending and the channel wakes it) instead of being handed silence: the
/// server's buffer and the samples held back here are the cushion against
/// the engine's 10 ms clock, and only if both run dry is there a gap.
struct Playout {
    chunks: mpsc::Receiver<Vec<f32>>,
    pending: VecDeque<f32>,
    /// Samples held back before playing starts, and again after the engine
    /// fell behind: the cushion against its clock.
    cushion: usize,
    primed: bool,
}

impl Playout {
    fn new(chunks: mpsc::Receiver<Vec<f32>>) -> Self {
        Self {
            chunks,
            pending: VecDeque::with_capacity(4 * 480),
            cushion: PLAYOUT_CUSHION,
            primed: false,
        }
    }
}

impl pulseaudio::PlaybackSource for Playout {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<usize> {
        use std::task::Poll;
        let this = self.get_mut();
        if !this.primed {
            loop {
                match this.chunks.poll_recv(cx) {
                    Poll::Ready(Some(chunk)) => this.pending.extend(chunk),
                    Poll::Ready(None) if this.pending.is_empty() => return Poll::Ready(0),
                    Poll::Ready(None) => break,
                    Poll::Pending if this.pending.len() < this.cushion => return Poll::Pending,
                    Poll::Pending => break,
                }
            }
            this.primed = true;
        }
        let (samples, _) = buf.as_chunks_mut::<4>();
        let mut written = 0;
        for bytes in samples.iter_mut() {
            while this.pending.is_empty() {
                match this.chunks.poll_recv(cx) {
                    Poll::Ready(Some(chunk)) => this.pending.extend(chunk),
                    // The engine is gone: what is written is the last, then
                    // the end of the stream.
                    Poll::Ready(None) => return Poll::Ready(written * 4),
                    Poll::Pending if written == 0 => {
                        this.primed = this.cushion == 0;
                        return Poll::Pending;
                    }
                    Poll::Pending => return Poll::Ready(written * 4),
                }
            }
            if let Some(sample) = this.pending.pop_front() {
                *bytes = sample.to_le_bytes();
                written += 1;
            }
        }
        Poll::Ready(written * 4)
    }
}

/// Cuts the server's little-endian f32 bytes, which arrive in any size, into
/// the engine's 960-sample frames.
#[derive(Default)]
struct ByteFramer {
    carry: Vec<u8>,
    frame: Vec<f32>,
}

impl ByteFramer {
    fn feed(&mut self, data: &[u8], mut ready: impl FnMut(Vec<f32>)) {
        let mut push = |frame: &mut Vec<f32>, sample: f32| {
            frame.push(sample);
            if frame.len() == FRAME_SAMPLES {
                ready(std::mem::replace(frame, Vec::with_capacity(FRAME_SAMPLES)));
            }
        };
        let mut rest = data;
        if !self.carry.is_empty() {
            let (head, tail) = rest.split_at((4 - self.carry.len()).min(rest.len()));
            self.carry.extend_from_slice(head);
            rest = tail;
            if let Ok(bytes) = <[u8; 4]>::try_from(self.carry.as_slice()) {
                push(&mut self.frame, f32::from_le_bytes(bytes));
                self.carry.clear();
            }
        }
        let (samples, remainder) = rest.as_chunks::<4>();
        for bytes in samples {
            push(&mut self.frame, f32::from_le_bytes(*bytes));
        }
        self.carry.extend_from_slice(remainder);
    }
}

fn stream_props(name: &CStr) -> protocol::Props {
    let mut props = protocol::Props::new();
    props.set(protocol::Prop::MediaName, name);
    props.set(protocol::Prop::MediaRole, c"phone");
    props
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        }
    }

    fn paths(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn the_socket_is_looked_for_where_libpulse_looks() {
        // CI's PulseAudio: a private runtime path, its socket `native` in it
        // (the crate's own lookup tries `<path>/pulse/native`).
        assert_eq!(
            socket_candidates(
                env(&[
                    ("PULSE_RUNTIME_PATH", "/tmp/w/pulse-runtime"),
                    ("XDG_RUNTIME_DIR", "/run/user/1000")
                ]),
                1000
            )[0],
            PathBuf::from("/tmp/w/pulse-runtime/native")
        );
        // A desktop session.
        assert_eq!(
            socket_candidates(env(&[("XDG_RUNTIME_DIR", "/run/user/1000")]), 1000),
            paths(&[
                "/run/user/1000/pulse/native",
                "/run/pulse/native",
                "/var/run/pulse/native"
            ])
        );
        // No runtime directory in the environment (cron, sudo -u).
        assert_eq!(
            socket_candidates(env(&[]), 1001)[0],
            PathBuf::from("/run/user/1001/pulse/native")
        );
        // An empty value is no value.
        assert_eq!(
            socket_candidates(env(&[("PULSE_SERVER", " "), ("XDG_RUNTIME_DIR", "/r")]), 1)[0],
            PathBuf::from("/r/pulse/native")
        );
    }

    #[test]
    fn pulse_server_is_the_only_place_when_set() {
        // WSLg.
        assert_eq!(
            socket_candidates(
                env(&[
                    ("PULSE_SERVER", "unix:/mnt/wslg/PulseServer"),
                    ("XDG_RUNTIME_DIR", "/r")
                ]),
                1
            ),
            paths(&["/mnt/wslg/PulseServer"])
        );
        // A list, a bare path, a machine-id prefix; network servers skipped.
        assert_eq!(
            socket_candidates(
                env(&[(
                    "PULSE_SERVER",
                    "tcp:host:4713 {abc}unix:/a/native /b/native host"
                )]),
                1
            ),
            paths(&["/a/native", "/b/native"])
        );
        // Only network servers: nothing local to speak to, and no fallback
        // to the session's socket (a test harness relies on this).
        assert!(
            socket_candidates(
                env(&[("PULSE_SERVER", "tcp:host"), ("XDG_RUNTIME_DIR", "/r")]),
                1
            )
            .is_empty()
        );
    }

    fn endpoint(name: &str, description: &str, card: Option<u32>, device: Option<u32>) -> Endpoint {
        Endpoint {
            name: name.into(),
            description: Some(description.into()),
            card,
            device,
            monitor: false,
        }
    }

    fn desktop() -> Vec<Endpoint> {
        vec![
            Endpoint {
                monitor: true,
                ..endpoint(
                    "alsa_output.pci.analog-stereo.monitor",
                    "Monitor of Built-in Audio",
                    Some(0),
                    Some(0),
                )
            },
            endpoint(
                "alsa_input.pci.analog-stereo",
                "Built-in Audio Analog Stereo",
                Some(0),
                Some(0),
            ),
            endpoint(
                "alsa_input.usb-Headset.mono",
                "USB Headset Mono",
                Some(1),
                Some(0),
            ),
            endpoint("bluez_input.headset", "Earbuds", None, None),
        ]
    }

    fn cards(id: &str) -> Option<u32> {
        match id {
            "PCH" => Some(0),
            "Headset" => Some(1),
            _ => None,
        }
    }

    #[test]
    fn a_configured_name_matches_a_server_name_or_a_label() {
        let devices = desktop();
        assert_eq!(resolve(None, &devices, cards), Choice::Default);
        assert_eq!(resolve(Some(" "), &devices, cards), Choice::Default);
        assert_eq!(
            resolve(Some("bluez_input.headset"), &devices, cards),
            Choice::Named("bluez_input.headset".into())
        );
        assert_eq!(
            resolve(Some("USB Headset Mono"), &devices, cards),
            Choice::Named("alsa_input.usb-Headset.mono".into())
        );
        assert_eq!(
            resolve(Some("Unplugged Mic"), &devices, cards),
            Choice::Missing
        );
    }

    #[test]
    fn names_saved_by_v0_16_are_mapped_or_fall_back_to_the_default() {
        let devices = desktop();
        for default in ["default", "pipewire", "pulse", "sysdefault"] {
            assert_eq!(
                resolve(Some(default), &devices, cards),
                Choice::Default,
                "{default}"
            );
        }
        let usb = Choice::Named("alsa_input.usb-Headset.mono".into());
        assert_eq!(resolve(Some("hw:CARD=Headset,DEV=0"), &devices, cards), usb);
        assert_eq!(resolve(Some("plughw:1,0"), &devices, cards), usb);
        assert_eq!(
            resolve(Some("sysdefault:CARD=Headset"), &devices, cards),
            usb
        );
        // Card 0's only input; never the monitor listed before it.
        assert_eq!(
            resolve(Some("front:CARD=PCH,DEV=0"), &devices, cards),
            Choice::Named("alsa_input.pci.analog-stereo".into())
        );
        // A card that is gone, a device the card lacks, a card id unknown.
        assert_eq!(
            resolve(Some("hw:CARD=2,DEV=0"), &devices, cards),
            Choice::Missing
        );
        assert_eq!(
            resolve(Some("hw:CARD=PCH,DEV=3"), &devices, cards),
            Choice::Missing
        );
        assert_eq!(
            resolve(Some("hw:CARD=Gone,DEV=0"), &devices, cards),
            Choice::Missing
        );
        assert_eq!(resolve(Some("dmix"), &devices, cards), Choice::Missing);
        // No server lists: every saved name falls back to the default.
        assert_eq!(
            resolve(Some("hw:CARD=PCH,DEV=0"), &[], cards),
            Choice::Missing
        );
    }

    #[test]
    fn a_missing_device_is_told_once_a_run() {
        let wanted = Some("hw:CARD=NotHere-once,DEV=0");
        let (name, notice) = pick("input", wanted, Some(desktop()), protocol::DEFAULT_SOURCE);
        assert_eq!(name.as_c_str(), protocol::DEFAULT_SOURCE);
        assert_eq!(
            notice.as_deref(),
            Some(
                "saved input device \"hw:CARD=NotHere-once,DEV=0\" not found; using the default (:huddle devices)"
            )
        );
        let (_, again) = pick("input", wanted, Some(desktop()), protocol::DEFAULT_SOURCE);
        assert_eq!(again, None);
        // The same name as an output is its own notice.
        let (_, output) = pick("output", wanted, None, protocol::DEFAULT_SINK);
        assert!(output.is_some());
    }

    #[test]
    fn playout_waits_for_the_engine_instead_of_playing_silence() {
        use pulseaudio::PlaybackSource as _;
        use std::task::{Context, Poll, Waker};
        let (tx, rx) = mpsc::channel(4);
        let mut playout = Playout::new(rx);
        playout.cushion = 0;
        let mut cx = Context::from_waker(Waker::noop());
        let mut buf = [0_u8; 4 * 600];
        // Nothing sent yet: the server is told to wait, not handed zeros.
        assert_eq!(
            std::pin::Pin::new(&mut playout).poll_read(&mut cx, &mut buf),
            Poll::Pending
        );
        tx.try_send(vec![0.25; 480]).unwrap();
        // One chunk: 480 samples of the 600 asked for, then wait again.
        assert_eq!(
            std::pin::Pin::new(&mut playout).poll_read(&mut cx, &mut buf),
            Poll::Ready(4 * 480)
        );
        assert_eq!(f32::from_le_bytes(buf[..4].try_into().unwrap()), 0.25);
        assert_eq!(
            std::pin::Pin::new(&mut playout).poll_read(&mut cx, &mut buf),
            Poll::Pending
        );
        // Part of a chunk stays for the next request.
        tx.try_send(vec![0.5; 480]).unwrap();
        assert_eq!(
            std::pin::Pin::new(&mut playout).poll_read(&mut cx, &mut buf[..400]),
            Poll::Ready(400)
        );
        assert_eq!(playout.pending.len(), 380);
        // The engine gone: the rest, then the end of the stream.
        drop(tx);
        assert_eq!(
            std::pin::Pin::new(&mut playout).poll_read(&mut cx, &mut buf),
            Poll::Ready(4 * 380)
        );
        assert_eq!(
            std::pin::Pin::new(&mut playout).poll_read(&mut cx, &mut buf),
            Poll::Ready(0)
        );
    }

    #[test]
    fn playout_holds_back_a_cushion_before_playing_and_after_running_dry() {
        use pulseaudio::PlaybackSource as _;
        use std::task::{Context, Poll, Waker};
        let (tx, rx) = mpsc::channel(8);
        let mut playout = Playout::new(rx);
        assert_eq!(playout.cushion, 480);
        let mut cx = Context::from_waker(Waker::noop());
        let mut buf = [0_u8; 4 * 2000];
        let mut read =
            |playout: &mut Playout| std::pin::Pin::new(playout).poll_read(&mut cx, &mut buf);
        tx.try_send(vec![0.1; 240]).unwrap();
        assert_eq!(
            read(&mut playout),
            Poll::Pending,
            "5 ms is not yet the cushion"
        );
        tx.try_send(vec![0.1; 240]).unwrap();
        tx.try_send(vec![0.1; 480]).unwrap();
        assert_eq!(
            read(&mut playout),
            Poll::Ready(4 * 960),
            "all that has come"
        );
        // Dry: held back again until the cushion is there.
        assert_eq!(read(&mut playout), Poll::Pending);
        tx.try_send(vec![0.1; 100]).unwrap();
        assert_eq!(read(&mut playout), Poll::Pending);
        tx.try_send(vec![0.1; 400]).unwrap();
        assert_eq!(read(&mut playout), Poll::Ready(4 * 500));
    }

    #[test]
    fn the_framer_cuts_frames_from_bytes_of_any_size() {
        let samples: Vec<f32> = (0..(2 * FRAME_SAMPLES + 7)).map(|n| n as f32).collect();
        let bytes: Vec<u8> = samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        let mut framer = ByteFramer::default();
        let mut frames = Vec::new();
        // Odd sizes split samples across calls.
        for chunk in bytes.chunks(13) {
            framer.feed(chunk, |frame| frames.push(frame));
        }
        assert_eq!(frames.len(), 2);
        assert!(frames.iter().all(|frame| frame.len() == FRAME_SAMPLES));
        assert_eq!(frames.concat(), samples[..2 * FRAME_SAMPLES]);
        assert_eq!(framer.frame.len(), 7);
        assert!(framer.carry.is_empty());
    }

    #[test]
    fn no_sound_server_answers_at_once_in_one_line() {
        assert!(!NO_SOUND_SERVER.contains('\n'));
        assert!(NO_SOUND_SERVER.contains("PipeWire or PulseAudio"));
        let started = std::time::Instant::now();
        let candidates = socket_candidates(
            env(&[("PULSE_SERVER", "unix:/nonexistent/bzz-test/native tcp:host")]),
            1,
        );
        match connect_to(candidates) {
            Err(Error::Unsupported(message)) => assert_eq!(message, NO_SOUND_SERVER),
            other => panic!("expected the no-sound-server line, got {other:?}"),
        }
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
