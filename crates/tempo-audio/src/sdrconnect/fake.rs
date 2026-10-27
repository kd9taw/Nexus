//! A fake SDRconnect for the tests: a WebSocket server on loopback that speaks the published
//! "SDRconnect WebSocket API 1.0.3" as far as the scope uses it, written from the specification
//! alone. It answers `get_property`, takes `set_property device_center_frequency` (pushing the new
//! centre as `property_changed`), starts and stops its IQ on `iq_stream_enable`, starts its
//! receiver on `device_stream_enable`, and streams the primary receiver's IQ (binary type 2, i16
//! little-endian, I then Q) on the wall clock at its sample rate: carriers at absolute frequencies,
//! placed around whatever its centre is. It keeps every text message it was sent, for the
//! receive-only assertions. Never a real SDRplay program, never a real network.

use std::collections::VecDeque;
use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tungstenite::{Message, WebSocket};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// How the fake starts.
#[derive(Debug, Clone)]
pub(crate) struct Config {
    /// The receiver's sample rate, reported as `device_sample_rate` (Hz).
    pub(crate) rate: u32,
    /// The receiver's centre, reported as `device_center_frequency` (Hz).
    pub(crate) centre: u64,
    /// Carriers at these RF frequencies (Hz), each about 30 dB below full scale.
    pub(crate) carriers: Vec<u64>,
    /// Whether it sends IQ at all once asked.
    pub(crate) sends_iq: bool,
    /// Whether its receiver is streaming as it starts (headless SDRconnect's is not).
    pub(crate) started: bool,
    /// What it reports as `can_control`; when false it ignores a retune.
    pub(crate) can_control: bool,
    /// Pairs in each IQ message.
    pub(crate) pairs_per_message: usize,
    /// The rate the IQ actually arrives at, when not the reported one: a stream decimated without
    /// saying so.
    pub(crate) pace_rate: Option<u32>,
    /// One block of IQ computed once and sent over and over, for the load measurement: no tone
    /// arithmetic in the fake's thread at the fastest rates.
    pub(crate) canned: bool,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            rate: 192_000,
            centre: 14_074_000,
            carriers: Vec::new(),
            sends_iq: true,
            started: true,
            can_control: true,
            pairs_per_message: 1_920,
            pace_rate: None,
            canned: false,
        }
    }
}

/// What the fake's thread and the test share.
#[derive(Default)]
struct World {
    centre: u64,
    started: bool,
    streaming: bool,
    /// IQ paused by the test (the receiver stopped under SDRconnect).
    paused: bool,
    received: Vec<String>,
    /// Raw messages the test wants sent next, before any IQ.
    inject: VecDeque<Message>,
    connections: u32,
}

/// The fake SDRconnect.
pub(crate) struct Fake {
    addr: SocketAddr,
    world: Arc<Mutex<World>>,
    quit: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Fake {
    /// Listen on an ephemeral loopback port.
    pub(crate) fn start(config: Config) -> Fake {
        Fake::start_on(0, config).expect("the fake SDRconnect listens")
    }

    /// Listen on `port` of loopback (0: any).
    pub(crate) fn start_on(port: u16, config: Config) -> io::Result<Fake> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        listener.set_nonblocking(true)?;
        let addr = listener.local_addr()?;
        let world = Arc::new(Mutex::new(World {
            centre: config.centre,
            started: config.started,
            ..World::default()
        }));
        let quit = Arc::new(AtomicBool::new(false));
        let thread = {
            let (world, quit) = (world.clone(), quit.clone());
            std::thread::Builder::new()
                .name("fake-sdrconnect".into())
                .spawn(move || serve(&listener, &config, &world, &quit))?
        };
        Ok(Fake {
            addr,
            world,
            quit,
            thread: Some(thread),
        })
    }

    pub(crate) fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Every text message a client sent, in order.
    pub(crate) fn received(&self) -> Vec<String> {
        lock(&self.world).received.clone()
    }

    /// The receiver's centre now (Hz).
    pub(crate) fn centre(&self) -> u64 {
        lock(&self.world).centre
    }

    /// Connections accepted.
    pub(crate) fn connections(&self) -> u32 {
        lock(&self.world).connections
    }

    /// Stop (or resume) the IQ, as a receiver stopped under SDRconnect would.
    pub(crate) fn pause(&self, paused: bool) {
        lock(&self.world).paused = paused;
    }

    /// SDRconnect's own window retunes the receiver: the centre moves, and the client is told.
    pub(crate) fn retune_from_its_window(&self, hz: u64) {
        let mut world = lock(&self.world);
        world.centre = hz;
        world.inject.push_back(Message::text(push(
            "device_center_frequency",
            &hz.to_string(),
        )));
    }

    /// Send `message` to the client next.
    pub(crate) fn inject(&self, message: Message) {
        lock(&self.world).inject.push_back(message);
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        self.quit.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn push(property: &str, value: &str) -> String {
    serde_json::json!({
        "event_type": "property_changed", "property": property, "device": "primary",
        "value": value,
    })
    .to_string()
}

fn response(property: &str, value: &str) -> String {
    serde_json::json!({
        "event_type": "get_property_response", "property": property, "device": "primary",
        "value": value,
    })
    .to_string()
}

fn serve(listener: &TcpListener, config: &Config, world: &Mutex<World>, quit: &AtomicBool) {
    while !quit.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((tcp, _)) => {
                let _ = tcp.set_nonblocking(false);
                let _ = tcp.set_read_timeout(Some(Duration::from_secs(2)));
                if let Ok(ws) = tungstenite::accept(tcp) {
                    {
                        let mut w = lock(world);
                        w.connections += 1;
                        w.streaming = false;
                    }
                    connection(ws, config, world, quit);
                }
            }
            Err(_) => std::thread::sleep(Duration::from_millis(5)),
        }
    }
}

/// One client, until it goes or the fake does.
fn connection(
    mut ws: WebSocket<TcpStream>,
    config: &Config,
    world: &Mutex<World>,
    quit: &AtomicBool,
) {
    let _ = ws
        .get_mut()
        .set_read_timeout(Some(Duration::from_millis(2)));
    let per = config.pairs_per_message.max(1);
    let canned = config.canned.then(|| block(config, config.centre, 0, per));
    // The IQ clock: pairs sent since the stream started, against the time it started.
    let mut clock: Option<(Instant, u64)> = None;
    loop {
        if quit.load(Ordering::Acquire) {
            let _ = ws.close(None);
            let _ = ws.flush();
            return;
        }
        match ws.read() {
            Ok(Message::Text(text)) => {
                if !answer(&mut ws, text.as_str(), config, world) {
                    drain(&mut ws, world);
                    return;
                }
            }
            Ok(Message::Close(_)) => {
                let _ = ws.flush();
                return;
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return,
        }

        let injected: Vec<Message> = lock(world).inject.drain(..).collect();
        for m in injected {
            if ws.send(m).is_err() {
                drain(&mut ws, world);
                return;
            }
        }
        let (streaming, centre) = {
            let w = lock(world);
            (
                w.streaming && w.started && !w.paused && config.sends_iq,
                w.centre,
            )
        };
        if !streaming {
            clock = None;
            continue;
        }
        let (since, sent) = clock.get_or_insert((Instant::now(), 0));
        let pace = config.pace_rate.unwrap_or(config.rate);
        let due = (since.elapsed().as_secs_f64() * f64::from(pace)) as u64;
        while *sent + per as u64 <= due {
            let bytes = match &canned {
                Some(b) => b.clone(),
                None => block(config, centre, *sent, per),
            };
            if ws.send(Message::binary(bytes)).is_err() {
                drain(&mut ws, world);
                return;
            }
            *sent += per as u64;
        }
    }
}

/// A send failed: the client has gone. Keep the text it sent before going, which is already here
/// but unread: a client that stops its IQ and closes while the fake is busy sending would
/// otherwise lose its last messages from [`World::received`].
fn drain(ws: &mut WebSocket<TcpStream>, world: &Mutex<World>) {
    loop {
        match ws.read() {
            Ok(Message::Text(text)) => lock(world).received.push(text.as_str().to_string()),
            Ok(Message::Close(_)) | Err(_) => return,
            Ok(_) => {}
        }
    }
}

/// Answer one text message; false when the connection is to end.
fn answer(
    ws: &mut WebSocket<TcpStream>,
    text: &str,
    config: &Config,
    world: &Mutex<World>,
) -> bool {
    lock(world).received.push(text.to_string());
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return true;
    };
    let event = v["event_type"].as_str().unwrap_or_default();
    let property = v["property"].as_str().unwrap_or_default();
    let value = v["value"].as_str().unwrap_or_default();
    let reply = match event {
        "get_property" => {
            let w = lock(world);
            let value = match property {
                "api_version" => Some("1.0.3".to_string()),
                "device_center_frequency" => Some(w.centre.to_string()),
                "device_sample_rate" => Some(format!("{:.3}", f64::from(config.rate))),
                "started" => Some(w.started.to_string()),
                "can_control" => Some(config.can_control.to_string()),
                _ => None,
            };
            value.map(|value| response(property, &value))
        }
        "set_property" if property == "device_center_frequency" && config.can_control => {
            let hz = value.parse::<u64>().ok();
            hz.map(|hz| {
                lock(world).centre = hz;
                push(property, &hz.to_string())
            })
        }
        "iq_stream_enable" => {
            lock(world).streaming = value == "true";
            None
        }
        "device_stream_enable" if value == "true" => {
            lock(world).started = true;
            Some(push("started", "true"))
        }
        _ => None,
    };
    reply.is_none_or(|r| ws.send(Message::text(r)).is_ok())
}

/// One IQ message: type 2, then `pairs` pairs from pair `first` of the stream, the carriers placed
/// around `centre`.
fn block(config: &Config, centre: u64, first: u64, pairs: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + 4 * pairs);
    out.extend_from_slice(&2u16.to_le_bytes());
    let rate = f64::from(config.rate);
    let amp = 0.03 * 32_767.0;
    for i in 0..pairs {
        let t = (first + i as u64) as f64 / rate;
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for f in &config.carriers {
            let ph = std::f64::consts::TAU * (*f as f64 - centre as f64) * t;
            re += ph.cos();
            im += ph.sin();
        }
        out.extend_from_slice(&((re * amp) as i16).to_le_bytes());
        out.extend_from_slice(&((im * amp) as i16).to_le_bytes());
    }
    out
}
