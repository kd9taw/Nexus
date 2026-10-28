//! Settings ▸ Digital ▸ JS8 ▸ Sounds: JS8's notification sounds, each its built-in tone or a WAV
//! file the operator picked (as JS8Call lets each of its notifications play one: the seven rows,
//! Configuration.cpp:1996-2002; the "Select" button's "Wave Files (*.wav)" dialog, :2054-2066), on
//! the output the operator picked (JS8Call's notification audio device, UI_Constructor.cpp:228-233).
//!
//! Two commands, each run on the computer the operator sits at:
//! - `pick_js8_sound_file`: the OS file picker. It reads a path back and remembers it for this run
//!   so Test can play it before Save; the Settings form holds it and Save writes it.
//! - `play_js8_sound`: plays one event's sound HERE, in Rust. The window never gets a file's bytes.
//!
//! WHAT IS READ. Only a file the operator chose: the one Settings holds for that event, or one the
//! picker returned in this run. Any other path the window names is refused unread
//! ([`SoundRefusal::NotPicked`]). What is read is bounded and checked by `tempo_codec::sound_file`
//! (a regular file, at most 10 MiB, RIFF/WAVE, a plausible format); a file that fails plays the
//! built-in tone instead, and the window is told why so it can say so once.
//!
//! WHERE IT PLAYS. `js8_sound_device` (empty: the system default output), and never a device the
//! station transmits from: `tempo_audio::notify_sound` checks the device that opens against every
//! configured TX audio device at each play, with the settings as they are then. A refused device
//! plays NOTHING, not even the tone: falling back to another output could land on the very device
//! the check refused.
//!
//! Nothing here writes, and nothing here transmits.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Mutex;
use tauri::{AppHandle, State};
use tempo_app::settings::Settings;
use tempo_codec::sound_file::{read_sound_file, SoundClip, SoundFileRefusal};

use crate::SharedEngine;

/// JS8Call's seven notification events, by the names Settings `js8Sounds` holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Js8SoundEvent {
    Cq,
    Hb,
    Ack,
    Directed,
    Inbox,
    CallNew,
    CallOld,
}

impl Js8SoundEvent {
    fn key(self) -> &'static str {
        match self {
            Self::Cq => "cq",
            Self::Hb => "hb",
            Self::Ack => "ack",
            Self::Directed => "directed",
            Self::Inbox => "inbox",
            Self::CallNew => "callNew",
            Self::CallOld => "callOld",
        }
    }

    /// The built-in tone (Hz, and whether it rings twice): each apart from the window's other
    /// alerts, and a message for me rings twice.
    fn tone(self) -> (f32, bool) {
        match self {
            Self::Cq => (587.0, false),
            Self::Hb => (466.0, false),
            Self::Ack => (1046.0, false),
            Self::Directed => (932.0, true),
            Self::Inbox => (784.0, true),
            Self::CallNew => (698.0, false),
            Self::CallOld => (554.0, false),
        }
    }
}

/// Why a sound, or its file, did not play. The window names each in the operator's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SoundRefusal {
    // A file that did not play; the tone played instead.
    Missing,
    NotAFile,
    Unreadable,
    TooLarge,
    NotWav,
    BadFormat,
    /// A path the operator did not choose (not in Settings for that event, not picked this run).
    NotPicked,
    // Nothing played.
    /// The output is a device the station transmits from (VOX safety).
    TxDevice,
    /// No such output here.
    NoDevice,
    /// The output would not open or play.
    Failed,
}

impl From<SoundFileRefusal> for SoundRefusal {
    fn from(r: SoundFileRefusal) -> Self {
        match r {
            SoundFileRefusal::Missing => Self::Missing,
            SoundFileRefusal::NotAFile => Self::NotAFile,
            SoundFileRefusal::Unreadable => Self::Unreadable,
            SoundFileRefusal::TooLarge => Self::TooLarge,
            SoundFileRefusal::NotWav => Self::NotWav,
            SoundFileRefusal::BadFormat => Self::BadFormat,
        }
    }
}

/// What played: the tone in place of the file, and why, when a file was asked for and did not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundPlayed {
    pub file_refused: Option<SoundRefusal>,
}

/// The paths the picker returned this run: Test plays one before Save writes it.
static PICKED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// Whether `path` is one the operator chose for `event`: the file Settings holds for it, or one
/// the picker returned this run.
fn chosen(
    settings: &Settings,
    event: Js8SoundEvent,
    path: &str,
    picked: &BTreeSet<String>,
) -> bool {
    settings
        .js8_sound_files
        .get(event.key())
        .map(String::as_str)
        == Some(path)
        || picked.contains(path)
}

/// The sound to play for `event`: the chosen file's samples, else the tone with why the file did
/// not play. Reads nothing unless the path is one the operator chose.
fn clip_for(
    settings: &Settings,
    event: Js8SoundEvent,
    file: Option<&str>,
    picked: &BTreeSet<String>,
) -> (SoundClip, Option<SoundRefusal>) {
    let tone = || {
        let (hz, double) = event.tone();
        SoundClip {
            rate: tempo_audio_rate(),
            samples: tone_samples(hz, double),
        }
    };
    let Some(path) = file.map(str::trim).filter(|p| !p.is_empty()) else {
        return (tone(), None);
    };
    if !chosen(settings, event, path, picked) {
        return (tone(), Some(SoundRefusal::NotPicked));
    }
    match read_sound_file(Path::new(path)) {
        Ok(clip) => (clip, None),
        Err(why) => (tone(), Some(why.into())),
    }
}

#[cfg(feature = "radio")]
fn tempo_audio_rate() -> u32 {
    tempo_audio::notify_sound::TONE_RATE
}
#[cfg(feature = "radio")]
fn tone_samples(hz: f32, double: bool) -> Vec<f32> {
    tempo_audio::notify_sound::tone(hz, double)
}
#[cfg(not(feature = "radio"))]
fn tempo_audio_rate() -> u32 {
    48_000
}
#[cfg(not(feature = "radio"))]
fn tone_samples(_hz: f32, _double: bool) -> Vec<f32> {
    Vec::new()
}

/// The OS file picker for one sound's WAV file. `None` when it is cancelled: never an error, and
/// never a change. The callback form, for the reason `pick_data_folder` gives.
#[tauri::command]
pub async fn pick_js8_sound_file(app: AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter("WAV", &["wav", "WAV"])
        .pick_file(move |picked| {
            let _ = tx.send(picked);
        });
    let path = rx
        .await
        .ok()
        .flatten()
        .and_then(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned())?;
    PICKED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(path.clone());
    Some(path)
}

/// Play `event`'s sound here: `file` (the form's, so Test plays an unsaved pick) or its tone, on
/// Settings' `js8SoundDevice`, unless that is a TX device. `Ok` says whether a file fell back to
/// the tone; `Err` is that nothing played.
#[tauri::command]
pub async fn play_js8_sound(
    state: State<'_, SharedEngine>,
    event: Js8SoundEvent,
    file: Option<String>,
) -> Result<SoundPlayed, SoundRefusal> {
    let settings = crate::with_engine(&state, |eng| eng.settings().clone())
        .await
        .map_err(|_| SoundRefusal::Failed)?;
    tauri::async_runtime::spawn_blocking(move || {
        let picked = PICKED.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let (clip, file_refused) = clip_for(&settings, event, file.as_deref(), &picked);
        play_on(&settings, clip)?;
        Ok(SoundPlayed { file_refused })
    })
    .await
    .map_err(|_| SoundRefusal::Failed)?
}

/// Where a sound goes and what it must never reach, from the settings as they are at this play: the
/// operator's output (`js8SoundDevice`), and every TX audio device the station has configured.
#[cfg(feature = "radio")]
fn sound_route(settings: &Settings) -> (&str, Vec<String>) {
    (
        &settings.js8_sound_device,
        tempo_audio::notify_sound::tx_audio_devices(settings),
    )
}

/// Play `clip` on the operator's output, refused on a TX device (`tempo_audio::notify_sound`).
#[cfg(feature = "radio")]
fn play_on(settings: &Settings, clip: SoundClip) -> Result<(), SoundRefusal> {
    use tempo_audio::notify_sound::{play, PlayRefusal};
    let (device, tx_devices) = sound_route(settings);
    play(device, tx_devices, clip.rate, clip.samples).map_err(|e| match e {
        PlayRefusal::TxDevice => SoundRefusal::TxDevice,
        PlayRefusal::NoDevice => SoundRefusal::NoDevice,
        PlayRefusal::Failed(why) => {
            eprintln!("nexus: a JS8 sound did not play: {why}");
            SoundRefusal::Failed
        }
    })
}

/// A build without the radio has no sound devices.
#[cfg(not(feature = "radio"))]
fn play_on(_settings: &Settings, _clip: SoundClip) -> Result<(), SoundRefusal> {
    Err(SoundRefusal::NoDevice)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nexus-js8-sounds-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A 44-byte PCM WAV header and two samples.
    fn tiny_wav() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&40u32.to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes()); // PCM
        b.extend_from_slice(&1u16.to_le_bytes()); // mono
        b.extend_from_slice(&12_000u32.to_le_bytes());
        b.extend_from_slice(&24_000u32.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&4u32.to_le_bytes());
        b.extend_from_slice(&[0, 0, 0, 0x40]);
        b
    }

    /// THE WINDOW CANNOT READ WHAT THE OPERATOR DID NOT CHOOSE: a path that is neither the event's
    /// saved file nor one picked this run is refused unread, a real WAV included.
    #[test]
    fn only_a_file_the_operator_chose_is_read() {
        let dir = scratch("chosen");
        let cq = dir.join("cq.wav");
        std::fs::write(&cq, tiny_wav()).unwrap();
        let cq = cq.to_string_lossy().into_owned();
        let mut settings = Settings::default();
        let none = BTreeSet::new();

        let (_, why) = clip_for(&settings, Js8SoundEvent::Cq, Some(&cq), &none);
        assert_eq!(why, Some(SoundRefusal::NotPicked), "never chosen");

        settings.js8_sound_files.insert("cq".into(), cq.clone());
        let (clip, why) = clip_for(&settings, Js8SoundEvent::Cq, Some(&cq), &none);
        assert_eq!((clip.rate, why), (12_000, None), "the event's saved file");
        let (_, why) = clip_for(&settings, Js8SoundEvent::Hb, Some(&cq), &none);
        assert_eq!(
            why,
            Some(SoundRefusal::NotPicked),
            "another event's file is not this one's"
        );

        let picked: BTreeSet<String> = [cq.clone()].into();
        let (clip, why) = clip_for(&Settings::default(), Js8SoundEvent::Ack, Some(&cq), &picked);
        assert_eq!(
            (clip.rate, why),
            (12_000, None),
            "picked this run, before Save"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_chosen_file_that_cannot_play_gives_the_tone_and_says_why() {
        let dir = scratch("fallback");
        let gone = dir.join("gone.wav").to_string_lossy().into_owned();
        let text = dir.join("notes.wav");
        std::fs::write(&text, b"just some words").unwrap();
        let text = text.to_string_lossy().into_owned();
        let folder = dir.to_string_lossy().into_owned();
        let picked: BTreeSet<String> = [gone.clone(), text.clone(), folder.clone()].into();
        let s = Settings::default();
        for (path, why) in [
            (&gone, SoundRefusal::Missing),
            (&text, SoundRefusal::NotWav),
            (&folder, SoundRefusal::NotAFile),
        ] {
            let (clip, got) = clip_for(&s, Js8SoundEvent::Inbox, Some(path), &picked);
            assert_eq!(got, Some(why), "{path}");
            assert_eq!(clip.rate, tempo_audio_rate(), "the tone instead");
        }
        let (_, got) = clip_for(&s, Js8SoundEvent::Inbox, None, &picked);
        assert_eq!(got, None, "no file: the tone, nothing to say");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The window reads each refusal by name (`ui/src/features/js8Sounds.ts`).
    #[test]
    fn a_refusal_reaches_the_window_by_name() {
        let names: Vec<String> = [
            SoundRefusal::Missing,
            SoundRefusal::NotAFile,
            SoundRefusal::Unreadable,
            SoundRefusal::TooLarge,
            SoundRefusal::NotWav,
            SoundRefusal::BadFormat,
            SoundRefusal::NotPicked,
            SoundRefusal::TxDevice,
            SoundRefusal::NoDevice,
            SoundRefusal::Failed,
        ]
        .iter()
        .map(|r| serde_json::to_string(r).unwrap())
        .collect();
        assert_eq!(
            names,
            [
                "\"missing\"",
                "\"notAFile\"",
                "\"unreadable\"",
                "\"tooLarge\"",
                "\"notWav\"",
                "\"badFormat\"",
                "\"notPicked\"",
                "\"txDevice\"",
                "\"noDevice\"",
                "\"failed\""
            ]
        );
        let ev: Js8SoundEvent = serde_json::from_str("\"callNew\"").unwrap();
        assert_eq!(ev, Js8SoundEvent::CallNew);
        assert!(serde_json::from_str::<Js8SoundEvent>("\"../etc\"").is_err());
    }

    /// VOX SAFETY, end to end on this side: the route a play takes from Settings hands the guard
    /// every TX audio device, so the operator's output set to one of them (by any spelling, the
    /// station's or a radio profile's, chosen before or after the TX device) refuses the sound.
    #[cfg(feature = "radio")]
    #[test]
    fn a_sound_routed_to_a_tx_audio_device_is_refused() {
        use tempo_audio::notify_sound::{refuse_tx, PlayRefusal};
        let mut settings = Settings {
            audio_out: "USB Audio CODEC".into(),
            js8_sound_device: "Speakers".into(),
            ..Default::default()
        };
        let verdict = |s: &Settings| {
            let (device, tx) = sound_route(s);
            refuse_tx(device, device, &tx, None)
        };
        assert_eq!(verdict(&settings), Ok(()), "control: another output plays");
        settings.js8_sound_device = "usb audio codec".into();
        assert_eq!(
            verdict(&settings),
            Err(PlayRefusal::TxDevice),
            "the station's TX device"
        );
        settings.js8_sound_device = "Rig 2 CODEC".into();
        settings.radios.push(tempo_app::settings::RadioProfile {
            audio_out: "Rig 2 CODEC".into(),
            ..Default::default()
        });
        assert_eq!(
            verdict(&settings),
            Err(PlayRefusal::TxDevice),
            "a radio profile's TX device"
        );
        settings.radios.clear();
        assert_eq!(verdict(&settings), Ok(()), "control: that radio removed");
        settings.audio_out = "Rig 2 CODEC".into();
        assert_eq!(
            verdict(&settings),
            Err(PlayRefusal::TxDevice),
            "the TX device changed to it after the output was picked"
        );
    }

    /// Both commands are registered, once each, and the old byte-handing command is gone: a
    /// command missing from the handler list fails in the window as "command not found".
    #[test]
    fn both_sound_commands_are_registered() {
        let src = include_str!("lib.rs");
        let handlers = src
            .split_once("tauri::generate_handler![")
            .and_then(|(_, rest)| rest.split_once("])"))
            .map(|(list, _)| list)
            .expect("lib.rs has a generate_handler! list");
        for command in ["pick_js8_sound_file", "play_js8_sound"] {
            let named = format!("js8_sounds::{command},");
            assert_eq!(handlers.matches(&named).count(), 1, "{named}");
        }
        assert!(!src.contains("read_js8_sound_file"));
    }
}
