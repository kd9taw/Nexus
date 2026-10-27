//! Settings ▸ Digital ▸ JS8 ▸ Sounds: the WAV file a JS8 sound plays, as JS8Call lets each of its
//! notifications play one (the seven rows, Configuration.cpp:1996-2002; the "Select" button's file
//! dialog, "Wave Files (*.wav)", :2054-2066).
//!
//! Two commands, each run on the computer the operator sits at:
//! - `pick_js8_sound_file`: the OS file picker. It only reads a path back; the Settings form holds
//!   it and Save writes it with every other field.
//! - `read_js8_sound_file`: a file's bytes, raw, for the window to play through the same audio as
//!   the built-in tones. Only a RIFF/WAVE file up to [`MAX_SOUND_FILE_BYTES`] is handed over, so a
//!   page that names any other file gets nothing from it but a refusal; the window then plays the
//!   built-in tone and says so once.
//!
//! Nothing here writes, and nothing here transmits.
use serde::Serialize;
use std::path::Path;
use tauri::ipc::Response;
use tauri::AppHandle;

/// The largest sound file handed over: a notification sound is a second or two, and 10 MiB is a
/// minute of CD-quality stereo.
pub const MAX_SOUND_FILE_BYTES: u64 = 10 * 1024 * 1024;

/// Why a sound file was not handed over. The window names each in the operator's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SoundFileRefusal {
    /// Nothing at that path.
    Missing,
    /// There, but not a file this computer lets Nexus read.
    Unreadable,
    /// Larger than [`MAX_SOUND_FILE_BYTES`].
    TooLarge,
    /// Not a RIFF/WAVE file.
    NotWav,
}

/// A WAV file's bytes, or why not. The size is checked before anything is read, and the header
/// after: "RIFF", a length, "WAVE" (what JS8Call's `BWFFile` opens, NotificationAudio.cpp:72-74).
pub fn wav_bytes(path: &Path) -> Result<Vec<u8>, SoundFileRefusal> {
    let refused = |e: std::io::Error| match e.kind() {
        std::io::ErrorKind::NotFound => SoundFileRefusal::Missing,
        _ => SoundFileRefusal::Unreadable,
    };
    let meta = std::fs::metadata(path).map_err(refused)?;
    if !meta.is_file() {
        return Err(SoundFileRefusal::Unreadable);
    }
    if meta.len() > MAX_SOUND_FILE_BYTES {
        return Err(SoundFileRefusal::TooLarge);
    }
    let bytes = std::fs::read(path).map_err(refused)?;
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(SoundFileRefusal::NotWav);
    }
    Ok(bytes)
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
    rx.await
        .ok()
        .flatten()
        .and_then(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
}

/// One sound file's bytes, raw (an ArrayBuffer in the window), or why it was refused.
#[tauri::command(async)]
pub fn read_js8_sound_file(path: String) -> Result<Response, SoundFileRefusal> {
    wav_bytes(Path::new(&path)).map(Response::new)
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

    /// A 44-byte PCM WAV header and two silent samples: the smallest file a player opens.
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
        b.extend_from_slice(&[0, 0, 0, 0]);
        b
    }

    #[test]
    fn a_wav_file_is_handed_over_byte_for_byte() {
        let dir = scratch("wav");
        let path = dir.join("cq.wav");
        std::fs::write(&path, tiny_wav()).unwrap();
        assert_eq!(wav_bytes(&path), Ok(tiny_wav()));
        // The extension is not what makes it a WAV: the header is.
        let renamed = dir.join("cq.sound");
        std::fs::write(&renamed, tiny_wav()).unwrap();
        assert_eq!(wav_bytes(&renamed), Ok(tiny_wav()));
    }

    #[test]
    fn anything_but_a_readable_wav_up_to_the_cap_is_refused_with_its_reason() {
        let dir = scratch("refused");
        assert_eq!(
            wav_bytes(&dir.join("gone.wav")),
            Err(SoundFileRefusal::Missing)
        );
        let text = dir.join("notes.wav");
        std::fs::write(&text, b"not a sound at all, just some words").unwrap();
        assert_eq!(wav_bytes(&text), Err(SoundFileRefusal::NotWav));
        // RIFF but not WAVE (an AVI, say).
        let avi = dir.join("clip.wav");
        let mut riff = tiny_wav();
        riff[8..12].copy_from_slice(b"AVI ");
        std::fs::write(&avi, riff).unwrap();
        assert_eq!(wav_bytes(&avi), Err(SoundFileRefusal::NotWav));
        let short = dir.join("short.wav");
        std::fs::write(&short, b"RIFF").unwrap();
        assert_eq!(wav_bytes(&short), Err(SoundFileRefusal::NotWav));
        assert_eq!(
            wav_bytes(&dir),
            Err(SoundFileRefusal::Unreadable),
            "a folder"
        );
        // One byte over the cap, a WAV header and all: refused before it is read (sparse, so the
        // test writes 44 bytes, not 10 MiB).
        let big = dir.join("big.wav");
        std::fs::write(&big, tiny_wav()).unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&big)
            .unwrap()
            .set_len(MAX_SOUND_FILE_BYTES + 1)
            .unwrap();
        assert_eq!(wav_bytes(&big), Err(SoundFileRefusal::TooLarge));
        // Exactly the cap is handed over.
        std::fs::OpenOptions::new()
            .write(true)
            .open(&big)
            .unwrap()
            .set_len(MAX_SOUND_FILE_BYTES)
            .unwrap();
        assert_eq!(
            wav_bytes(&big).map(|b| b.len() as u64),
            Ok(MAX_SOUND_FILE_BYTES)
        );
    }

    /// The window reads each refusal by name (`ui/src/features/js8Sounds.ts`).
    #[test]
    fn a_refusal_reaches_the_window_by_name() {
        let names: Vec<String> = [
            SoundFileRefusal::Missing,
            SoundFileRefusal::Unreadable,
            SoundFileRefusal::TooLarge,
            SoundFileRefusal::NotWav,
        ]
        .iter()
        .map(|r| serde_json::to_string(r).unwrap())
        .collect();
        assert_eq!(
            names,
            [
                "\"missing\"",
                "\"unreadable\"",
                "\"tooLarge\"",
                "\"notWav\""
            ]
        );
    }

    /// Both commands are registered, once each: a command missing from the handler list fails in
    /// the window as "command not found" with nothing in any log.
    #[test]
    fn both_sound_commands_are_registered() {
        let src = include_str!("lib.rs");
        let handlers = src
            .split_once("tauri::generate_handler![")
            .and_then(|(_, rest)| rest.split_once("])"))
            .map(|(list, _)| list)
            .expect("lib.rs has a generate_handler! list");
        for command in ["pick_js8_sound_file", "read_js8_sound_file"] {
            let named = format!("js8_sounds::{command},");
            assert_eq!(handlers.matches(&named).count(), 1, "{named}");
        }
    }
}
