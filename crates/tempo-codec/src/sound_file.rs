//! A notification sound's WAV file (JS8's sounds: Settings ▸ Digital ▸ JS8 ▸ Sounds), read and
//! checked before a sample of it is played. Pure: no device, no engine.
//!
//! What is read is bounded at every step, so a path pointing anywhere costs a refusal at most:
//! - only a regular file (never a folder, a device, a pipe or a socket: a pipe would block the read);
//! - at most [`MAX_SOUND_FILE_BYTES`], checked before reading and enforced while reading, so a file
//!   that grows meanwhile is still cut off;
//! - only a RIFF/WAVE file, by its header, and only one whose format is a plausible sound: 1 to
//!   [`MAX_CHANNELS`] channels, [`MIN_RATE`] to [`MAX_RATE`] samples a second, 8/16/24/32-bit
//!   integer or 32-bit float PCM. A header cut short, or samples that stop before the header says
//!   they do, is refused rather than played short.
//!
//! The bytes never leave this module: the caller gets mono samples to play, or why not.
use std::io::Read;
use std::path::Path;

/// The largest sound file read: a notification is a second or two, and 10 MiB is a minute of
/// CD-quality stereo.
pub const MAX_SOUND_FILE_BYTES: u64 = 10 * 1024 * 1024;
/// The fewest samples a second a sound may have (telephone audio).
pub const MIN_RATE: u32 = 8_000;
/// The most samples a second a sound may have (the highest common studio rate).
pub const MAX_RATE: u32 = 192_000;
/// The most channels a sound may have (7.1).
pub const MAX_CHANNELS: u16 = 8;
/// "RIFF", its length and "WAVE": the smallest thing that can start a WAV file.
const RIFF_HEADER: usize = 12;

/// Why a sound file was not played. Each is named to the operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundFileRefusal {
    /// Nothing at that path.
    Missing,
    /// There, but not a regular file (a folder, a device, a pipe…).
    NotAFile,
    /// A file this computer would not let Nexus read.
    Unreadable,
    /// Larger than [`MAX_SOUND_FILE_BYTES`].
    TooLarge,
    /// Not a RIFF/WAVE file.
    NotWav,
    /// A WAV file whose header is cut short or garbled, whose format is not a plausible sound, or
    /// whose samples stop early.
    BadFormat,
}

/// A sound ready to play: mono samples in -1..=1 at `rate`.
#[derive(Debug, Clone, PartialEq)]
pub struct SoundClip {
    pub rate: u32,
    pub samples: Vec<f32>,
}

/// Read and check the WAV file at `path`, and give back its samples downmixed to mono.
pub fn read_sound_file(path: &Path) -> Result<SoundClip, SoundFileRefusal> {
    let refused = |e: std::io::Error| match e.kind() {
        std::io::ErrorKind::NotFound => SoundFileRefusal::Missing,
        _ => SoundFileRefusal::Unreadable,
    };
    // Before opening: opening a pipe for reading blocks until something writes to it.
    let meta = std::fs::metadata(path).map_err(refused)?;
    if !meta.is_file() {
        return Err(SoundFileRefusal::NotAFile);
    }
    if meta.len() > MAX_SOUND_FILE_BYTES {
        return Err(SoundFileRefusal::TooLarge);
    }
    let file = std::fs::File::open(path).map_err(refused)?;
    // …and again on what was opened, should the path have changed in between.
    if !file.metadata().map_err(refused)?.is_file() {
        return Err(SoundFileRefusal::NotAFile);
    }
    let mut bytes = Vec::new();
    file.take(MAX_SOUND_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(refused)?;
    if bytes.len() as u64 > MAX_SOUND_FILE_BYTES {
        return Err(SoundFileRefusal::TooLarge);
    }
    decode_sound(&bytes)
}

/// Check a WAV file's bytes and give back its samples downmixed to mono.
pub fn decode_sound(bytes: &[u8]) -> Result<SoundClip, SoundFileRefusal> {
    if bytes.len() < RIFF_HEADER || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(SoundFileRefusal::NotWav);
    }
    let reader = hound::WavReader::new(std::io::Cursor::new(bytes))
        .map_err(|_| SoundFileRefusal::BadFormat)?;
    let spec = reader.spec();
    let plausible_bits = match spec.sample_format {
        hound::SampleFormat::Int => matches!(spec.bits_per_sample, 8 | 16 | 24 | 32),
        hound::SampleFormat::Float => spec.bits_per_sample == 32,
    };
    if !(1..=MAX_CHANNELS).contains(&spec.channels)
        || !(MIN_RATE..=MAX_RATE).contains(&spec.sample_rate)
        || !plausible_bits
    {
        return Err(SoundFileRefusal::BadFormat);
    }
    let channels = usize::from(spec.channels);
    // The header's count of samples, held to what the bytes can carry, so a header that claims
    // more than the file holds is refused below rather than trusted for an allocation.
    let claimed = reader.len() as usize;
    if !claimed.is_multiple_of(channels) || claimed > bytes.len() {
        return Err(SoundFileRefusal::BadFormat);
    }
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .collect::<Result<_, _>>()
            .map_err(|_| SoundFileRefusal::BadFormat)?,
        hound::SampleFormat::Int => {
            let full_scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .into_samples::<i32>()
                .map(|s| s.map(|v| v as f32 / full_scale))
                .collect::<Result<_, _>>()
                .map_err(|_| SoundFileRefusal::BadFormat)?
        }
    };
    if interleaved.len() != claimed {
        return Err(SoundFileRefusal::BadFormat);
    }
    let samples = interleaved
        .chunks_exact(channels)
        .map(|frame| (frame.iter().sum::<f32>() / channels as f32).clamp(-1.0, 1.0))
        .collect();
    Ok(SoundClip {
        rate: spec.sample_rate,
        samples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nexus-sound-file-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A PCM WAV: the canonical 44-byte header, then `data`.
    fn wav(channels: u16, rate: u32, bits: u16, format: u16, data: &[u8]) -> Vec<u8> {
        let block = channels * bits / 8;
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&format.to_le_bytes());
        b.extend_from_slice(&channels.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&rate.wrapping_mul(u32::from(block)).to_le_bytes());
        b.extend_from_slice(&block.to_le_bytes());
        b.extend_from_slice(&bits.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(data.len() as u32).to_le_bytes());
        b.extend_from_slice(data);
        b
    }

    fn i16s(v: &[i16]) -> Vec<u8> {
        v.iter().flat_map(|s| s.to_le_bytes()).collect()
    }

    #[test]
    fn a_mono_16_bit_file_plays_as_written() {
        let clip =
            decode_sound(&wav(1, 12_000, 16, 1, &i16s(&[0, 16384, -16384, i16::MIN]))).unwrap();
        assert_eq!(clip.rate, 12_000);
        assert_eq!(clip.samples, vec![0.0, 0.5, -0.5, -1.0]);
    }

    #[test]
    fn a_stereo_file_is_mixed_to_mono() {
        let clip =
            decode_sound(&wav(2, 48_000, 16, 1, &i16s(&[16384, 0, -16384, -16384]))).unwrap();
        assert_eq!(clip.samples, vec![0.25, -0.5]);
    }

    #[test]
    fn a_32_bit_float_file_plays() {
        let data: Vec<u8> = [0.25f32, -0.75]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        let clip = decode_sound(&wav(1, 44_100, 32, 3, &data)).unwrap();
        assert_eq!(clip.samples, vec![0.25, -0.75]);
    }

    #[test]
    fn what_is_not_riff_wave_is_refused() {
        assert_eq!(
            decode_sound(b"ID3\x04 an mp3"),
            Err(SoundFileRefusal::NotWav)
        );
        let mut avi = wav(1, 12_000, 16, 1, &i16s(&[0]));
        avi[8..12].copy_from_slice(b"AVI ");
        assert_eq!(decode_sound(&avi), Err(SoundFileRefusal::NotWav));
        assert_eq!(
            decode_sound(b"RIFF"),
            Err(SoundFileRefusal::NotWav),
            "shorter than RIFF's header"
        );
        assert_eq!(decode_sound(&[]), Err(SoundFileRefusal::NotWav));
    }

    #[test]
    fn a_header_cut_short_is_refused() {
        let whole = wav(1, 12_000, 16, 1, &i16s(&[0, 1, 2]));
        for cut in [12, 20, 30, 40] {
            assert_eq!(
                decode_sound(&whole[..cut]),
                Err(SoundFileRefusal::BadFormat),
                "cut at {cut} bytes"
            );
        }
        assert!(
            decode_sound(&whole).is_ok(),
            "control: the whole file plays"
        );
    }

    #[test]
    fn samples_that_stop_before_the_header_says_are_refused() {
        let mut short = wav(1, 12_000, 16, 1, &i16s(&[1, 2, 3, 4]));
        short.truncate(short.len() - 3);
        assert_eq!(decode_sound(&short), Err(SoundFileRefusal::BadFormat));
        // A data length far past the file is not trusted for an allocation.
        let mut liar = wav(1, 12_000, 16, 1, &i16s(&[1]));
        let at = liar.len() - 6;
        liar[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(decode_sound(&liar), Err(SoundFileRefusal::BadFormat));
    }

    #[test]
    fn an_absurd_rate_channel_count_or_sample_size_is_refused() {
        let one = i16s(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        for (ch, rate, bits, fmt, why) in [
            (1, 7_999, 16, 1, "rate below 8 kHz"),
            (1, 192_001, 16, 1, "rate above 192 kHz"),
            (1, 4_000_000_000, 16, 1, "absurd rate"),
            (9, 12_000, 16, 1, "nine channels"),
            (1, 12_000, 12, 1, "12-bit"),
            (1, 12_000, 64, 3, "64-bit float"),
        ] {
            let data = &one[..usize::from(ch) * usize::from(bits).div_ceil(8)];
            assert_eq!(
                decode_sound(&wav(ch, rate, bits, fmt, data)),
                Err(SoundFileRefusal::BadFormat),
                "{why}"
            );
        }
        // Positive controls at each edge.
        assert!(decode_sound(&wav(1, MIN_RATE, 16, 1, &i16s(&[0]))).is_ok());
        assert!(decode_sound(&wav(1, MAX_RATE, 16, 1, &i16s(&[0]))).is_ok());
        assert!(decode_sound(&wav(MAX_CHANNELS, 12_000, 16, 1, &one[..16])).is_ok());
        // A channel count of zero cannot even be read as a header.
        assert_eq!(
            decode_sound(&wav(0, 12_000, 16, 1, &[])),
            Err(SoundFileRefusal::BadFormat)
        );
    }

    #[test]
    fn a_file_on_disk_plays_and_its_neighbours_are_refused_by_name() {
        let dir = scratch("disk");
        let good = dir.join("cq.wav");
        std::fs::write(&good, wav(1, 12_000, 16, 1, &i16s(&[0, 16384]))).unwrap();
        assert_eq!(read_sound_file(&good).unwrap().samples, vec![0.0, 0.5]);
        assert_eq!(
            read_sound_file(&dir.join("gone.wav")),
            Err(SoundFileRefusal::Missing)
        );
        assert_eq!(
            read_sound_file(&dir),
            Err(SoundFileRefusal::NotAFile),
            "a folder"
        );
        let text = dir.join("notes.wav");
        std::fs::write(&text, b"not a sound at all").unwrap();
        assert_eq!(read_sound_file(&text), Err(SoundFileRefusal::NotWav));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_over_the_cap_is_refused_before_it_is_read() {
        let dir = scratch("cap");
        let big = dir.join("big.wav");
        let f = std::fs::File::create(&big).unwrap();
        f.set_len(MAX_SOUND_FILE_BYTES + 1).unwrap();
        assert_eq!(read_sound_file(&big), Err(SoundFileRefusal::TooLarge));
        // At the cap exactly, the size is no reason (a zero-filled file is then not a WAV).
        f.set_len(MAX_SOUND_FILE_BYTES).unwrap();
        assert_eq!(read_sound_file(&big), Err(SoundFileRefusal::NotWav));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_device_or_a_pipe_is_not_a_file() {
        assert_eq!(
            read_sound_file(Path::new("/dev/zero")),
            Err(SoundFileRefusal::NotAFile),
            "a device that never ends"
        );
        let dir = scratch("fifo");
        let fifo = dir.join("pipe.wav");
        let made = std::process::Command::new("mkfifo").arg(&fifo).status();
        if made.is_ok_and(|s| s.success()) {
            // Would block forever if it were opened for reading.
            assert_eq!(read_sound_file(&fifo), Err(SoundFileRefusal::NotAFile));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
