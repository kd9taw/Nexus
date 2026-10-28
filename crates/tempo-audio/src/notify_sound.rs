//! Notification sounds (JS8's: Settings ▸ Digital ▸ JS8 ▸ Sounds) played through an output the
//! operator picks, as JS8Call lets them pick one (its notification audio device), and NEVER through
//! a device the station transmits from.
//!
//! VOX SAFETY, and it is the reason this module exists. A rig keyed by VOX, or by a sound card's
//! own VOX circuit, transmits whatever plays on its TX audio device. So a sound is refused, and
//! nothing plays, when the device that would open is:
//! - the TX audio device of the station (`audio_out`) or of ANY radio profile, the one in use or
//!   not, since a second rig can be keyed by its own VOX while the first is the one shown;
//! - SmartSDR's DAX transmit device, whatever is configured ([`crate::monitor`]'s rule);
//! - the TX output the radio's audio path holds open now, by the name it resolved to when the
//!   radio opened ([`open_tx_outputs`]): a TX device of "system default" stays on the device it
//!   opened when the default later moves, and the settings no longer name it;
//! - the same ALSA card as one of those, reached by another name (`hw:` against `plughw:`, or by
//!   its number, `hw:1,0`, resolved to the card's id);
//! - the system default output, when a TX device is configured as "system default" (empty);
//! - one that cannot be PROVEN different from a TX device by name. On Linux only two different
//!   ALSA cards are (`plughw:CARD=CODEC` against `hw:CARD=PCH`). Everything else is refused: the
//!   sound server's default sink (`default`, `pulse`, `pipewire`… are ONE target, and it may be
//!   the rig's card), and any PCM whose routing lives in a config file rather than its name (an
//!   `~/.asoundrc` PCM such as `rigtx`, `plug:…`, `oss`, a bare `dmix`), against anything but its
//!   own name. Elsewhere (Windows, macOS) two different device names are two devices.
//!
//! The check is made at EVERY play, against the settings as they are then and the device that
//! actually opened, so a TX device changed after the sound device was picked is caught at the next
//! sound. Off Linux two devices are compared by name, case-blind, the way the headphone monitor's
//! guard compares them, and without the " #N" a device list adds to a second identically named
//! codec and the "N- " Windows numbers it by inside its name ("Speakers (2- USB Audio CODEC)"). On
//! Linux a second card with a taken id is `<id>_N` (`CODEC_1`), and it is compared without the
//! `_N`. Two identically named codecs are therefore one device here, whichever number each has
//! today (the numbers move when one is unplugged), which refuses more rather than less.
//! The refusal says which of these it was ([`PlayRefusal`]), so the operator is told what to change.
//!
//! The pure pieces (the guard, the built-in tones) build without the `device` feature so they are
//! tested in the headless workspace build; the cpal output lives behind it.

use tempo_app::settings::Settings;

/// The sample rate the built-in tones are made at.
pub const TONE_RATE: u32 = 48_000;

/// Why a notification sound did not play at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayRefusal {
    /// The device that would open is one the station transmits from.
    TxDevice,
    /// A TX audio device is set to "the system default output" (the station's, or a radio
    /// profile's that has none, as a radio whose audio is not on this computer has), and the sound
    /// would open that default, or a default nobody can name.
    TxDefault,
    /// The two cannot be proven different by name, so the sound is refused (Linux): the sound
    /// server's default sink (`default`, `pulse`, `pipewire`…) or a PCM routed by a config file
    /// (`~/.asoundrc`'s own names, `plug:…`, `oss`…) against anything but itself, or a card by a
    /// number this computer cannot resolve.
    TxUnsure,
    /// No such output device here (a named one not found, or no default output at all). Never
    /// replaced by the default: that could be the TX device under another name.
    NoDevice,
    /// The device would not open or play.
    Failed(String),
}

/// Every TX audio device the station has configured: the flat `audio_out` and each radio
/// profile's. An empty name is "the system default output".
pub fn tx_audio_devices(settings: &Settings) -> Vec<String> {
    std::iter::once(settings.audio_out.clone())
        .chain(settings.radios.iter().map(|r| r.audio_out.clone()))
        .collect()
}

/// The TX outputs the radio's audio path holds now, each by the name of the device it ACTUALLY
/// resolved when it opened ([`hold_tx_output`]). The settings name a TX device as the operator
/// chose it, and "" (the system default) is resolved once, when the radio opens: if the system
/// default moves afterwards (a headset plugged in), the radio keeps transmitting on the device it
/// holds, which the settings no longer name. Every sound is refused against these as well.
static OPEN_TX_OUTPUTS: std::sync::Mutex<Vec<(u64, String)>> = std::sync::Mutex::new(Vec::new());
static NEXT_TX_HOLD: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// One TX output held open by the radio's audio path, named in [`OPEN_TX_OUTPUTS`] until it is
/// dropped (when the audio backend that resolved it goes).
#[derive(Debug)]
pub struct TxOutputHold(u64);

impl Drop for TxOutputHold {
    fn drop(&mut self) {
        OPEN_TX_OUTPUTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|(id, _)| *id != self.0);
    }
}

/// Name `device` (the name the device that opened reports) as a TX output held open, until the
/// returned hold is dropped.
pub fn hold_tx_output(device: String) -> TxOutputHold {
    let id = NEXT_TX_HOLD.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    OPEN_TX_OUTPUTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push((id, device));
    TxOutputHold(id)
}

/// The TX outputs held open now, by the names they opened as. Read-only.
pub fn open_tx_outputs() -> Vec<String> {
    OPEN_TX_OUTPUTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .map(|(_, name)| name.clone())
        .collect()
}

/// The TX devices a sound is refused against: the settings' (`tx_devices`) and every TX output
/// the radio holds open now ([`open_tx_outputs`]).
#[cfg_attr(not(feature = "device"), allow(dead_code))]
fn guarded_tx_devices(tx_devices: &[String]) -> Vec<String> {
    let mut all = tx_devices.to_vec();
    all.extend(open_tx_outputs());
    all
}

/// The ALSA PCM names that reach the sound server's (or ALSA's) default sink rather than a card
/// named in them: whatever that sink is, it may be the rig's card.
const SOUND_SERVER_ROUTES: [&str; 10] = [
    "default",
    "sysdefault",
    "pulse",
    "pipewire",
    "jack",
    "samplerate",
    "speexrate",
    "lavrate",
    "upmix",
    "vdownmix",
];

/// A sound card, by the id ALSA gives it (lower-cased), or by its number when the id is unknown.
#[derive(Debug, PartialEq, Eq)]
enum Card {
    Id(String),
    Number(u32),
}

/// The ALSA plugins whose argument names one card and that play to that card: the hardware and
/// its standard per-card aliases (alsa-lib's `cards/*.conf`). A card argument on any other plugin
/// proves nothing: an `~/.asoundrc` PCM may take a `CARD=` and send its sound anywhere.
const CARD_PLUGINS: [&str; 13] = [
    "hw",
    "plughw",
    "dmix",
    "dsnoop",
    "front",
    "rear",
    "center_lfe",
    "side",
    "iec958",
    "spdif",
    "hdmi",
    "usbstream",
    "sysdefault",
];

/// How this computer's audio host names an output device, and so how two names are compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Naming {
    /// Linux: ALSA PCM names, whose routing may live in a config file.
    Alsa,
    /// Windows / macOS: one name per endpoint.
    Endpoint,
}

/// This computer's naming (cpal's ALSA host on Linux, WASAPI / CoreAudio elsewhere).
const HOST_NAMING: Naming = if cfg!(target_os = "linux") {
    Naming::Alsa
} else {
    Naming::Endpoint
};

/// Where a device name sends its sound, as far as its name can tell.
#[derive(Debug, PartialEq, Eq)]
enum Route {
    /// ALSA: the sound server's default sink.
    Server,
    /// ALSA: one card, by a plugin that plays to it (`hw:`, `plughw:`, `dmix:`, `front:`…).
    Card(Card),
    /// ALSA: any other PCM (an `~/.asoundrc` name, `plug:…`, `oss`, a bare `dmix`), lower-cased.
    /// Its routing lives in a config file, so it is the same as its own name and proven different
    /// from nothing.
    Pcm(String),
    /// Windows / macOS: its base name, lower-cased, without the " #N" ordinal that tells two
    /// identically named codecs apart in a list, and without the "N- " Windows numbers a second
    /// one by inside its name ("Speakers (2- USB Audio CODEC)") — they are one device here.
    Named(String),
}

/// Two names collide: the same device, or two the guard cannot prove different.
#[derive(Debug, PartialEq, Eq)]
enum Collision {
    Same,
    Unsure,
}

/// The route of `name` under `naming`, a card number resolved to its id through `card_id`.
fn route(name: &str, naming: Naming, card_id: &dyn Fn(u32) -> Option<String>) -> Route {
    let name = name.trim();
    let lower = name.to_ascii_lowercase();
    if naming == Naming::Endpoint {
        return Route::Named(without_instance_numbers(
            crate::audiodev::split_device_ordinal(&lower).0.trim(),
        ));
    }
    let (plugin, args) = lower.split_once(':').unwrap_or((&lower, ""));
    if CARD_PLUGINS.contains(&plugin) || plugin.starts_with("surround") {
        // `plughw:CARD=CODEC,DEV=0`, `hw:1,0`, `front:CODEC`: the card argument, by name or first.
        let card = match args
            .split(',')
            .find_map(|kv| kv.trim().strip_prefix("card="))
        {
            Some(card) => Some(card),
            None => args.split(',').next().filter(|a| !a.contains('=')),
        };
        // ALSA takes a quoted argument too (`hw:CARD="CODEC"`): the same card.
        let card = card.map(|c| c.trim().trim_matches(|q| q == '"' || q == '\'').trim());
        if let Some(card) = card.filter(|c| !c.is_empty()) {
            return Route::Card(card_ref(card, card_id));
        }
    }
    if args.is_empty() && SOUND_SERVER_ROUTES.contains(&plugin) {
        return Route::Server;
    }
    Route::Pcm(lower)
}

/// `name` without the "N- " Windows puts before a second identically named device's name, at the
/// start or just inside a parenthesis ("Speakers (2- USB Audio CODEC)" is "Speakers (USB Audio
/// CODEC)"). Windows renumbers the twins when one is unplugged, so the number does not tell them
/// apart over time.
fn without_instance_numbers(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut rest = name;
    loop {
        let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if digits > 0 && rest[digits..].starts_with("- ") {
            rest = &rest[digits + 2..];
        }
        match rest.find('(') {
            Some(i) => {
                out.push_str(&rest[..=i]);
                rest = &rest[i + 1..];
            }
            None => {
                out.push_str(rest);
                return out;
            }
        }
    }
}

/// An ALSA card id as the card it names, whichever of identically named cards it is. A second
/// card whose id is taken gets `<id>_N` (`CODEC_1`), the id cut short to keep it within 15
/// characters (`USBAudioCODEC_1` for a second `USBAudioCODEC12`), and the numbers move when a
/// card is unplugged. So two ids are one card when they match without the `_N`, or when one was
/// cut short for it and the other starts with what was left.
fn same_card_id(x: &str, y: &str) -> bool {
    fn base(id: &str) -> (&str, bool) {
        match id.rsplit_once('_') {
            Some((b, n))
                if !b.is_empty() && !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()) =>
            {
                (b, id.len() == 15)
            }
            _ => (id, false),
        }
    }
    let ((bx, cut_x), (by, cut_y)) = (base(x), base(y));
    bx == by || (cut_x && by.starts_with(bx)) || (cut_y && bx.starts_with(by))
}

fn card_ref(token: &str, card_id: &dyn Fn(u32) -> Option<String>) -> Card {
    match token.parse::<u32>() {
        Ok(n) => card_id(n)
            .map(|id| Card::Id(id.trim().to_ascii_lowercase()))
            .unwrap_or(Card::Number(n)),
        Err(_) => Card::Id(token.to_ascii_lowercase()),
    }
}

/// Whether two routes may be one device. `None` ONLY where they are proven different: two
/// different cards (by id, or by number), or two different endpoint names. Identically named
/// twins are one device ([`same_card_id`], [`without_instance_numbers`]). Every other pair —
/// the sound server against anything else, a config-file PCM against anything but its own name,
/// a card by id against one by a number nobody resolved — is unsure, and refuses.
fn collide(a: &Route, b: &Route) -> Option<Collision> {
    match (a, b) {
        (Route::Server, Route::Server) => Some(Collision::Same),
        (Route::Card(x), Route::Card(y)) => match (x, y) {
            (Card::Id(x), Card::Id(y)) if same_card_id(x, y) => Some(Collision::Same),
            (Card::Number(x), Card::Number(y)) if x == y => Some(Collision::Same),
            (Card::Id(_), Card::Id(_)) | (Card::Number(_), Card::Number(_)) => None,
            (Card::Id(_), Card::Number(_)) | (Card::Number(_), Card::Id(_)) => {
                Some(Collision::Unsure)
            }
        },
        (Route::Pcm(x), Route::Pcm(y)) if x == y => Some(Collision::Same),
        (Route::Named(x), Route::Named(y)) if x == y => Some(Collision::Same),
        (Route::Named(_), Route::Named(_)) => None,
        _ => Some(Collision::Unsure),
    }
}

/// An ALSA card's id from its number, as the kernel lists it (`/proc/asound/card<N>/id`).
fn system_card_id(n: u32) -> Option<String> {
    if cfg!(target_os = "linux") {
        std::fs::read_to_string(format!("/proc/asound/card{n}/id"))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    } else {
        None
    }
}

/// THE GUARD for one name: why playing on `device` might play into one of `tx_devices`, where an
/// empty TX name is the system default output, `default_output` (`None` when the host cannot name
/// one). The strongest reason wins: a named TX device over an unprovable one over a default one.
fn device_verdict(
    device: &str,
    tx_devices: &[String],
    default_output: Option<&str>,
    naming: Naming,
    card_id: &dyn Fn(u32) -> Option<String>,
) -> Option<PlayRefusal> {
    let device = device.trim();
    if device.is_empty() {
        return Some(PlayRefusal::TxDevice); // a device with no name cannot be told apart
    }
    // SmartSDR's DAX TX, whatever is configured (the monitor's rule).
    if crate::monitor::is_flex_dax_tx_device(device) {
        return Some(PlayRefusal::TxDevice);
    }
    let dev = route(device, naming, card_id);
    let default_output = default_output.map(str::trim).filter(|d| !d.is_empty());
    let mut why: Option<PlayRefusal> = None;
    let rank = |r: &PlayRefusal| match r {
        PlayRefusal::TxDevice => 3,
        PlayRefusal::TxUnsure => 2,
        _ => 1,
    };
    for tx in tx_devices {
        let tx = tx.trim();
        let found = if tx.is_empty() {
            match default_output {
                // A TX device that is "the default" on a host that cannot name its default: no
                // output can be shown to differ from it.
                None => Some(PlayRefusal::TxDefault),
                Some(d) => collide(&dev, &route(d, naming, card_id)).map(|c| match c {
                    Collision::Same => PlayRefusal::TxDefault,
                    Collision::Unsure => PlayRefusal::TxUnsure,
                }),
            }
        } else {
            collide(&dev, &route(tx, naming, card_id)).map(|c| match c {
                Collision::Same => PlayRefusal::TxDevice,
                Collision::Unsure => PlayRefusal::TxUnsure,
            })
        };
        if let Some(f) = found {
            if why.as_ref().is_none_or(|w| rank(&f) > rank(w)) {
                why = Some(f);
            }
        }
    }
    why
}

/// [`refuse_tx`] with the naming and the card-number lookup handed in (tests name their own
/// platform and cards).
fn verdict_with(
    opened: &str,
    configured: &str,
    tx_devices: &[String],
    default_output: Option<&str>,
    naming: Naming,
    card_id: &dyn Fn(u32) -> Option<String>,
) -> Option<PlayRefusal> {
    let a = device_verdict(opened, tx_devices, default_output, naming, card_id);
    let b = (!configured.trim().is_empty())
        .then(|| device_verdict(configured, tx_devices, default_output, naming, card_id))
        .flatten();
    match (a, b) {
        (Some(PlayRefusal::TxDevice), _) | (_, Some(PlayRefusal::TxDevice)) => {
            Some(PlayRefusal::TxDevice)
        }
        (Some(PlayRefusal::TxUnsure), _) | (_, Some(PlayRefusal::TxUnsure)) => {
            Some(PlayRefusal::TxUnsure)
        }
        (a, b) => a.or(b),
    }
}

/// THE GUARD, as a predicate: whether playing on `device` (the name of the device that opens,
/// never empty) might play into one of `tx_devices` — the same device, or one it cannot be told
/// apart from, which refuses as well. An empty TX name is the system default output,
/// `default_output` (`None` when the host cannot name one).
pub fn would_transmit(device: &str, tx_devices: &[String], default_output: Option<&str>) -> bool {
    device_verdict(
        device,
        tx_devices,
        default_output,
        HOST_NAMING,
        &system_card_id,
    )
    .is_some()
}

/// The check made before a sound's stream is built: the device that OPENED (`opened`) and the name
/// it was asked by (`configured`, empty for the default) may neither be a TX device, nor one that
/// cannot be told apart from it. Every play passes here, with the TX devices as the settings hold
/// them at that play. The refusal says why, so the operator is told what to change.
pub fn refuse_tx(
    opened: &str,
    configured: &str,
    tx_devices: &[String],
    default_output: Option<&str>,
) -> Result<(), PlayRefusal> {
    match verdict_with(
        opened,
        configured,
        tx_devices,
        default_output,
        HOST_NAMING,
        &system_card_id,
    ) {
        Some(why) => Err(why),
        None => Ok(()),
    }
}

/// A built-in tone: a short sine at `hz` with the alerts' envelope (10 ms up to 0.18, down to
/// silence at 220 ms), twice for `double`, the second a fifth higher 130 ms later — what the
/// window's own alert beeps sound like.
pub fn tone(hz: f32, double: bool) -> Vec<f32> {
    let rate = TONE_RATE as f32;
    let beep = |f: f32| -> Vec<f32> {
        let n = (0.24 * rate) as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / rate;
                let env = if t < 0.01 {
                    0.0001 * (0.18f32 / 0.0001).powf(t / 0.01)
                } else if t < 0.22 {
                    0.18 * (0.0001f32 / 0.18).powf((t - 0.01) / 0.21)
                } else {
                    0.0
                };
                env * (std::f32::consts::TAU * f * t).sin()
            })
            .collect()
    };
    let mut out = beep(hz);
    if double {
        let at = (0.13 * rate) as usize;
        let second = beep(hz * 1.5);
        out.resize(at + second.len(), 0.0);
        for (o, s) in out[at..].iter_mut().zip(second) {
            *o = (*o + s).clamp(-1.0, 1.0);
        }
    }
    out
}

/// The longest a notification sound plays, whatever it was made from: a notification is a second
/// or two, and this bounds the time a sound holds its output and the memory its samples take
/// at the device's rate (the WAV reader cuts a file to the same length before it is resampled).
pub const MAX_PLAY_SECONDS: u32 = 10;

/// `samples` at `from` samples a second, linearly resampled to `to` and cut to
/// [`MAX_PLAY_SECONDS`] there.
#[cfg_attr(not(feature = "device"), allow(dead_code))]
fn clip_for_output(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    let from = from.max(1);
    let keep = (from as usize).saturating_mul(MAX_PLAY_SECONDS as usize);
    // Cut before resampling, so what is made at the device rate is at most that long too.
    let samples = &samples[..samples.len().min(keep)];
    if from == to || samples.is_empty() {
        return samples.to_vec();
    }
    let n = (samples.len() as u64 * u64::from(to) / u64::from(from)) as usize;
    let step = from as f64 / to as f64;
    (0..n)
        .map(|i| {
            let p = i as f64 * step;
            let k = p as usize;
            let f = (p - k as f64) as f32;
            let a = samples[k.min(samples.len() - 1)];
            let b = samples[(k + 1).min(samples.len() - 1)];
            a + (b - a) * f
        })
        .collect()
}

/// Whether a sound is playing now ([`begin_play`]).
static PLAYING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A sound's turn at the output, held from before its stream opens until after it is closed.
#[cfg_attr(not(feature = "device"), allow(dead_code))]
struct PlayTurn;

impl Drop for PlayTurn {
    fn drop(&mut self) {
        PLAYING.store(false, std::sync::atomic::Ordering::Release);
    }
}

/// The output's turn for one sound, or `None` while another sound has it: sounds play one at a
/// time, so a burst of decodes cannot stack streams and threads.
#[cfg_attr(not(feature = "device"), allow(dead_code))]
fn begin_play() -> Option<PlayTurn> {
    use std::sync::atomic::Ordering;
    PLAYING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .ok()
        .map(|_| PlayTurn)
}

#[cfg(feature = "device")]
pub use device_play::play;

#[cfg(feature = "device")]
mod device_play {
    use super::{begin_play, clip_for_output, guarded_tx_devices, refuse_tx, PlayRefusal};
    use crate::device::{cpal_device_name, dispatch_format, resolve_configured, AUDIO_HOST_LOCK};
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::SampleFormat;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn err_fn(e: cpal::Error) {
        eprintln!("tempo-audio: notification sound stream error: {e}");
    }

    /// Play `samples` (mono, at `rate`) on `device` ("" = the system default output), unless the
    /// device that opens is one of `tx_devices` ([`would_transmit`]). Returns once the sound has
    /// started, or with why it did not; the sound plays out on its own thread, which owns the
    /// stream (a cpal `Stream` is `!Send`). One sound plays at a time: a play asked for while
    /// another is playing is dropped ([`begin_play`]) and returns `Ok`, nothing opened.
    pub fn play(
        device: &str,
        tx_devices: Vec<String>,
        rate: u32,
        samples: Vec<f32>,
    ) -> Result<(), PlayRefusal> {
        // One sound at a time: one asked for while another plays is dropped, not queued.
        let Some(turn) = begin_play() else {
            return Ok(());
        };
        let device = device.to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("notify-sound".into())
            .spawn(move || {
                let started = open_and_start(&device, &tx_devices, rate, &samples);
                let ok = started.is_ok();
                let (stream, secs) = match started {
                    Ok(s) => (Some(s.0), s.1),
                    Err(e) => {
                        drop(turn);
                        let _ = tx.send(Err(e));
                        return;
                    }
                };
                let _ = tx.send(Ok(()));
                if ok {
                    std::thread::sleep(std::time::Duration::from_secs_f32(secs + 0.25));
                }
                close_locked(stream);
                drop(turn);
            })
            .map_err(|e| PlayRefusal::Failed(e.to_string()))?;
        rx.recv()
            .unwrap_or_else(|_| Err(PlayRefusal::Failed("the sound thread ended".into())))
    }

    /// Tear the sound's stream down under [`AUDIO_HOST_LOCK`], as every other cpal teardown in
    /// this crate is (`device.rs`: host init and stream construction/teardown from two threads at
    /// once fault natively, which no `catch_unwind` catches). The play thread closes ONLY here.
    fn close_locked(stream: Option<cpal::Stream>) {
        let _host_guard = AUDIO_HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        drop(stream);
    }

    fn open_and_start(
        device: &str,
        tx_devices: &[String],
        rate: u32,
        samples: &[f32],
    ) -> Result<(cpal::Stream, f32), PlayRefusal> {
        let _host_guard = AUDIO_HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let host = cpal::default_host();
        let default_output = host
            .default_output_device()
            .and_then(|d| cpal_device_name(&d));
        let name = (!device.trim().is_empty()).then_some(device);
        let dev = resolve_configured(
            || host.output_devices().ok(),
            name,
            host.default_output_device(),
            "notification output",
        )
        .map_err(|_| PlayRefusal::NoDevice)?;
        let opened = cpal_device_name(&dev).ok_or(PlayRefusal::NoDevice)?;
        // The settings' TX devices and the TX outputs the radio holds open, as they are now.
        refuse_tx(
            &opened,
            device,
            &guarded_tx_devices(tx_devices),
            default_output.as_deref(),
        )?;
        let supported = dev
            .default_output_config()
            .map_err(|e| PlayRefusal::Failed(e.to_string()))?;
        let out_rate = supported.sample_rate();
        let out_ch = usize::from(supported.channels()).max(1);
        let config: cpal::StreamConfig = supported.config();
        let clip: Arc<Vec<f32>> = Arc::new(clip_for_output(samples, rate, out_rate));
        let secs = clip.len() as f32 / out_rate.max(1) as f32;
        let stream = dispatch_format!(
            supported.sample_format(),
            build,
            "notification output",
            &dev,
            &config,
            out_ch,
            &clip,
        )
        .map_err(PlayRefusal::Failed)?;
        stream
            .play()
            .map_err(|e| PlayRefusal::Failed(e.to_string()))?;
        Ok((stream, secs))
    }

    fn build<T: crate::device::DeviceSample + Send + 'static>(
        dev: &cpal::Device,
        config: &cpal::StreamConfig,
        out_ch: usize,
        clip: &Arc<Vec<f32>>,
    ) -> Result<cpal::Stream, String> {
        let clip = clip.clone();
        let at = AtomicUsize::new(0);
        dev.build_output_stream(
            *config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                for frame in data.chunks_mut(out_ch) {
                    let i = at.fetch_add(1, Ordering::Relaxed);
                    let v = T::from_unit(clip.get(i).copied().unwrap_or(0.0));
                    for x in frame.iter_mut() {
                        *x = v;
                    }
                }
            },
            err_fn,
            None,
        )
        .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    fn no_cards(_: u32) -> Option<String> {
        None
    }

    fn refuse(
        naming: Naming,
        opened: &str,
        configured: &str,
        tx: &[String],
        default_output: Option<&str>,
    ) -> Result<(), PlayRefusal> {
        match verdict_with(opened, configured, tx, default_output, naming, &no_cards) {
            Some(why) => Err(why),
            None => Ok(()),
        }
    }

    /// Windows / macOS naming: whether a sound on `device` is refused.
    fn ep(device: &str, tx: &[String], default_output: Option<&str>) -> bool {
        refuse(Naming::Endpoint, device, "", tx, default_output).is_err()
    }

    fn ep_refuse(
        opened: &str,
        configured: &str,
        tx: &[String],
        default_output: Option<&str>,
    ) -> Result<(), PlayRefusal> {
        refuse(Naming::Endpoint, opened, configured, tx, default_output)
    }

    /// Linux (ALSA) naming: whether a sound on `device` is refused.
    fn alsa(device: &str, tx: &[String], default_output: Option<&str>) -> bool {
        refuse(Naming::Alsa, device, "", tx, default_output).is_err()
    }

    fn alsa_refuse(
        opened: &str,
        configured: &str,
        tx: &[String],
        default_output: Option<&str>,
    ) -> Result<(), PlayRefusal> {
        refuse(Naming::Alsa, opened, configured, tx, default_output)
    }

    #[test]
    fn the_tx_device_is_refused_by_any_spelling_of_its_name() {
        let tx = s(&["USB Audio CODEC"]);
        assert!(ep("USB Audio CODEC", &tx, Some("Speakers")));
        assert!(ep("usb audio codec ", &tx, Some("Speakers")));
        assert!(
            !ep("Speakers", &tx, Some("Speakers")),
            "control: another device plays"
        );
    }

    #[test]
    fn every_radio_profiles_tx_device_is_refused_not_only_the_one_in_use() {
        let settings = Settings {
            audio_out: "Rig A CODEC".into(),
            radios: vec![tempo_app::settings::RadioProfile {
                audio_out: "Rig B CODEC".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let tx = tx_audio_devices(&settings);
        assert!(ep("Rig A CODEC", &tx, None));
        assert!(ep("Rig B CODEC", &tx, None), "the radio not shown");
        assert!(!ep("Headphones", &tx, None), "control");
    }

    #[test]
    fn a_tx_device_set_to_the_system_default_refuses_the_default() {
        let tx = s(&[""]);
        assert!(ep("Speakers", &tx, Some("Speakers")));
        assert!(!ep("Headphones", &tx, Some("Speakers")), "control");
        assert!(
            ep("Headphones", &tx, None),
            "a default nobody can name might be any device"
        );
    }

    #[test]
    fn the_same_alsa_card_by_another_name_is_the_same_device() {
        let tx = s(&["plughw:CARD=CODEC,DEV=0"]);
        assert!(alsa("hw:CARD=CODEC,DEV=0", &tx, None));
        assert!(alsa("dmix:CARD=CODEC,DEV=0", &tx, None));
        assert!(
            !alsa("hw:CARD=PCH,DEV=0", &tx, None),
            "control: another card"
        );
    }

    /// Windows/macOS list a second identically named codec as "<name> #2" (`audiodev`'s
    /// `disambiguate_names`), while the device that opens is named without it: the ordinal must not
    /// tell the TX codec apart from the one a sound opens (the module's own rule: two identically
    /// named codecs are one device here).
    #[test]
    fn the_ordinal_suffix_does_not_hide_the_tx_codec() {
        let tx = s(&["USB Audio CODEC #2"]);
        assert!(
            ep("USB Audio CODEC", &tx, Some("Speakers")),
            "TX is the second codec, the opened one is named bare"
        );
        assert!(
            ep("usb audio codec #3", &s(&["USB Audio CODEC"]), None),
            "and the other way round"
        );
        assert_eq!(
            ep_refuse("USB Audio CODEC", "", &tx, Some("USB Audio CODEC")),
            Err(PlayRefusal::TxDevice),
            "the default output is that codec, nothing configured"
        );
        assert!(
            !ep("Speakers", &tx, Some("Speakers")),
            "control: another device plays"
        );
        assert!(
            !ep("Speakers #2", &tx, Some("Speakers")),
            "control: a second pair of speakers is not the codec"
        );
    }

    /// Windows numbers a second identically named codec INSIDE its endpoint name ("Speakers (2- USB
    /// Audio CODEC)"), and renumbers it when the first is unplugged, so a stale TX name and the
    /// sound's name can be one codec under two numbers: twins are one device here.
    #[test]
    fn a_renumbered_twin_codec_is_the_same_device_on_windows() {
        let tx = s(&["Speakers (2- USB Audio CODEC)"]);
        assert_eq!(
            ep_refuse(
                "Speakers (USB Audio CODEC)",
                "",
                &tx,
                Some("Speakers (Realtek)")
            ),
            Err(PlayRefusal::TxDevice),
            "the TX codec, renumbered after its twin was unplugged"
        );
        assert!(
            ep(
                "Speakers (3- USB Audio CODEC)",
                &s(&["Speakers (USB Audio CODEC)"]),
                None
            ),
            "and the other way round"
        );
        assert!(
            ep("2- USB Audio CODEC", &s(&["USB Audio CODEC"]), None),
            "a bare name"
        );
        assert!(
            !ep("Speakers (Realtek)", &tx, Some("Speakers (Realtek)")),
            "control: another device plays"
        );
        assert!(
            !ep("Headphones (USB Audio CODEC)", &tx, None),
            "control: another endpoint of another name plays"
        );
        assert!(
            !ep("Speakers (Line 2- Out)", &s(&["Speakers (Line Out)"]), None),
            "control: a number inside a word is part of the name"
        );
    }

    /// Linux names a second card with a taken id `<id>_N` (`CODEC_1`), cutting the id short to
    /// keep it within 15 characters, and renumbers it when the first is unplugged: twins are one
    /// card here.
    #[test]
    fn a_renumbered_twin_card_is_the_same_card_on_linux() {
        let tx = s(&["plughw:CARD=CODEC_1,DEV=0"]);
        assert_eq!(
            alsa_refuse("hw:CARD=CODEC,DEV=0", "", &tx, None),
            Err(PlayRefusal::TxDevice)
        );
        assert!(
            alsa("front:CODEC_2", &s(&["hw:CARD=CODEC"]), None),
            "the other way round"
        );
        assert_eq!(
            verdict_with(
                "hw:1,0",
                "",
                &s(&["hw:CARD=CODEC"]),
                None,
                Naming::Alsa,
                &|n| { (n == 1).then(|| "CODEC_1".to_string()) }
            ),
            Some(PlayRefusal::TxDevice),
            "by number, resolved to the twin's id"
        );
        assert!(
            alsa(
                "hw:CARD=USBAudioCODEC12",
                &s(&["hw:CARD=USBAudioCODEC_1"]),
                None
            ),
            "a 15-character id cut short for its number"
        );
        assert!(
            !alsa("hw:CARD=PCH,DEV=0", &tx, None),
            "control: another card plays"
        );
        assert!(
            !alsa("hw:CARD=CODECPRO", &s(&["hw:CARD=CODEC_1"]), None),
            "control: a twin id not cut short is not a prefix match"
        );
    }

    /// ALSA accepts a quoted card argument (`hw:CARD="CODEC"`): it names the same card.
    #[test]
    fn a_quoted_card_argument_is_the_card() {
        let tx = s(&["plughw:CARD=CODEC,DEV=0"]);
        assert!(alsa("hw:CARD=\"CODEC\",DEV=0", &tx, None));
        assert!(alsa("hw:'CODEC',0", &tx, None), "positional, single quotes");
        assert!(
            alsa("hw:CARD=CODEC", &s(&["dmix:CARD=\"CODEC\""]), None),
            "a quoted TX"
        );
        assert!(
            !alsa("hw:CARD=\"PCH\",DEV=0", &tx, None),
            "control: another card, quoted, plays"
        );
    }

    /// Linux: `default`, `pulse`, `pipewire` (and the other sound-server routes) all reach the
    /// sound server's default sink, which may be the rig's card: one routing target.
    #[test]
    fn the_sound_servers_names_are_one_routing_target() {
        for tx in ["pipewire", "pulse", "default", "jack", "sysdefault"] {
            for dev in ["default", "pulse", "pipewire", "Default"] {
                assert!(
                    alsa(dev, &s(&[tx]), Some("default")),
                    "TX {tx}, sound on {dev}"
                );
            }
        }
        assert!(
            !ep("pipewire", &s(&["Speakers"]), Some("Speakers")),
            "control: off Linux these are two device names like any other"
        );
    }

    /// Linux: the TX is a card by name, the sound goes to the sound server's default sink, which
    /// may be that card. Nexus cannot prove it is not, so it refuses, and says why.
    #[test]
    fn the_sound_servers_default_against_a_tx_card_is_refused_as_unprovable() {
        let tx = s(&["plughw:CARD=CODEC,DEV=0"]);
        assert!(alsa("default", &tx, Some("default")));
        assert_eq!(
            alsa_refuse("default", "", &tx, Some("default")),
            Err(PlayRefusal::TxUnsure)
        );
        assert_eq!(
            alsa_refuse("pipewire", "pipewire", &tx, Some("default")),
            Err(PlayRefusal::TxUnsure)
        );
        // The TX is the sound server's default; the sound names a card that may be its sink.
        assert_eq!(
            alsa_refuse(
                "hw:CARD=PCH,DEV=0",
                "hw:CARD=PCH,DEV=0",
                &s(&["pulse"]),
                Some("default")
            ),
            Err(PlayRefusal::TxUnsure)
        );
        assert_eq!(
            alsa_refuse(
                "hw:CARD=PCH,DEV=0",
                "hw:CARD=PCH,DEV=0",
                &tx,
                Some("default")
            ),
            Ok(()),
            "control: another card by name plays"
        );
    }

    /// Linux: a PCM whose routing lives in a config file (`~/.asoundrc`'s `rigtx`, `plug:rigtx`,
    /// `oss`, a bare `dmix`) cannot be proven different from any card or from the sound server's
    /// default, so a sound against one is refused, either way round. Only two different cards play.
    #[test]
    fn an_alsa_pcm_routed_by_a_config_file_is_never_proven_different() {
        let none = |_: u32| None;
        let unsure = Some(PlayRefusal::TxUnsure);
        // (a) TX on an asoundrc PCM, the sound on the system default.
        assert_eq!(
            verdict_with(
                "default",
                "",
                &s(&["rigtx"]),
                Some("default"),
                Naming::Alsa,
                &none
            ),
            unsure
        );
        // (b) TX on the codec by card, the sound on an asoundrc alias of that codec.
        for alias in [
            "js8out",
            "plug:rigtx",
            "oss",
            "dmix",
            "plughw",
            "plug:dmix:CODEC",
        ] {
            assert_eq!(
                verdict_with(
                    alias,
                    alias,
                    &s(&["plughw:CARD=CODEC,DEV=0"]),
                    Some("default"),
                    Naming::Alsa,
                    &none
                ),
                unsure,
                "sound on {alias}"
            );
        }
        // (c) TX on the system default, the sound on an asoundrc PCM.
        assert_eq!(
            verdict_with(
                "js8out",
                "js8out",
                &s(&[""]),
                Some("default"),
                Naming::Alsa,
                &none
            ),
            unsure
        );
        // Two different config-file PCMs may reach one card.
        assert_eq!(
            verdict_with(
                "js8out",
                "js8out",
                &s(&["rigtx"]),
                Some("default"),
                Naming::Alsa,
                &none
            ),
            unsure
        );
        assert_eq!(
            verdict_with(
                "hw:CARD=PCH,DEV=0",
                "hw:CARD=PCH,DEV=0",
                &s(&["plughw:CARD=CODEC,DEV=0"]),
                Some("default"),
                Naming::Alsa,
                &none
            ),
            None,
            "control: two different cards play"
        );
    }

    /// Off Linux a device name is only a name: one with a `:` is not read as an ALSA card, so its
    /// " #N" twin in a list is still the same device, and `pulse` is not a sound server. On Linux
    /// the ALSA reading applies.
    #[test]
    fn alsa_names_are_read_only_on_linux() {
        assert!(
            ep("Line:Out", &s(&["Line:Out #2"]), Some("Speakers")),
            "a name with a colon and its twin are one device"
        );
        assert!(!ep("pulse", &s(&["Rig CODEC"]), Some("pulse")), "control");
        assert_eq!(
            HOST_NAMING,
            if cfg!(target_os = "linux") {
                Naming::Alsa
            } else {
                Naming::Endpoint
            }
        );
    }

    /// Linux: `hw:1,0` names card 1 by number; the card's id (`/proc/asound/card1/id`) makes it
    /// the same card as `plughw:CARD=CODEC`. A number nobody can resolve may be any card.
    #[test]
    fn a_card_by_number_is_the_card_by_name() {
        let ids = |n: u32| (n == 1).then(|| "CODEC".to_string());
        let tx = s(&["plughw:CARD=CODEC,DEV=0"]);
        assert_eq!(
            verdict_with("hw:1,0", "", &tx, Some("default"), Naming::Alsa, &ids),
            Some(PlayRefusal::TxDevice)
        );
        assert_eq!(
            verdict_with(
                "plughw:CARD=CODEC,DEV=0",
                "",
                &s(&["hw:1"]),
                None,
                Naming::Alsa,
                &ids
            ),
            Some(PlayRefusal::TxDevice),
            "and the other way round"
        );
        assert_eq!(
            verdict_with("hw:0,0", "", &tx, Some("default"), Naming::Alsa, &|n| {
                (n == 0).then(|| "PCH".to_string())
            }),
            None,
            "control: card 0 is another card"
        );
        assert_eq!(
            verdict_with("hw:5,0", "", &tx, Some("default"), Naming::Alsa, &|_| None),
            Some(PlayRefusal::TxUnsure),
            "a number that names no card here cannot be told apart"
        );
        assert_eq!(
            verdict_with(
                "hw:2,0",
                "",
                &s(&["plughw:2,0"]),
                None,
                Naming::Alsa,
                &|_| None
            ),
            Some(PlayRefusal::TxDevice),
            "the same number is the same card, resolved or not"
        );
    }

    /// A TX device set to "the system default" (or a radio profile with none, as with a radio whose
    /// audio is not on this computer) refuses the default output, and says THAT rather than
    /// calling the output a TX device.
    #[test]
    fn a_default_tx_device_is_named_as_the_reason() {
        assert_eq!(
            ep_refuse("Speakers", "", &s(&["Rig CODEC", ""]), Some("Speakers")),
            Err(PlayRefusal::TxDefault)
        );
        assert_eq!(
            ep_refuse("Headphones", "Headphones", &s(&[""]), None),
            Err(PlayRefusal::TxDefault),
            "a default nobody can name"
        );
        assert_eq!(
            alsa_refuse("default", "", &s(&[""]), Some("default")),
            Err(PlayRefusal::TxDefault),
            "Linux: both the sound server's default"
        );
        assert_eq!(
            ep_refuse(
                "Rig CODEC",
                "Rig CODEC",
                &s(&["", "Rig CODEC"]),
                Some("Speakers")
            ),
            Err(PlayRefusal::TxDevice),
            "a named TX device is the stronger reason"
        );
        assert_eq!(
            ep_refuse("Headphones", "Headphones", &s(&[""]), Some("Speakers")),
            Ok(()),
            "control"
        );
    }

    #[test]
    fn smartsdrs_dax_tx_is_refused_whatever_is_configured() {
        let tx = s(&["Speakers"]);
        assert!(ep("DAX Audio TX (FlexRadio Systems DAX TX)", &tx, None));
        assert!(
            !ep("DAX Audio RX 1 (FlexRadio Systems DAX Audio)", &tx, None),
            "control: DAX RX"
        );
    }

    #[test]
    fn a_device_with_no_name_is_refused() {
        assert!(ep("  ", &s(&["Rig"]), Some("Speakers")));
    }

    /// The radio opened its TX output while it was "the system default", and the default moved
    /// later: the radio still transmits on the device it opened, which the settings no longer name.
    /// A sound on that device is refused for as long as the radio holds it.
    #[test]
    fn the_tx_output_the_radio_holds_open_is_refused_whatever_settings_say() {
        let tx = s(&[""]);
        let codec = "Nexus Test Held CODEC";
        assert_eq!(
            ep_refuse(codec, codec, &guarded_tx_devices(&tx), Some("Headphones")),
            Ok(()),
            "premise: by the settings alone it plays"
        );
        let held = hold_tx_output(codec.to_string());
        assert_eq!(
            ep_refuse(codec, codec, &guarded_tx_devices(&tx), Some("Headphones")),
            Err(PlayRefusal::TxDevice)
        );
        assert_eq!(
            ep_refuse(
                "Headphones",
                "",
                &guarded_tx_devices(&s(&["Rig CODEC"])),
                Some("Headphones")
            ),
            Ok(()),
            "control: another output plays"
        );
        drop(held);
        assert!(!open_tx_outputs().iter().any(|n| n == codec));
        assert_eq!(
            ep_refuse(codec, codec, &guarded_tx_devices(&tx), Some("Headphones")),
            Ok(()),
            "released when the radio lets it go"
        );
    }

    /// The setting moves AFTER the sound's device was picked: the next play is refused, because
    /// the guard reads the settings at each play.
    #[test]
    fn a_tx_device_changed_after_the_sound_device_was_picked_is_caught() {
        let mut settings = Settings {
            audio_out: "Rig CODEC".into(),
            ..Default::default()
        };
        let picked = "Speakers";
        assert!(
            !ep(picked, &tx_audio_devices(&settings), None),
            "premise: it plays"
        );
        settings.audio_out = "Speakers".into();
        assert!(ep(picked, &tx_audio_devices(&settings), None));
        settings.audio_out = "Rig CODEC".into();
        settings.radios.push(tempo_app::settings::RadioProfile {
            audio_out: "SPEAKERS".into(),
            ..Default::default()
        });
        assert!(
            ep(picked, &tx_audio_devices(&settings), None),
            "a radio added later"
        );
    }

    /// The device that opens is checked, not only the name it was asked by: an empty choice that
    /// opens the default, or a name that resolves to the TX device, is refused either way.
    #[test]
    fn the_device_that_opens_and_the_name_asked_are_both_checked() {
        let tx = s(&["USB Audio CODEC"]);
        assert_eq!(
            ep_refuse("USB Audio CODEC", "", &tx, Some("USB Audio CODEC")),
            Err(PlayRefusal::TxDevice)
        );
        assert_eq!(
            ep_refuse("USB Audio CODEC", "Codec (renamed)", &tx, None),
            Err(PlayRefusal::TxDevice)
        );
        assert_eq!(
            ep_refuse("Speakers", "usb audio codec", &tx, None),
            Err(PlayRefusal::TxDevice)
        );
        assert_eq!(
            ep_refuse("Speakers", "Speakers", &tx, None),
            Ok(()),
            "control"
        );
        assert_eq!(
            ep_refuse("Speakers", "", &tx, Some("Speakers")),
            Ok(()),
            "control: the default"
        );
    }

    /// Every play passes the check before its stream is built (the cpal half cannot run headless,
    /// so its order is held here by its source).
    #[test]
    fn every_play_is_checked_before_its_stream_is_built() {
        let src = include_str!("notify_sound.rs");
        let body: String = src[src.find("fn open_and_start(").expect("the play path")..]
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let checked = body
            .find("refuse_tx(&opened,device,&guarded_tx_devices(tx_devices),")
            .expect("the check, against the settings and the outputs the radio holds");
        let built = body.find("dispatch_format!(").expect("the stream");
        let played = body.find(".play()").expect("the start");
        assert!(checked < built && built < played);
        let fallback = format!(".or_else(|| host.{}())", "default_output_device");
        assert!(
            !src.contains(&fallback),
            "no silent fallback to the default"
        );
    }

    /// The radio's audio open names the TX output it resolved for as long as the backend lives
    /// (the cpal half cannot run headless, so this is held by the source): the hold is taken from
    /// the device that resolved, before it moves into the deferred output, and kept in the backend.
    #[test]
    fn the_radios_open_holds_the_tx_output_it_resolved() {
        let src: String = include_str!("device.rs")
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let open = &src[src
            .find("letout_dev=matchsame_card")
            .expect("the output resolves")..];
        let held = open
            .find(
                "lettx_output_held=out_dev.device_name().map(crate::notify_sound::hold_tx_output);",
            )
            .expect("the resolved output is held");
        let moved = open
            .find("dev:out_dev,")
            .expect("it moves to the deferred output");
        let kept = open
            .find("_tx_output_held:tx_output_held,")
            .expect("the backend keeps the hold");
        assert!(held < moved && moved < kept);
    }

    /// A sound's stream is torn down under the host lock, as every other cpal teardown is
    /// (`device.rs`'s AUDIO_HOST_LOCK: an unlocked teardown racing an enumeration or an open faults
    /// natively). The cpal half cannot run headless, so this is held by the source: the play
    /// thread closes its stream only through `close_locked`, which takes the lock before the drop.
    #[test]
    fn a_sounds_stream_is_closed_under_the_host_lock() {
        let src = include_str!("notify_sound.rs");
        let play = &src[src.find("pub fn play(").expect("the play path")..];
        let play = &play[..play.find("fn close_locked(").expect("the helper")];
        assert!(
            play.contains("close_locked(stream)"),
            "the play thread closes through the locked helper"
        );
        assert!(
            !play.contains(&format!("{}(stream)", "drop")),
            "no unlocked drop of the stream"
        );
        let helper = &src[src.find("fn close_locked(").expect("the helper")..];
        let helper = &helper[..helper.find("\n    }\n").expect("its end")];
        let locked = helper.find("AUDIO_HOST_LOCK").expect("takes the lock");
        let dropped = helper
            .find(&format!("{}(stream)", "drop"))
            .expect("drops it");
        assert!(locked < dropped, "the lock is taken before the drop");
    }

    /// What reaches the output is at most MAX_PLAY_SECONDS, whatever the caller hands in and
    /// whatever rate the device runs at, so a sound neither holds its output nor its memory long.
    #[test]
    fn a_sound_is_cut_to_the_longest_play_at_the_devices_rate() {
        let long = vec![0.1f32; (8_000 * (MAX_PLAY_SECONDS + 5)) as usize];
        assert_eq!(
            clip_for_output(&long, 8_000, 48_000).len(),
            (48_000 * MAX_PLAY_SECONDS) as usize
        );
        assert_eq!(
            clip_for_output(&long, 8_000, 8_000).len(),
            (8_000 * MAX_PLAY_SECONDS) as usize,
            "the same rate too"
        );
        let short = tone(587.0, false);
        let out = clip_for_output(&short, TONE_RATE, 96_000);
        assert_eq!(out.len(), short.len() * 2, "control: a tone plays whole");
    }

    /// One sound at a time: a play while another is playing is dropped, and the next one after
    /// it ends plays.
    #[test]
    fn overlapping_sounds_play_one_at_a_time() {
        let first = begin_play().expect("nothing playing");
        assert!(begin_play().is_none(), "a second while the first plays");
        drop(first);
        assert!(begin_play().is_some(), "the next after it ends");
    }

    #[test]
    fn a_play_holds_its_turn_until_its_stream_is_closed() {
        let src = include_str!("notify_sound.rs");
        let play = &src[src.find("pub fn play(").expect("the play path")..];
        let play = &play[..play.find("fn close_locked(").expect("the helper")];
        let turn = play.find("begin_play()").expect("takes its turn");
        let spawned = play.find(".spawn(").expect("the thread");
        let closed = play.find("close_locked(stream)").expect("closes");
        let released = play.rfind("drop(turn)").expect("gives its turn back");
        assert!(turn < spawned && closed < released);
    }

    #[test]
    fn the_built_in_tones_are_short_quiet_and_double_where_asked() {
        let one = tone(587.0, false);
        assert_eq!(one.len(), (0.24 * TONE_RATE as f32) as usize);
        let peak = one.iter().fold(0f32, |m, v| m.max(v.abs()));
        assert!(peak > 0.1 && peak <= 0.18 + 1e-3, "peak {peak}");
        let two = tone(784.0, true);
        assert!(two.len() > one.len());
        assert!(two.iter().all(|v| v.abs() <= 1.0));
    }
}
