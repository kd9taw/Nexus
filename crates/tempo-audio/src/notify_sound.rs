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
//! - the same ALSA card as one of those, reached by another name (`hw:` against `plughw:`);
//! - the system default output, when a TX device is configured as "system default" (empty).
//!
//! The check is made at EVERY play, against the settings as they are then and the device that
//! actually opened, so a TX device changed after the sound device was picked is caught at the next
//! sound. Two devices are compared by name, case-blind, the way the headphone monitor's guard
//! compares them; two identically named codecs are therefore one device here, which refuses more
//! rather than less.
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

/// The ALSA `CARD=<id>` of a device name, lower-cased, if it has one.
fn alsa_card(name: &str) -> Option<String> {
    let at = name.find("CARD=")?;
    let rest = &name[at..];
    Some(rest[..rest.find(',').unwrap_or(rest.len())].to_ascii_lowercase())
}

/// THE GUARD, as a pure predicate: whether playing on `device` (the name of the device that
/// opens, never empty) would play into one of `tx_devices`, where an empty TX name is the system
/// default output, `default_output` (`None` when the host cannot name one).
pub fn would_transmit(device: &str, tx_devices: &[String], default_output: Option<&str>) -> bool {
    let device = device.trim();
    if device.is_empty() {
        return true; // a device with no name cannot be told apart from the TX device
    }
    // SmartSDR's DAX TX, whatever is configured (the monitor's rule).
    if crate::monitor::is_flex_dax_tx_device(device) {
        return true;
    }
    tx_devices.iter().any(|tx| {
        let tx = tx.trim();
        let tx = if tx.is_empty() {
            match default_output.map(str::trim).filter(|d| !d.is_empty()) {
                Some(d) => d,
                // A TX device that is "the default" on a host that cannot name its default: no
                // output can be shown to differ from it.
                None => return true,
            }
        } else {
            tx
        };
        device.eq_ignore_ascii_case(tx)
            || alsa_card(device).is_some_and(|c| alsa_card(tx).as_deref() == Some(c.as_str()))
    })
}

/// The check made before a sound's stream is built: the device that OPENED (`opened`) and the name
/// it was asked by (`configured`, empty for the default) may neither be a TX device. Every play
/// passes here, with the TX devices as the settings hold them at that play.
pub fn refuse_tx(
    opened: &str,
    configured: &str,
    tx_devices: &[String],
    default_output: Option<&str>,
) -> Result<(), PlayRefusal> {
    if would_transmit(opened, tx_devices, default_output)
        || (!configured.trim().is_empty() && would_transmit(configured, tx_devices, default_output))
    {
        return Err(PlayRefusal::TxDevice);
    }
    Ok(())
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
                drop(stream);
            })
            .map_err(|e| PlayRefusal::Failed(e.to_string()))?;
        rx.recv()
            .unwrap_or_else(|_| Err(PlayRefusal::Failed("the sound thread ended".into())))
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
        assert!(!src.contains(&fallback), "no silent fallback to the default");
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
