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
//! - the same ALSA card as one of those, reached by another name (`hw:` against `plughw:`, or by
//!   its number, `hw:1,0`, resolved to the card's id);
//! - the system default output, when a TX device is configured as "system default" (empty);
//! - one that cannot be told apart from a TX device by name (Linux): the sound server's default
//!   sink (`default`, `pulse`, `pipewire`… are ONE target) against a card named outright, either
//!   way round, since that sink may be the rig's card. When the guard cannot prove two devices
//!   different, it refuses.
//!
//! The check is made at EVERY play, against the settings as they are then and the device that
//! actually opened, so a TX device changed after the sound device was picked is caught at the next
//! sound. Two devices are compared by name, case-blind, the way the headphone monitor's guard
//! compares them, and without the " #N" a device list adds to a second identically named codec:
//! two identically named codecs are therefore one device here, which refuses more rather than less.
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
    /// The two cannot be told apart by name, so the sound is refused: on Linux, the sound server's
    /// default sink (`default`, `pulse`, `pipewire`…) against a sound card named outright, either
    /// way round, or a card by a number this computer cannot resolve.
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

/// Where a device name sends its sound, as far as its name can tell.
#[derive(Debug, PartialEq, Eq)]
enum Route {
    /// The sound server's default sink.
    Server,
    /// One ALSA card, by any access path (`hw:`, `plughw:`, `dmix:`, `front:`…).
    Card(Card),
    /// Any other name (Windows / macOS): its base name, lower-cased, without the " #N" ordinal
    /// that tells two identically named codecs apart in a list — they are one device here.
    Named(String),
}

/// Two names collide: the same device, or two the guard cannot prove different.
#[derive(Debug, PartialEq, Eq)]
enum Collision {
    Same,
    Unsure,
}

/// The route of `name`, a card number resolved to its id through `card_id`.
fn route(name: &str, card_id: &dyn Fn(u32) -> Option<String>) -> Route {
    let name = name.trim();
    if let Some(at) = name.find("CARD=") {
        let rest = &name[at + 5..];
        let id = &rest[..rest.find(',').unwrap_or(rest.len())];
        return Route::Card(card_ref(id, card_id));
    }
    // An ALSA PCM name has no spaces; a Windows or macOS friendly name nearly always has.
    if !name.contains(char::is_whitespace) {
        let (plugin, args) = name.split_once(':').unwrap_or((name, ""));
        let plugin = plugin.to_ascii_lowercase();
        let first = args.split(',').next().unwrap_or("").trim();
        if !first.is_empty() && !first.contains('=') {
            // `hw:1,0`, `plughw:CODEC`, `front:1`: the first argument is the card.
            return Route::Card(card_ref(first, card_id));
        }
        if args.is_empty() && SOUND_SERVER_ROUTES.contains(&plugin.as_str()) {
            return Route::Server;
        }
    }
    let lower = name.to_ascii_lowercase();
    Route::Named(
        crate::audiodev::split_device_ordinal(&lower)
            .0
            .trim()
            .to_string(),
    )
}

fn card_ref(token: &str, card_id: &dyn Fn(u32) -> Option<String>) -> Card {
    match token.parse::<u32>() {
        Ok(n) => card_id(n)
            .map(|id| Card::Id(id.trim().to_ascii_lowercase()))
            .unwrap_or(Card::Number(n)),
        Err(_) => Card::Id(token.to_ascii_lowercase()),
    }
}

fn collide(a: &Route, b: &Route) -> Option<Collision> {
    match (a, b) {
        (Route::Server, Route::Server) => Some(Collision::Same),
        (Route::Server, Route::Card(_)) | (Route::Card(_), Route::Server) => {
            Some(Collision::Unsure)
        }
        (Route::Card(x), Route::Card(y)) => match (x, y) {
            (Card::Id(x), Card::Id(y)) if x == y => Some(Collision::Same),
            (Card::Number(x), Card::Number(y)) if x == y => Some(Collision::Same),
            (Card::Id(_), Card::Number(_)) | (Card::Number(_), Card::Id(_)) => {
                Some(Collision::Unsure)
            }
            _ => None,
        },
        (Route::Named(x), Route::Named(y)) if x == y => Some(Collision::Same),
        _ => None,
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
    let dev = route(device, card_id);
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
                Some(d) => collide(&dev, &route(d, card_id)).map(|c| match c {
                    Collision::Same => PlayRefusal::TxDefault,
                    Collision::Unsure => PlayRefusal::TxUnsure,
                }),
            }
        } else {
            collide(&dev, &route(tx, card_id)).map(|c| match c {
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

/// [`refuse_tx`] with the card-number lookup handed in (tests name their own cards).
fn verdict_with(
    opened: &str,
    configured: &str,
    tx_devices: &[String],
    default_output: Option<&str>,
    card_id: &dyn Fn(u32) -> Option<String>,
) -> Option<PlayRefusal> {
    let a = device_verdict(opened, tx_devices, default_output, card_id);
    let b = (!configured.trim().is_empty())
        .then(|| device_verdict(configured, tx_devices, default_output, card_id))
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
    device_verdict(device, tx_devices, default_output, &system_card_id).is_some()
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

#[cfg(feature = "device")]
pub use device_play::play;

#[cfg(feature = "device")]
mod device_play {
    use super::{refuse_tx, PlayRefusal};
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
    /// stream (a cpal `Stream` is `!Send`).
    pub fn play(
        device: &str,
        tx_devices: Vec<String>,
        rate: u32,
        samples: Vec<f32>,
    ) -> Result<(), PlayRefusal> {
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
                        let _ = tx.send(Err(e));
                        return;
                    }
                };
                let _ = tx.send(Ok(()));
                if ok {
                    std::thread::sleep(std::time::Duration::from_secs_f32(secs + 0.25));
                }
                close_locked(stream);
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
        refuse_tx(&opened, device, tx_devices, default_output.as_deref())?;
        let supported = dev
            .default_output_config()
            .map_err(|e| PlayRefusal::Failed(e.to_string()))?;
        let out_rate = supported.sample_rate();
        let out_ch = usize::from(supported.channels()).max(1);
        let config: cpal::StreamConfig = supported.config();
        let clip: Arc<Vec<f32>> = Arc::new(resample(samples, rate, out_rate));
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

    /// Linear resampling from `from` to `to` samples a second.
    fn resample(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
        if from == to || samples.is_empty() {
            return samples.to_vec();
        }
        let n = (samples.len() as u64 * u64::from(to) / u64::from(from.max(1))) as usize;
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn the_tx_device_is_refused_by_any_spelling_of_its_name() {
        let tx = s(&["USB Audio CODEC"]);
        assert!(would_transmit("USB Audio CODEC", &tx, Some("Speakers")));
        assert!(would_transmit("usb audio codec ", &tx, Some("Speakers")));
        assert!(
            !would_transmit("Speakers", &tx, Some("Speakers")),
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
        assert!(would_transmit("Rig A CODEC", &tx, None));
        assert!(
            would_transmit("Rig B CODEC", &tx, None),
            "the radio not shown"
        );
        assert!(!would_transmit("Headphones", &tx, None), "control");
    }

    #[test]
    fn a_tx_device_set_to_the_system_default_refuses_the_default() {
        let tx = s(&[""]);
        assert!(would_transmit("Speakers", &tx, Some("Speakers")));
        assert!(
            !would_transmit("Headphones", &tx, Some("Speakers")),
            "control"
        );
        assert!(
            would_transmit("Headphones", &tx, None),
            "a default nobody can name might be any device"
        );
    }

    #[test]
    fn the_same_alsa_card_by_another_name_is_the_same_device() {
        let tx = s(&["plughw:CARD=CODEC,DEV=0"]);
        assert!(would_transmit("hw:CARD=CODEC,DEV=0", &tx, None));
        assert!(would_transmit("dmix:CARD=CODEC,DEV=0", &tx, None));
        assert!(
            !would_transmit("hw:CARD=PCH,DEV=0", &tx, None),
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
            would_transmit("USB Audio CODEC", &tx, Some("Speakers")),
            "TX is the second codec, the opened one is named bare"
        );
        assert!(
            would_transmit("usb audio codec #3", &s(&["USB Audio CODEC"]), None),
            "and the other way round"
        );
        assert_eq!(
            refuse_tx("USB Audio CODEC", "", &tx, Some("USB Audio CODEC")),
            Err(PlayRefusal::TxDevice),
            "the default output is that codec, nothing configured"
        );
        assert!(
            !would_transmit("Speakers", &tx, Some("Speakers")),
            "control: another device plays"
        );
        assert!(
            !would_transmit("Speakers #2", &tx, Some("Speakers")),
            "control: a second pair of speakers is not the codec"
        );
    }

    /// Linux: `default`, `pulse`, `pipewire` (and the other sound-server routes) all reach the
    /// sound server's default sink, which may be the rig's card: one routing target.
    #[test]
    fn the_sound_servers_names_are_one_routing_target() {
        for tx in ["pipewire", "pulse", "default", "jack", "sysdefault"] {
            for dev in ["default", "pulse", "pipewire", "Default"] {
                assert!(
                    would_transmit(dev, &s(&[tx]), Some("default")),
                    "TX {tx}, sound on {dev}"
                );
            }
        }
        assert!(
            !would_transmit("Speakers", &s(&["pipewire"]), Some("Speakers")),
            "control: a name of another system is not a route"
        );
    }

    /// Linux: the TX is a card by name, the sound goes to the sound server's default sink, which
    /// may be that card. Nexus cannot prove it is not, so it refuses, and says why.
    #[test]
    fn the_sound_servers_default_against_a_tx_card_is_refused_as_unprovable() {
        let tx = s(&["plughw:CARD=CODEC,DEV=0"]);
        assert!(would_transmit("default", &tx, Some("default")));
        assert_eq!(
            refuse_tx("default", "", &tx, Some("default")),
            Err(PlayRefusal::TxUnsure)
        );
        assert_eq!(
            refuse_tx("pipewire", "pipewire", &tx, Some("default")),
            Err(PlayRefusal::TxUnsure)
        );
        // The TX is the sound server's default; the sound names a card that may be its sink.
        assert_eq!(
            refuse_tx(
                "hw:CARD=PCH,DEV=0",
                "hw:CARD=PCH,DEV=0",
                &s(&["pulse"]),
                Some("default")
            ),
            Err(PlayRefusal::TxUnsure)
        );
        assert_eq!(
            refuse_tx(
                "hw:CARD=PCH,DEV=0",
                "hw:CARD=PCH,DEV=0",
                &tx,
                Some("default")
            ),
            Ok(()),
            "control: another card by name plays"
        );
    }

    /// Linux: `hw:1,0` names card 1 by number; the card's id (`/proc/asound/card1/id`) makes it
    /// the same card as `plughw:CARD=CODEC`. A number nobody can resolve may be any card.
    #[test]
    fn a_card_by_number_is_the_card_by_name() {
        let ids = |n: u32| (n == 1).then(|| "CODEC".to_string());
        let tx = s(&["plughw:CARD=CODEC,DEV=0"]);
        assert_eq!(
            verdict_with("hw:1,0", "", &tx, Some("default"), &ids),
            Some(PlayRefusal::TxDevice)
        );
        assert_eq!(
            verdict_with("plughw:CARD=CODEC,DEV=0", "", &s(&["hw:1"]), None, &ids),
            Some(PlayRefusal::TxDevice),
            "and the other way round"
        );
        assert_eq!(
            verdict_with("hw:0,0", "", &tx, Some("default"), &|n| {
                (n == 0).then(|| "PCH".to_string())
            }),
            None,
            "control: card 0 is another card"
        );
        assert_eq!(
            verdict_with("hw:5,0", "", &tx, Some("default"), &|_| None),
            Some(PlayRefusal::TxUnsure),
            "a number that names no card here cannot be told apart"
        );
        assert_eq!(
            verdict_with("hw:2,0", "", &s(&["plughw:2,0"]), None, &|_| None),
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
            refuse_tx("Speakers", "", &s(&["Rig CODEC", ""]), Some("Speakers")),
            Err(PlayRefusal::TxDefault)
        );
        assert_eq!(
            refuse_tx("Headphones", "Headphones", &s(&[""]), None),
            Err(PlayRefusal::TxDefault),
            "a default nobody can name"
        );
        assert_eq!(
            refuse_tx("default", "", &s(&[""]), Some("default")),
            Err(PlayRefusal::TxDefault),
            "Linux: both the sound server's default"
        );
        assert_eq!(
            refuse_tx(
                "Rig CODEC",
                "Rig CODEC",
                &s(&["", "Rig CODEC"]),
                Some("Speakers")
            ),
            Err(PlayRefusal::TxDevice),
            "a named TX device is the stronger reason"
        );
        assert_eq!(
            refuse_tx("Headphones", "Headphones", &s(&[""]), Some("Speakers")),
            Ok(()),
            "control"
        );
    }

    #[test]
    fn smartsdrs_dax_tx_is_refused_whatever_is_configured() {
        let tx = s(&["Speakers"]);
        assert!(would_transmit(
            "DAX Audio TX (FlexRadio Systems DAX TX)",
            &tx,
            None
        ));
        assert!(
            !would_transmit("DAX Audio RX 1 (FlexRadio Systems DAX Audio)", &tx, None),
            "control: DAX RX"
        );
    }

    #[test]
    fn a_device_with_no_name_is_refused() {
        assert!(would_transmit("  ", &s(&["Rig"]), Some("Speakers")));
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
            !would_transmit(picked, &tx_audio_devices(&settings), None),
            "premise: it plays"
        );
        settings.audio_out = "Speakers".into();
        assert!(would_transmit(picked, &tx_audio_devices(&settings), None));
        settings.audio_out = "Rig CODEC".into();
        settings.radios.push(tempo_app::settings::RadioProfile {
            audio_out: "SPEAKERS".into(),
            ..Default::default()
        });
        assert!(
            would_transmit(picked, &tx_audio_devices(&settings), None),
            "a radio added later"
        );
    }

    /// The device that opens is checked, not only the name it was asked by: an empty choice that
    /// opens the default, or a name that resolves to the TX device, is refused either way.
    #[test]
    fn the_device_that_opens_and_the_name_asked_are_both_checked() {
        let tx = s(&["USB Audio CODEC"]);
        assert_eq!(
            refuse_tx("USB Audio CODEC", "", &tx, Some("USB Audio CODEC")),
            Err(PlayRefusal::TxDevice)
        );
        assert_eq!(
            refuse_tx("USB Audio CODEC", "Codec (renamed)", &tx, None),
            Err(PlayRefusal::TxDevice)
        );
        assert_eq!(
            refuse_tx("Speakers", "usb audio codec", &tx, None),
            Err(PlayRefusal::TxDevice)
        );
        assert_eq!(
            refuse_tx("Speakers", "Speakers", &tx, None),
            Ok(()),
            "control"
        );
        assert_eq!(
            refuse_tx("Speakers", "", &tx, Some("Speakers")),
            Ok(()),
            "control: the default"
        );
    }

    /// Every play passes the check before its stream is built (the cpal half cannot run headless,
    /// so its order is held here by its source).
    #[test]
    fn every_play_is_checked_before_its_stream_is_built() {
        let src = include_str!("notify_sound.rs");
        let body = &src[src.find("fn open_and_start(").expect("the play path")..];
        let checked = body
            .find("refuse_tx(&opened, device, tx_devices")
            .expect("the check");
        let built = body.find("dispatch_format!(").expect("the stream");
        let played = body.find(".play()").expect("the start");
        assert!(checked < built && built < played);
        let fallback = format!(".or_else(|| host.{}())", "default_output_device");
        assert!(
            !src.contains(&fallback),
            "no silent fallback to the default"
        );
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
