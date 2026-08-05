//! What the operator picks from: the audio-device list, and the rules that build it.
//!
//! Pure string policy — no cpal, no alsa-lib, no device access — so it is unit-testable
//! on every platform (including the Windows/macOS CI runners, which have no alsa-lib at
//! all, and the headless `cargo test --workspace` build, which has no `device` feature).
//! The live enumeration that feeds it lives in [`crate::device::available_devices`],
//! the same split [`crate::usbrig::detect_rigs`] uses for USB ports.
//!
//! ## The bug this module exists for
//!
//! cpal's ALSA host names a device by its PCM **hint name** and throws the human
//! **description** away (`cpal-0.15.3/src/host/alsa/enumerate.rs:25-47` keeps
//! `hint.name`, discards `desc` and `direction`), and it probe-OPENS every hint in both
//! directions, silently discarding whatever will not open (`mod.rs:185-197`,
//! `traits.rs:61-83`). On a PipeWire desktop that reduced a reported 8-card machine to
//! ONE card, listed under raw `hw:` / `dmix:` / `dsnoop:` / `iec958:` names, while the
//! log filled with `unable to open slave`, `dmix supports only playback` and
//! `Cannot open device /dev/dsp` — the sweep failing in real time. The operator's FTDX10
//! codec, which his system, `aplay -l`, KDE, pavucontrol and WSJT-X all call
//! `USB AUDIO CODEC`, was simply absent.
//!
//! A third casualty nobody reported: [`crate::usbrig::pair_audio`] matches on
//! `"USB AUDIO CODEC"` / `"USB PNP SOUND DEVICE"` / `"USB AUDIO"`. Against
//! `hw:CARD=CODEC,DEV=0` every pattern misses, so zero-config rig-audio pairing has never
//! worked on Linux — every Linux operator has been wiring audio by hand without knowing
//! he should not have to. Naming devices by their description repairs that too.

use std::collections::HashMap;

/// One selectable audio device: the string that ADDRESSES it and the string the operator
/// READS.
///
/// They are identical on Windows and macOS, where cpal's `Device::name()` is already the
/// friendly name the OS shows (WASAPI reports `DEVPKEY_Device_FriendlyName`, CoreAudio
/// `kAudioDevicePropertyDeviceNameCFString`). They differ on Linux, where `name` is the
/// ALSA PCM name (`plughw:CARD=CODEC,DEV=0`) and `label` its description
/// (`USB AUDIO CODEC`).
///
/// ⚠️ **`name` is what settings.json stores and what [`crate::device::pick_device`]
/// resolves at open time. `label` is display-only and must NEVER be persisted** — two
/// identical rig codecs both describe themselves as "USB AUDIO CODEC", so a label cannot
/// identify a device, and storing one would invalidate every existing config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioDevice {
    /// Identity. Stored in settings; resolved against the host at open time.
    pub name: String,
    /// Display only. Never stored.
    pub label: String,
}

/// Direction of an ALSA PCM hint (its `IOID` property).
///
/// ALSA has no third value: a PCM usable BOTH ways simply omits `IOID`, and
/// `alsa::device_name::Hint` (alsa-0.9.1 `src/device_name.rs:73-77`) maps only
/// `"Input"`/`"Output"` — anything else, missing included, becomes `None`. So `None`
/// means "both", and it is also the safe reading of a value we do not understand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HintDir {
    Capture,
    Playback,
}

/// One ALSA PCM hint as `snd_device_name_hint` reports it. Mirrors
/// `alsa::device_name::Hint` without the crate type, so [`prune_alsa_hints`] stays pure.
#[derive(Clone, Debug)]
pub struct PcmHint {
    /// The `NAME` property — the ALSA PCM name, e.g. `plughw:CARD=CODEC,DEV=0`.
    pub name: String,
    /// The `DESC` property. A card-bound PCM carries two lines: `"<card>, <pcm>"`, then a
    /// generic per-plugin blurb from `/usr/share/alsa/pcm/*.conf` ("Direct hardware device
    /// without any conversions"). Only line 1 names the hardware. A card-less software PCM
    /// has a single line ("PulseAudio Sound Server").
    pub desc: Option<String>,
    /// `None` = no `IOID` = usable both ways, so it belongs in BOTH lists.
    pub direction: Option<HintDir>,
}

/// Turn ALSA's raw PCM hints into the two lists the operator picks from: `(input, output)`.
///
/// The policy, in order — each rule's risk is noted because they are judgement calls a
/// maintainer may want to overrule:
///
/// 1. **Direction comes from `IOID`, not from a probe-open.** Nothing is opened, so a card
///    another application is holding can no longer vanish from both lists, and the ALSA
///    error flood disappears by construction rather than by silencing. Unknown reads as
///    "both", so the failure direction is inclusive: the worst case is an entry that fails
///    at OPEN time, where [`crate::device::resolve_configured`] now says so out loud.
/// 2. **One survivor per `(CARD, DEV)`**, ranked `plughw` before `hw` before `default`
///    before `sysdefault` before anything else. `plughw:` format-converts; bare `hw:` is
///    unforgiving about a rig codec's native rate/format. *Risk:* an existing config's `hw:` is
///    no longer IN the offered list — it stays selectable because the pickers prepend a
///    stored-but-unenumerated value, and it still resolves, because open-time lookup goes
///    through cpal's full list, not this pruned one.
/// 3. **Card-bound plugin aliases are denied by prefix** (`dmix`, `dsnoop`, `front`,
///    `surround*`, `iec958`, `hdmi`, `usbstream`, …): the same hardware behind a plugin,
///    already covered by `plughw:`. *Risk:* `dmix`/`dsnoop` are the SHAREABLE paths, so an
///    operator who wants to share his codec with another app loses that entry;
///    `default:CARD=x` (kept by rule 2 when nothing better exists) is the equivalent.
/// 4. **Card-less PCMs are filtered by DENY-list, never allow-list.** `default`, `pulse`,
///    `pipewire` and `jack` are frequently the RIGHT answer on a PipeWire desktop, and an
///    operator's hand-written `~/.asoundrc` PCM must survive. Only pure plumbing (`null`,
///    `oss`, the rate converters, the bare argument-less plugin names) is removed.
///    *Risk:* a future ALSA plugin shows up as one noise entry — chosen over silently
///    deleting somebody's hand-built rig PCM.
/// 5. **Label = the first line of `DESC`**, which is the exact string `aplay -l`, KDE and
///    pavucontrol show. Never invented: a hint with no description falls back to its name.
/// 6. **Hardware first** (in ALSA's hint order, which is card-index order), routing
///    endpoints last. Not only presentation — [`crate::usbrig::pair_audio`] takes the
///    first match.
pub fn prune_alsa_hints(hints: &[PcmHint]) -> (Vec<AudioDevice>, Vec<AudioDevice>) {
    (
        prune_one_direction(hints, HintDir::Capture),
        prune_one_direction(hints, HintDir::Playback),
    )
}

/// One direction's list. See [`prune_alsa_hints`] for the policy and its risks.
fn prune_one_direction(hints: &[PcmHint], want: HintDir) -> Vec<AudioDevice> {
    // Card-bound survivors, keyed by (CARD, DEV) with the best prefix rank seen so far.
    let mut cards: Vec<((String, u32), u8, AudioDevice)> = Vec::new();
    // Card-less PCMs — routing endpoints and custom ~/.asoundrc entries — in hint order.
    let mut routes: Vec<AudioDevice> = Vec::new();
    for h in hints {
        // `None` = no IOID = usable both ways (rule 1).
        if h.direction.is_some_and(|d| d != want) {
            continue;
        }
        let (prefix, card) = split_pcm_name(&h.name);
        let dev = AudioDevice {
            name: h.name.clone(),
            label: hint_label(&h.name, h.desc.as_deref()),
        };
        match card {
            Some((card_id, dev_no)) => {
                if denied_card_prefix(prefix) {
                    continue;
                }
                let key = (card_id.to_string(), dev_no);
                let rank = card_prefix_rank(prefix);
                match cards.iter_mut().find(|(k, _, _)| *k == key) {
                    Some(slot) if rank < slot.1 => {
                        slot.1 = rank;
                        slot.2 = dev;
                    }
                    Some(_) => {}
                    None => cards.push((key, rank, dev)),
                }
            }
            None => {
                if denied_bare_pcm(&h.name) {
                    continue;
                }
                routes.push(dev);
            }
        }
    }
    // `sysdefault` is `default` with the same description — offer one of them.
    if routes.iter().any(|d| d.name == "default") {
        routes.retain(|d| d.name != "sysdefault");
    }
    let mut out: Vec<AudioDevice> = cards.into_iter().map(|(_, _, d)| d).collect();
    shorten_card_labels(&mut out);
    out.extend(routes); // hardware first, routing endpoints last (rule 6)
    disambiguate_labels(&mut out);
    out
}

/// The operator-facing name for a hint: the FIRST line of its description, which ALSA
/// formats as `"<card name>, <PCM name>"`. Line 2 is the generic per-plugin blurb, which
/// describes the PLUGIN, not the hardware, so it is dropped. Falls back to the PCM name
/// when a hint carries no usable description — this never invents a string.
fn hint_label(name: &str, desc: Option<&str>) -> String {
    desc.and_then(|d| d.lines().next())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(name)
        .to_string()
}

/// Split a PCM name into its plugin prefix and, when it carries `CARD=`/`DEV=` arguments,
/// the (card id, device index) it addresses:
/// `plughw:CARD=CODEC,DEV=0` → `("plughw", Some(("CODEC", 0)))`, `pipewire` →
/// `("pipewire", None)`. A missing `DEV=` is device 0 (ALSA's own default), so
/// `default:CARD=x` keys the same slot as `plughw:CARD=x,DEV=0` and the two compete
/// rather than both being listed.
fn split_pcm_name(name: &str) -> (&str, Option<(&str, u32)>) {
    let Some((prefix, args)) = name.split_once(':') else {
        return (name, None);
    };
    let mut card = None;
    let mut dev = 0u32;
    for kv in args.split(',') {
        let Some((k, v)) = kv.split_once('=') else {
            continue;
        };
        match k.trim() {
            "CARD" => card = Some(v.trim()),
            "DEV" => dev = v.trim().parse().unwrap_or(0),
            _ => {}
        }
    }
    (prefix, card.map(|c| (c, dev)))
}

/// Card-bound prefixes never shown: each is the same hardware behind a plugin that
/// `plughw:` already covers, or is useless for rig audio. Probing exactly these is what
/// produced the operator's `dmix supports only playback` / `dsnoop supports only capture`
/// flood — here they are dropped without being opened.
fn denied_card_prefix(prefix: &str) -> bool {
    matches!(
        prefix,
        "dmix"
            | "dsnoop"
            | "front"
            | "rear"
            | "center_lfe"
            | "side"
            | "iec958"
            | "spdif"
            | "hdmi"
            | "usbstream"
            | "modem"
            | "phoneline"
            | "dsp"
    ) || prefix.starts_with("surround")
}

/// Rank among the survivors for one `(CARD, DEV)`; lower wins. Unknown prefixes rank LAST
/// but are kept, so a future ALSA plugin adds a redundant entry rather than losing a card.
fn card_prefix_rank(prefix: &str) -> u8 {
    match prefix {
        "plughw" => 0,
        "hw" => 1,
        "default" => 2,
        "sysdefault" => 3,
        _ => 4,
    }
}

/// Card-less names that are pure plumbing, never an operator's endpoint: the null sink,
/// the OSS shim (source of his `Cannot open device /dev/dsp`), the rate/channel
/// converters, and the bare argument-less plugin names (`hw` with no card selects
/// nothing useful here). Everything else card-less is KEPT — that is what preserves
/// `default`, `pulse`, `pipewire`, `jack` without naming them, and any custom PCM the
/// operator wrote into `~/.asoundrc`.
fn denied_bare_pcm(name: &str) -> bool {
    matches!(
        name,
        "null"
            | "oss"
            | "dsp"
            | "speex"
            | "speexrate"
            | "samplerate"
            | "lavrate"
            | "upmix"
            | "vdownmix"
            | "shm"
            | "tee"
            | "file"
            | "cards"
            | "empty"
            | "hw"
            | "plughw"
            | "plug"
            | "dmix"
            | "dsnoop"
            | "hdmi"
            | "front"
            | "rear"
            | "center_lfe"
            | "side"
            | "iec958"
            | "spdif"
            | "modem"
            | "phoneline"
            | "usbstream"
    ) || name.starts_with("surround")
}

/// Shorten a card-bound label from `"<card>, <pcm>"` to just `"<card>"` — so the rig reads
/// plainly as "USB AUDIO CODEC" instead of "USB AUDIO CODEC, USB Audio". Split at the LAST
/// ", " so a card whose own name contains a comma keeps it.
///
/// Held back only where the PCM part is DISCRIMINATING: a card contributing two survivors
/// with different full labels keeps "HD-Audio Generic, ALC1220 Analog" and
/// "HD-Audio Generic, ALC1220 Digital", because shortening both to "HD-Audio Generic"
/// would throw away the only thing telling them apart. Two survivors whose full labels are
/// IDENTICAL (two of the same rig codec, both "USB AUDIO CODEC, USB Audio") lose nothing by
/// shortening — [`disambiguate_labels`] gives them their " #N" either way.
fn shorten_card_labels(devices: &mut [AudioDevice]) {
    let heads: Vec<String> = devices
        .iter()
        .map(|d| {
            d.label
                .rsplit_once(", ")
                .map_or_else(|| d.label.clone(), |(head, _)| head.to_string())
        })
        .collect();
    // head -> the one full label seen under it, or None once a second, different one shows up.
    let mut only: HashMap<&str, Option<&str>> = HashMap::new();
    for (d, head) in devices.iter().zip(&heads) {
        only.entry(head.as_str())
            .and_modify(|seen| {
                if *seen != Some(d.label.as_str()) {
                    *seen = None;
                }
            })
            .or_insert(Some(d.label.as_str()));
    }
    let unambiguous: Vec<bool> = heads
        .iter()
        .map(|h| only.get(h.as_str()).copied().flatten().is_some())
        .collect();
    for ((d, head), ok) in devices.iter_mut().zip(&heads).zip(unambiguous) {
        if ok {
            d.label = head.clone();
        }
    }
}

/// Give duplicate LABELS a trailing " #N" — the same courtesy [`disambiguate_names`]
/// does on Windows/macOS, but applied to the display string only. The NAME is the
/// address and is never touched: ALSA PCM names are unique by construction, so two
/// entries can only collide on their description (two identical rig codecs both saying
/// "USB AUDIO CODEC").
fn disambiguate_labels(devices: &mut [AudioDevice]) {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for d in devices.iter_mut() {
        let c = counts.entry(d.label.clone()).or_insert(0);
        *c += 1;
        if *c > 1 {
            d.label = format!("{} #{c}", d.label);
        }
    }
}

/// Wrap cpal's own device names as [`AudioDevice`]s — the Windows/macOS path.
///
/// Disambiguates duplicates exactly as before (the " #N" suffix is load-bearing: it is
/// how [`split_device_ordinal`] tells two identically-named rig codecs apart), then uses
/// each resulting NAME as its own label. Those hosts already report the friendly string
/// the OS shows the operator, so every rendered entry is byte-identical to what shipped
/// before the Linux naming fix — which is what `cpal_names_are_their_own_labels` asserts.
pub fn devices_from_cpal_names(names: Vec<String>) -> Vec<AudioDevice> {
    disambiguate_names(names)
        .into_iter()
        .map(|n| AudioDevice {
            label: n.clone(),
            name: n,
        })
        .collect()
}

/// Disambiguate duplicate device names for a UI picker: the FIRST occurrence of a name is
/// kept bare (so existing single-device configs still resolve), and each later duplicate
/// gets a trailing " #N" (`#2`, `#3`, …). Two radios that both enumerate as the generic
/// "USB Audio CODEC" (a Yaesu + Icom pair is the common case) thus become distinct,
/// selectable entries instead of two identical strings that both resolve to the first
/// codec. Matched back by [`split_device_ordinal`].
///
/// Windows/macOS only in practice — ALSA PCM names are unique, so the Linux path
/// disambiguates LABELS instead ([`disambiguate_labels`]) and leaves names alone.
fn disambiguate_names(names: Vec<String>) -> Vec<String> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    names
        .into_iter()
        .map(|n| {
            let c = counts.entry(n.clone()).or_insert(0);
            *c += 1;
            if *c == 1 {
                n
            } else {
                format!("{n} #{c}")
            }
        })
        .collect()
}

/// Inverse of [`disambiguate_names`] for one name: split off a trailing " #N" (N ≥ 2) into
/// a base name + 1-based ordinal. A name with no such suffix is the 1st (bare) device.
///
/// `pub`, not `pub(crate)`: its only caller is `device::pick_device`, which is behind the
/// `device` feature, so the headless `cargo clippy --workspace` build sees it as dead code
/// and `-D warnings` fails the gate. Same reason [`prune_alsa_hints`] is public.
pub fn split_device_ordinal(name: &str) -> (&str, usize) {
    if let Some(pos) = name.rfind(" #") {
        if let Ok(n) = name[pos + 2..].parse::<usize>() {
            if n >= 2 {
                return (&name[..pos], n);
            }
        }
    }
    (name, 1)
}

#[cfg(test)]
mod tests {
    //! NON-VACUITY, measured (2026-08-05). Each rule below was broken and the suite re-run;
    //! baseline is 381 lib tests passing. Observed:
    //!
    //! | mutation | result |
    //! |---|---|
    //! | `hint_label` returns the PCM name (= the reported bug) | 4 red |
    //! | direction filter removed | 2 red |
    //! | `shorten_card_labels` no-op | 3 red |
    //! | `disambiguate_labels` no-op | 1 red |
    //! | `devices_from_cpal_names` drops the label | 1 red |
    //! | card-bound deny-list loses `dmix`/`dsnoop` | 1 red |
    //! | card-bound deny-list loses `hdmi` | 2 red |
    //! | card-less deny-list emptied | 2 red |
    //! | card-less filter becomes an allow-list (endpoints dropped) | 3 red |
    //! | rank prefers `hw:` over `plughw:` | 3 red |
    //!
    //! ⚠️ The `dmix`/`dsnoop` line was GREEN before
    //! `plugin_aliases_are_denied_even_when_nothing_better_shares_the_card` was written:
    //! rule 2 shadows the deny-list on any normal machine, so the 8-card fixture could not
    //! see it. That is what the extra fixture is for; do not delete it as redundant.
    use super::*;

    fn hint(name: &str, desc: &str, dir: Option<HintDir>) -> PcmHint {
        PcmHint {
            name: name.into(),
            desc: (!desc.is_empty()).then(|| desc.to_string()),
            direction: dir,
        }
    }

    /// A faithful `aplay -L` / `arecord -L` hint list for the reported machine: Kubuntu
    /// 26.04, 8 cards, PipeWire, FTDX10 on `card 7: CODEC [USB AUDIO CODEC]`. Hand-built
    /// rather than generated, because ALSA config files cannot declare colon-argument PCM
    /// names — only real cards produce those hints, and this box has no `/proc/asound`.
    // `rustfmt::skip`: these fixtures mirror `aplay -L` output, one hint per line —
    // reflowing each call across four lines makes 65 hints unreadable as a device list.
    #[rustfmt::skip]
    fn eight_card_box() -> Vec<PcmHint> {
        use HintDir::{Capture, Playback};
        vec![
            // ---- card-less software PCMs ----
            hint("null", "Discard all samples (playback) or generate zero samples (capture)", None),
            hint("lavrate", "Rate Converter Plugin Using Libav/FFmpeg Library", None),
            hint("samplerate", "Rate Converter Plugin Using Samplerate Library", None),
            hint("speexrate", "Rate Converter Plugin Using Speex Resampler", None),
            hint("jack", "JACK Audio Connection Kit", None),
            hint("oss", "Open Sound System", None),
            hint("pipewire", "PipeWire Sound Server", None),
            hint("pulse", "PulseAudio Sound Server", None),
            hint("speex", "Plugin using Speex DSP (resample, dsp)", None),
            hint("upmix", "Plugin for channel upmix (4,6,8)", None),
            hint("vdownmix", "Plugin for channel downmix (stereo) with a simple spacialization", None),
            hint("default", "Default ALSA Output (currently PipeWire Media Server)", None),
            hint("sysdefault", "Default ALSA Output (currently PipeWire Media Server)", None),
            // ---- card 0 HDMI [HDA ATI HDMI] — playback only ----
            hint("hdmi:CARD=HDMI,DEV=0", "HDA ATI HDMI, HDMI 0\nHDMI Audio Output", Some(Playback)),
            hint("hdmi:CARD=HDMI,DEV=1", "HDA ATI HDMI, HDMI 1\nHDMI Audio Output", Some(Playback)),
            hint("hdmi:CARD=HDMI,DEV=2", "HDA ATI HDMI, HDMI 2\nHDMI Audio Output", Some(Playback)),
            hint("hw:CARD=HDMI,DEV=3", "HDA ATI HDMI, HDMI 0\nDirect hardware device without any conversions", Some(Playback)),
            hint("plughw:CARD=HDMI,DEV=3", "HDA ATI HDMI, HDMI 0\nHardware device with all software conversions", Some(Playback)),
            hint("hw:CARD=HDMI,DEV=7", "HDA ATI HDMI, HDMI 1\nDirect hardware device without any conversions", Some(Playback)),
            hint("plughw:CARD=HDMI,DEV=7", "HDA ATI HDMI, HDMI 1\nHardware device with all software conversions", Some(Playback)),
            // ---- card 1 Generic [HD-Audio Generic] — two PCMs, analog + digital ----
            hint("default:CARD=Generic", "HD-Audio Generic, ALC1220 Analog\nDefault Audio Device", None),
            hint("sysdefault:CARD=Generic", "HD-Audio Generic, ALC1220 Analog\nDefault Audio Device", None),
            hint("front:CARD=Generic,DEV=0", "HD-Audio Generic, ALC1220 Analog\nFront output / input", None),
            hint("surround21:CARD=Generic,DEV=0", "HD-Audio Generic, ALC1220 Analog\n2.1 Surround output to Front and Subwoofer speakers", Some(Playback)),
            hint("surround40:CARD=Generic,DEV=0", "HD-Audio Generic, ALC1220 Analog\n4.0 Surround output to Front and Rear speakers", Some(Playback)),
            hint("surround51:CARD=Generic,DEV=0", "HD-Audio Generic, ALC1220 Analog\n5.1 Surround output to Front, Center, Rear and Subwoofer speakers", Some(Playback)),
            hint("iec958:CARD=Generic,DEV=1", "HD-Audio Generic, ALC1220 Digital\nIEC958 (S/PDIF) Digital Audio Output", Some(Playback)),
            hint("dmix:CARD=Generic,DEV=0", "HD-Audio Generic, ALC1220 Analog\nDirect sample mixing device", Some(Playback)),
            hint("dsnoop:CARD=Generic,DEV=0", "HD-Audio Generic, ALC1220 Analog\nDirect sample snooping device", Some(Capture)),
            hint("hw:CARD=Generic,DEV=0", "HD-Audio Generic, ALC1220 Analog\nDirect hardware device without any conversions", None),
            hint("plughw:CARD=Generic,DEV=0", "HD-Audio Generic, ALC1220 Analog\nHardware device with all software conversions", None),
            hint("hw:CARD=Generic,DEV=1", "HD-Audio Generic, ALC1220 Digital\nDirect hardware device without any conversions", Some(Playback)),
            hint("plughw:CARD=Generic,DEV=1", "HD-Audio Generic, ALC1220 Digital\nHardware device with all software conversions", Some(Playback)),
            // ---- card 2 NVidia ----
            hint("hdmi:CARD=NVidia,DEV=0", "HDA NVidia, HDMI 0\nHDMI Audio Output", Some(Playback)),
            hint("hw:CARD=NVidia,DEV=3", "HDA NVidia, HDMI 0\nDirect hardware device without any conversions", Some(Playback)),
            hint("plughw:CARD=NVidia,DEV=3", "HDA NVidia, HDMI 0\nHardware device with all software conversions", Some(Playback)),
            // ---- card 3 Device [USB Audio Device] — the ONE card Nexus showed before ----
            hint("default:CARD=Device", "USB Audio Device, USB Audio\nDefault Audio Device", None),
            hint("sysdefault:CARD=Device", "USB Audio Device, USB Audio\nDefault Audio Device", None),
            hint("front:CARD=Device,DEV=0", "USB Audio Device, USB Audio\nFront output / input", None),
            hint("surround40:CARD=Device,DEV=0", "USB Audio Device, USB Audio\n4.0 Surround output to Front and Rear speakers", Some(Playback)),
            hint("iec958:CARD=Device,DEV=0", "USB Audio Device, USB Audio\nIEC958 (S/PDIF) Digital Audio Output", Some(Playback)),
            hint("dmix:CARD=Device,DEV=0", "USB Audio Device, USB Audio\nDirect sample mixing device", Some(Playback)),
            hint("dsnoop:CARD=Device,DEV=0", "USB Audio Device, USB Audio\nDirect sample snooping device", Some(Capture)),
            hint("hw:CARD=Device,DEV=0", "USB Audio Device, USB Audio\nDirect hardware device without any conversions", None),
            hint("plughw:CARD=Device,DEV=0", "USB Audio Device, USB Audio\nHardware device with all software conversions", None),
            hint("usbstream:CARD=Device", "USB Audio Device\nUSB Stream Output", Some(Playback)),
            // ---- card 4 Webcam — CAPTURE ONLY ----
            hint("hw:CARD=Webcam,DEV=0", "HD Pro Webcam C920, USB Audio\nDirect hardware device without any conversions", Some(Capture)),
            hint("plughw:CARD=Webcam,DEV=0", "HD Pro Webcam C920, USB Audio\nHardware device with all software conversions", Some(Capture)),
            hint("dsnoop:CARD=Webcam,DEV=0", "HD Pro Webcam C920, USB Audio\nDirect sample snooping device", Some(Capture)),
            hint("usbstream:CARD=Webcam", "HD Pro Webcam C920\nUSB Stream Output", Some(Playback)),
            // ---- card 5 Headset ----
            hint("hw:CARD=Headset,DEV=0", "Logitech USB Headset, USB Audio\nDirect hardware device without any conversions", None),
            hint("plughw:CARD=Headset,DEV=0", "Logitech USB Headset, USB Audio\nHardware device with all software conversions", None),
            hint("dmix:CARD=Headset,DEV=0", "Logitech USB Headset, USB Audio\nDirect sample mixing device", Some(Playback)),
            hint("dsnoop:CARD=Headset,DEV=0", "Logitech USB Headset, USB Audio\nDirect sample snooping device", Some(Capture)),
            // ---- card 6 Dock ----
            hint("hw:CARD=Dock,DEV=0", "ThinkPad Dock USB Audio, USB Audio\nDirect hardware device without any conversions", None),
            hint("plughw:CARD=Dock,DEV=0", "ThinkPad Dock USB Audio, USB Audio\nHardware device with all software conversions", None),
            hint("dsnoop:CARD=Dock,DEV=0", "ThinkPad Dock USB Audio, USB Audio\nDirect sample snooping device", Some(Capture)),
            // ---- card 7 CODEC [USB AUDIO CODEC] — the FTDX10, the whole point ----
            hint("default:CARD=CODEC", "USB AUDIO CODEC, USB Audio\nDefault Audio Device", None),
            hint("sysdefault:CARD=CODEC", "USB AUDIO CODEC, USB Audio\nDefault Audio Device", None),
            hint("front:CARD=CODEC,DEV=0", "USB AUDIO CODEC, USB Audio\nFront output / input", None),
            hint("dmix:CARD=CODEC,DEV=0", "USB AUDIO CODEC, USB Audio\nDirect sample mixing device", Some(Playback)),
            hint("dsnoop:CARD=CODEC,DEV=0", "USB AUDIO CODEC, USB Audio\nDirect sample snooping device", Some(Capture)),
            hint("hw:CARD=CODEC,DEV=0", "USB AUDIO CODEC, USB Audio\nDirect hardware device without any conversions", None),
            hint("plughw:CARD=CODEC,DEV=0", "USB AUDIO CODEC, USB Audio\nHardware device with all software conversions", None),
            hint("usbstream:CARD=CODEC", "USB AUDIO CODEC\nUSB Stream Output", Some(Playback)),
        ]
    }

    fn names(list: &[AudioDevice]) -> Vec<&str> {
        list.iter().map(|d| d.name.as_str()).collect()
    }
    fn labels(list: &[AudioDevice]) -> Vec<&str> {
        list.iter().map(|d| d.label.as_str()).collect()
    }

    /// THE field report, closed: the FTDX10's codec is present in both lists and reads
    /// exactly as `aplay -l`, KDE, pavucontrol and WSJT-X read it.
    #[test]
    fn the_rig_codec_is_listed_under_the_name_the_operator_sees() {
        let (input, output) = prune_alsa_hints(&eight_card_box());
        let want = AudioDevice {
            name: "plughw:CARD=CODEC,DEV=0".into(),
            label: "USB AUDIO CODEC".into(),
        };
        assert!(input.contains(&want), "input list: {:?}", labels(&input));
        assert!(output.contains(&want), "output list: {:?}", labels(&output));
    }

    /// The whole before/after, pinned. 65 raw hints → 10 input / 13 output, one entry per
    /// usable card plus the routing endpoints, hardware first. Every error family in the
    /// operator's log is accounted for here: the `dmix:`/`dsnoop:` hints that produced
    /// "supports only playback/capture" are dropped as data, and `oss` (his
    /// "Cannot open device /dev/dsp") with them — none of them is ever opened.
    #[test]
    fn eight_card_box_prunes_to_the_operators_real_devices() {
        let hints = eight_card_box();
        assert_eq!(hints.len(), 65, "fixture drifted");
        let (input, output) = prune_alsa_hints(&hints);
        assert_eq!(
            names(&input),
            vec![
                "plughw:CARD=Generic,DEV=0",
                "plughw:CARD=Device,DEV=0",
                "plughw:CARD=Webcam,DEV=0",
                "plughw:CARD=Headset,DEV=0",
                "plughw:CARD=Dock,DEV=0",
                "plughw:CARD=CODEC,DEV=0",
                "jack",
                "pipewire",
                "pulse",
                "default",
            ]
        );
        assert_eq!(
            names(&output),
            vec![
                "plughw:CARD=HDMI,DEV=3",
                "plughw:CARD=HDMI,DEV=7",
                "plughw:CARD=Generic,DEV=0",
                "plughw:CARD=Generic,DEV=1",
                "plughw:CARD=NVidia,DEV=3",
                "plughw:CARD=Device,DEV=0",
                "plughw:CARD=Headset,DEV=0",
                "plughw:CARD=Dock,DEV=0",
                "plughw:CARD=CODEC,DEV=0",
                "jack",
                "pipewire",
                "pulse",
                "default",
            ]
        );
    }

    /// The plugin-alias deny-list, measured ON ITS OWN.
    ///
    /// Rule 2 already shadows most of it: a `dmix:` hint loses to its own card's `plughw:`
    /// on rank, so deleting "dmix" from the deny-list changes nothing on a normal machine
    /// (verified — that mutation left the 8-card fixture green, which is why this test
    /// exists). So give the card NO `plughw:`/`hw:` sibling: without the deny-list these
    /// aliases become the survivor and the operator is offered `dmix:CARD=CODEC,DEV=0` —
    /// one of the very names his log was full of. `hdmi:` is the case rule 2 can never
    /// shadow, because it renumbers DEV (HDMI 0/1/2 against hw's 3/7).
    #[test]
    // `rustfmt::skip`: these fixtures mirror `aplay -L` output, one hint per line —
    // reflowing each call across four lines makes 65 hints unreadable as a device list.
    #[rustfmt::skip]
    fn plugin_aliases_are_denied_even_when_nothing_better_shares_the_card() {
        use HintDir::{Capture, Playback};
        let hints = vec![
            hint("dmix:CARD=CODEC,DEV=0", "USB AUDIO CODEC, USB Audio\nDirect sample mixing device", Some(Playback)),
            hint("dsnoop:CARD=CODEC,DEV=0", "USB AUDIO CODEC, USB Audio\nDirect sample snooping device", Some(Capture)),
            hint("front:CARD=CODEC,DEV=0", "USB AUDIO CODEC, USB Audio\nFront output / input", None),
            hint("surround40:CARD=CODEC,DEV=0", "USB AUDIO CODEC, USB Audio\n4.0 Surround output", Some(Playback)),
            hint("iec958:CARD=CODEC,DEV=0", "USB AUDIO CODEC, USB Audio\nIEC958 Digital Audio Output", Some(Playback)),
            hint("usbstream:CARD=CODEC", "USB AUDIO CODEC\nUSB Stream Output", Some(Playback)),
            hint("hdmi:CARD=HDMI,DEV=0", "HDA ATI HDMI, HDMI 0\nHDMI Audio Output", Some(Playback)),
        ];
        let (input, output) = prune_alsa_hints(&hints);
        assert!(input.is_empty(), "input: {:?}", names(&input));
        assert!(output.is_empty(), "output: {:?}", names(&output));
    }

    /// The card-less deny-list, likewise on its own — nothing here shares a key with
    /// anything, so rule 2 cannot shadow it. `oss` is the hint behind his repeated
    /// "Cannot open device /dev/dsp"; the rest is rate/channel plumbing. The routing
    /// endpoints in the same list must survive: on a PipeWire desktop they are often the
    /// right answer, and an allow-list would have deleted them along with the noise.
    #[test]
    // `rustfmt::skip`: these fixtures mirror `aplay -L` output, one hint per line —
    // reflowing each call across four lines makes 65 hints unreadable as a device list.
    #[rustfmt::skip]
    fn card_less_plumbing_is_denied_and_the_routing_endpoints_are_not() {
        let hints: Vec<PcmHint> = [
            "null", "oss", "lavrate", "samplerate", "speexrate", "speex", "upmix", "vdownmix",
            "hw", "plughw", "plug", "dmix", "dsnoop", "surround51", "iec958", "usbstream",
            "jack", "pipewire", "pulse", "default",
        ]
        .iter()
        .map(|n| hint(n, &format!("{n} description"), None))
        .collect();
        let (input, _) = prune_alsa_hints(&hints);
        assert_eq!(names(&input), vec!["jack", "pipewire", "pulse", "default"]);
    }

    /// `IOID` splits the lists, so a capture-only card is absent from the output list and
    /// a playback-only card from the input list — WITHOUT opening anything.
    #[test]
    fn direction_comes_from_ioid_not_from_a_probe_open() {
        let (input, output) = prune_alsa_hints(&eight_card_box());
        // Webcam is capture-only; HDMI and NVidia are playback-only.
        assert!(names(&input).contains(&"plughw:CARD=Webcam,DEV=0"));
        assert!(!names(&output).contains(&"plughw:CARD=Webcam,DEV=0"));
        assert!(names(&output).contains(&"plughw:CARD=NVidia,DEV=3"));
        assert!(!names(&input).contains(&"plughw:CARD=NVidia,DEV=3"));
        // A hint with NO IOID is usable both ways and appears in both.
        assert!(names(&input).contains(&"plughw:CARD=CODEC,DEV=0"));
        assert!(names(&output).contains(&"plughw:CARD=CODEC,DEV=0"));
    }

    /// A one-PCM card reads as just the card ("USB AUDIO CODEC"); a card with two PCMs
    /// keeps the PCM part so its entries stay distinguishable.
    #[test]
    fn labels_shorten_only_when_the_card_is_unambiguous() {
        let (_, output) = prune_alsa_hints(&eight_card_box());
        let l = labels(&output);
        assert!(l.contains(&"USB AUDIO CODEC"), "{l:?}");
        assert!(l.contains(&"HD-Audio Generic, ALC1220 Analog"), "{l:?}");
        assert!(l.contains(&"HD-Audio Generic, ALC1220 Digital"), "{l:?}");
        // NVidia has one surviving PCM, so it shortens.
        assert!(l.contains(&"HDA NVidia"), "{l:?}");
    }

    /// Two identical rig codecs (a Yaesu + an Icom, both "USB AUDIO CODEC") get distinct
    /// LABELS — and identical NAMES would be a bug, so the names must stay untouched.
    #[test]
    fn duplicate_labels_get_an_ordinal_and_names_are_never_rewritten() {
        let hints = vec![
            PcmHint {
                name: "plughw:CARD=CODEC,DEV=0".into(),
                desc: Some("USB AUDIO CODEC, USB Audio\nHardware device".into()),
                direction: None,
            },
            PcmHint {
                name: "plughw:CARD=CODEC_1,DEV=0".into(),
                desc: Some("USB AUDIO CODEC, USB Audio\nHardware device".into()),
                direction: None,
            },
        ];
        let (input, _) = prune_alsa_hints(&hints);
        assert_eq!(
            labels(&input),
            vec!["USB AUDIO CODEC", "USB AUDIO CODEC #2"]
        );
        assert_eq!(
            names(&input),
            vec!["plughw:CARD=CODEC,DEV=0", "plughw:CARD=CODEC_1,DEV=0"]
        );
    }

    /// A hand-written `~/.asoundrc` PCM survives (deny-list, not allow-list), and a hint
    /// with no description falls back to its own name rather than inventing one.
    #[test]
    fn custom_pcms_survive_and_missing_descriptions_fall_back_to_the_name() {
        let hints = vec![
            PcmHint {
                name: "ft8rig".into(),
                desc: Some("My FTDX10 rig audio".into()),
                direction: None,
            },
            PcmHint {
                name: "undocumented".into(),
                desc: None,
                direction: None,
            },
        ];
        let (input, _) = prune_alsa_hints(&hints);
        assert_eq!(names(&input), vec!["ft8rig", "undocumented"]);
        assert_eq!(labels(&input), vec!["My FTDX10 rig audio", "undocumented"]);
    }

    /// WINDOWS / macOS REGRESSION GUARD. The DTO widening is the one part of this change
    /// that is not Linux-only, so the strings those hosts render must be provably
    /// unchanged: cpal's names pass through byte-identical (including the " #N" ordinal
    /// suffix, which is the ADDRESS there), and each is its own label — so every
    /// `<option>` renders exactly what it rendered before.
    #[test]
    fn cpal_names_are_their_own_labels() {
        let got = devices_from_cpal_names(vec![
            "Microphone (USB Audio CODEC)".into(),
            "Speakers (Realtek(R) Audio)".into(),
            "Microphone (USB Audio CODEC)".into(),
        ]);
        assert_eq!(
            names(&got),
            vec![
                "Microphone (USB Audio CODEC)",
                "Speakers (Realtek(R) Audio)",
                "Microphone (USB Audio CODEC) #2",
            ]
        );
        for d in &got {
            assert_eq!(d.name, d.label, "a non-Linux host must render its own name");
        }
    }

    #[test]
    fn split_device_ordinal_is_the_inverse_of_disambiguate() {
        assert_eq!(
            split_device_ordinal("USB Audio CODEC"),
            ("USB Audio CODEC", 1)
        );
        assert_eq!(
            split_device_ordinal("USB Audio CODEC #2"),
            ("USB Audio CODEC", 2)
        );
        assert_eq!(
            split_device_ordinal("USB Audio CODEC #3"),
            ("USB Audio CODEC", 3)
        );
        // Only a synthetic " #N" with N >= 2 is an ordinal; a real name that happens to
        // contain "#1" or a non-numeric "#" is left intact as the 1st device. An ALSA PCM
        // name never carries one, so the Linux path is unaffected either way.
        assert_eq!(split_device_ordinal("Rig #1"), ("Rig #1", 1));
        assert_eq!(split_device_ordinal("Mic #A"), ("Mic #A", 1));
        assert_eq!(
            split_device_ordinal("plughw:CARD=CODEC,DEV=0"),
            ("plughw:CARD=CODEC,DEV=0", 1)
        );
    }
}
