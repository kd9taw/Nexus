//! The command link's wire, protocol 2, as both ends read and write it: the shack's channel
//! (src-tauri's `remote_service::lan::channel`, with its command lane in
//! `remote_service::lan::commands`) and the computer's link ([`crate::link`]).
//!
//! ## Frames
//!
//! Every WebSocket message either way is at most [`FRAME`] bytes, protocol 1's own bound. A control
//! message is JSON text, as on protocol 1. A pushed feed is binary ([`feed_frame`]). A message
//! longer than one frame goes as pieces ([`split`]): binary frames carrying its bytes in order under
//! a stream number of the sender's, which the other end puts back together ([`Assembly`]) before it
//! reads the message. Pieces of different messages may come between one another, and any other
//! message may come between them, so a Stop never waits behind a big call or a big answer.
//!
//! - A piece: `[1][stream: u32 LE][flags: u8][bytes]`. Flag 1: the message's last piece. Flag 2: the
//!   whole message is binary (a feed); otherwise it is JSON text.
//! - A feed: `[2][name length: u8][name][number: u64 LE][deflated JSON]` ([`feed_frame`]).
//!
//! ## The feeds
//!
//! A feed is one of the polls the screens make, run at the shack on the computer's behalf at the
//! feed's own pace and pushed, latest only ([`FEEDS`]). Each is named with its version (`snapshot/1`)
//! so a new one, or a new shape, is a new name. Every value pushed is numbered, 1, 2, 3 for each feed
//! on each connection, and the numbers have no gap and never go back: the screen's state echo names
//! the snapshot it drew by its number. Each feed is one deflate stream for the connection's life
//! ([`Deflate`], [`Inflate`]): every value is compressed with the values sent before it in reach, so
//! a snapshot is sent as about what changed. Over 11,041 snapshots recorded from a running station
//! (2026-10-07), a snapshot's frame is 165 bytes at the median (p99 597; the first, 2.4 KB)
//! against 11.3 KB of JSON, with no field-level delta format to keep.
//!
//! ## The same build at both ends
//!
//! The hello is protocol 1's in shape, so that every shack back to the first one refuses a computer
//! it cannot serve and says which side to update. The command lane then opens with the build: the
//! app's version and the contract ([`contract`]), a digest of every command's class and the
//! dispatcher's tables. Anything else is refused, saying which side to update ([`refusal`]), and
//! between two builds that call themselves the same version, `otherBuild`.
use std::sync::OnceLock;
use std::time::Duration;

use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress};
use ring::digest::{digest, SHA256};

/// The LAN protocol both ends speak: the hello and everything after it on the shack's channel. 2
/// adds the command lane (the channel's step 5, and this module).
pub const PROTOCOL_VERSION: u8 = 2;
/// The operation version every request on the shack's channel is answered at: the one that
/// carries the stop token, as on the stream's own `control` channel.
pub const OPERATION_VERSION: u8 = 4;
/// The LAN protocol, stream and operation versions this build speaks, in the order a hello names
/// them: the shack's own, and the ones its paired computers' window says.
pub const VERSIONS: (u8, u8, u8) = (
    PROTOCOL_VERSION,
    tempo_stream::protocol::STREAM_VERSION,
    OPERATION_VERSION,
);
/// The largest message either way, the relay's own bound.
pub const MESSAGE_BYTES: usize = 8192;
/// The most bytes one WebSocket message may carry, either way: protocol 1's own bound.
pub const FRAME: usize = MESSAGE_BYTES;
/// A piece's header: its kind, its stream and its flags.
const PIECE_HEADER: usize = 6;
const KIND_PIECE: u8 = 1;
const KIND_FEED: u8 = 2;
const LAST: u8 = 1;
const BINARY: u8 = 2;
/// The most a call may be, in pieces: an ADIF import, a voice message, an SSTV picture. The
/// computer refuses a bigger one itself, so a bigger one at the shack ends the connection.
pub const CALL_BYTES: usize = 16 * 1024 * 1024;
/// The most an answer may be: a whole logbook exported as text. A bigger one is answered with the
/// error [`ANSWER_TOO_LARGE`] instead.
pub const ANSWER_BYTES: usize = 64 * 1024 * 1024;
/// The most ids one `outcomes` may ask after: the calls in flight when a link dropped.
pub const OUTCOMES: usize = 64;
/// The error an answer bigger than [`ANSWER_BYTES`] is replaced with.
pub const ANSWER_TOO_LARGE: &str = "answerTooLarge";
/// Messages one end may be putting together at once.
const STREAMS: usize = 16;
/// How many messages of the largest size ([`Assembly`]'s limit) one end may hold half-sent at once,
/// all of its streams together, as a sender of this protocol interleaves its big messages a frame
/// a turn (`crate::outbox`). At the shack: the calls that reach [`CALL_BYTES`] are two, an ADIF
/// import and a voice message's `.wav`, each as big as the operator's file; every other call is far
/// smaller (an SSTV picture, PD-290's 800 x 616 as base64, is 1,971,200 bytes). So two of the
/// largest, not [`STREAMS`] of them: 16 streams of 16 MiB each let one paired computer hold 256 MiB
/// at the shack. At the computer, the same for answers: two whole logbooks.
const AT_ONCE: usize = 2;
/// The most one feed's value may be once inflated.
pub const FEED_BYTES: usize = 4 * 1024 * 1024;

/// The app's version, the same number the installer and the updater show: src-tauri's own
/// package version, which this crate's build script reads from its manifest.
pub const APP: &str = env!("NEXUS_APP_VERSION");

/// One feed: its name and version, the command the shack runs for it, how often, and whether a
/// value is pushed every time or only when it changed.
pub struct Feed {
    pub name: &'static str,
    pub command: &'static str,
    pub every: Duration,
    pub always: bool,
}

/// The feeds a computer may watch. The snapshot goes every 300 ms whether or not it changed, so
/// its number always moves while the link is live; the waterfall's row and the meters go when they
/// change, polled a little faster than the screen polls them at the shack; the RTTY tuning scope
/// goes when it changes, at half the row's pace.
pub const FEEDS: &[Feed] = &[
    Feed {
        name: "snapshot/1",
        command: "get_snapshot",
        every: Duration::from_millis(300),
        always: true,
    },
    Feed {
        name: "spectrumRow/1",
        command: "get_spectrum_row",
        every: Duration::from_millis(50),
        always: false,
    },
    Feed {
        name: "meters/1",
        command: "get_meters",
        every: Duration::from_millis(100),
        always: false,
    },
    // The RTTY tuning scope's newest frame: 256 points as i8 pairs, about 2 KB of JSON before
    // deflate, at half the waterfall row's pace. Each frame holds about 85 ms of points, so 100 ms
    // draws nearly all of them. It changes only while the shack's RTTY decoder is armed; disarmed,
    // the shack's poll answers the same empty frame and nothing is pushed.
    Feed {
        name: "rttyScope/1",
        command: "get_rtty_scope",
        every: Duration::from_millis(100),
        always: false,
    },
];

/// The feed named `name`, if there is one.
pub fn feed(name: &str) -> Option<&'static Feed> {
    FEEDS.iter().find(|feed| feed.name == name)
}

/// A whole message, before it is split or once it is put back together.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Whole {
    Text(String),
    Binary(Vec<u8>),
}

impl Whole {
    fn len(&self) -> usize {
        match self {
            Whole::Text(text) => text.len(),
            Whole::Binary(bytes) => bytes.len(),
        }
    }
}

/// `whole` as the frames it goes in: itself, when it fits one, or its pieces under `stream`.
pub fn split(whole: Whole, stream: u32) -> Vec<Vec<u8>> {
    let (binary, bytes) = match whole {
        Whole::Text(text) => (false, text.into_bytes()),
        Whole::Binary(bytes) => (true, bytes),
    };
    let size = FRAME - PIECE_HEADER;
    let count = bytes.len().div_ceil(size).max(1);
    (0..count)
        .map(|n| {
            let part = &bytes[(n * size).min(bytes.len())..((n + 1) * size).min(bytes.len())];
            let mut frame = Vec::with_capacity(PIECE_HEADER + part.len());
            frame.push(KIND_PIECE);
            frame.extend_from_slice(&stream.to_le_bytes());
            frame.push(if n + 1 == count { LAST } else { 0 } | if binary { BINARY } else { 0 });
            frame.extend_from_slice(part);
            frame
        })
        .collect()
}

/// Whether `whole` goes as one frame of its own.
pub fn fits(whole: &Whole) -> bool {
    whole.len() <= FRAME
}

/// The pieces one end is putting back together. Anything a sender of this protocol never sends
/// (a piece of no kind, a message over the limit, too many at once, more than [`AT_ONCE`] messages
/// of the limit held in all, a message whose pieces disagree on its kind, text that is not UTF-8)
/// is refused, and the connection ends.
pub struct Assembly {
    open: std::collections::HashMap<u32, (bool, Vec<u8>)>,
    limit: usize,
    /// The bytes held in messages not yet whole, every stream's together.
    held: usize,
}

impl Assembly {
    /// For messages of at most `limit` bytes.
    pub fn new(limit: usize) -> Self {
        Self {
            open: Default::default(),
            limit,
            held: 0,
        }
    }

    /// The bytes held in messages not yet whole.
    #[cfg(test)]
    fn held(&self) -> usize {
        self.held
    }

    /// One piece frame, its kind byte included: the whole message when this was its last piece.
    pub fn piece(&mut self, frame: &[u8]) -> Result<Option<Whole>, ()> {
        if frame.len() < PIECE_HEADER || frame[0] != KIND_PIECE {
            return Err(());
        }
        let stream = u32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]);
        let (flags, part) = (frame[5], &frame[PIECE_HEADER..]);
        if flags & !(LAST | BINARY) != 0 {
            return Err(());
        }
        let binary = flags & BINARY != 0;
        if !self.open.contains_key(&stream) && self.open.len() >= STREAMS {
            return Err(());
        }
        let (kind, bytes) = self.open.entry(stream).or_insert((binary, Vec::new()));
        let held: usize = bytes.len() + part.len();
        if *kind != binary || held > self.limit || self.held + part.len() > AT_ONCE * self.limit {
            return Err(());
        }
        bytes.extend_from_slice(part);
        self.held += part.len();
        if flags & LAST == 0 {
            return Ok(None);
        }
        let (binary, bytes) = self.open.remove(&stream).ok_or(())?;
        self.held -= bytes.len();
        if binary {
            Ok(Some(Whole::Binary(bytes)))
        } else {
            String::from_utf8(bytes)
                .map(|text| Some(Whole::Text(text)))
                .map_err(|_| ())
        }
    }
}

/// Whether a binary message is a piece (as against a feed).
pub fn is_piece(frame: &[u8]) -> bool {
    frame.first() == Some(&KIND_PIECE)
}

/// One feed's stream at the shack: every value it sends is compressed after the ones before it.
pub struct Deflate(Compress);

impl Default for Deflate {
    fn default() -> Self {
        // Raw deflate, its 32 KiB window carried from one value to the next.
        Self(Compress::new(Compression::default(), false))
    }
}

impl Deflate {
    /// `json` compressed, flushed so the other end can read it whole now. `None` only if the
    /// compressor fails, which ends the connection.
    pub fn value(&mut self, json: &[u8]) -> Option<Vec<u8>> {
        let mut out = Vec::with_capacity(json.len() / 4 + 64);
        let before = self.0.total_in();
        loop {
            if out.capacity() - out.len() < 64 {
                out.reserve(out.capacity().max(64));
            }
            let read = (self.0.total_in() - before) as usize;
            self.0
                .compress_vec(&json[read..], &mut out, FlushCompress::Sync)
                .ok()?;
            let read = (self.0.total_in() - before) as usize;
            // Flushed: everything read, and room left over that the compressor did not need.
            if read == json.len() && out.len() < out.capacity() {
                return Some(out);
            }
        }
    }
}

/// One feed's stream at the computer, the other half of [`Deflate`].
pub struct Inflate(Decompress);

impl Default for Inflate {
    fn default() -> Self {
        Self(Decompress::new(false))
    }
}

impl Inflate {
    /// The value `deflated` carries, or `None` for anything the shack's [`Deflate`] never makes or
    /// a value over [`FEED_BYTES`]: the link then ends, since the stream is lost.
    pub fn value(&mut self, deflated: &[u8]) -> Option<Vec<u8>> {
        let mut out = Vec::with_capacity((deflated.len() * 8).min(FEED_BYTES) + 64);
        let before = self.0.total_in();
        loop {
            if out.capacity() - out.len() < 64 {
                if out.len() >= FEED_BYTES {
                    return None;
                }
                out.reserve(out.capacity().max(64));
            }
            let (read_was, made_was) = (self.0.total_in(), out.len());
            let read = (read_was - before) as usize;
            self.0
                .decompress_vec(&deflated[read..], &mut out, FlushDecompress::Sync)
                .ok()?;
            let read = (self.0.total_in() - before) as usize;
            if read == deflated.len() && out.len() < out.capacity() {
                break;
            }
            if self.0.total_in() == read_was && out.len() == made_was {
                return None;
            }
        }
        (out.len() <= FEED_BYTES).then_some(out)
    }
}

/// A feed's value as it goes on the wire, `number` the feed's count of values sent.
pub fn feed_frame(name: &str, number: u64, deflated: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(2 + name.len() + 8 + deflated.len());
    frame.push(KIND_FEED);
    frame.push(name.len() as u8);
    frame.extend_from_slice(name.as_bytes());
    frame.extend_from_slice(&number.to_le_bytes());
    frame.extend_from_slice(deflated);
    frame
}

/// A feed frame read back: its name, number and deflated value.
pub fn read_feed(frame: &[u8]) -> Option<(&str, u64, &[u8])> {
    if frame.first() != Some(&KIND_FEED) {
        return None;
    }
    let length = usize::from(*frame.get(1)?);
    let name = std::str::from_utf8(frame.get(2..2 + length)?).ok()?;
    let number = u64::from_le_bytes(frame.get(2 + length..10 + length)?.try_into().ok()?);
    Some((name, number, frame.get(10 + length..)?))
}

// --- The build ------------------------------------------------------------------------------------

/// This build's contract: SHA-256, lowercase hex, of this protocol's number, every command's class,
/// the dispatcher's tables, the feeds, and the datagrams of the audio road. Two builds with the same
/// contract route, admit and push every command alike, and read each other's audio road. Stop's
/// second road is not in it: its format is frozen across builds (`crate::stop_road::wire`).
pub fn contract() -> &'static str {
    static CONTRACT: OnceLock<String> = OnceLock::new();
    CONTRACT.get_or_init(|| {
        digest(&SHA256, contract_text().as_bytes())
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    })
}

/// What [`contract`] digests.
fn contract_text() -> String {
    let mut text = format!("nexus command link\nprotocol {}\n", PROTOCOL_VERSION);
    text.push_str(&crate::dispatch::contract_text());
    for feed in FEEDS {
        text.push_str(&format!(
            "feed {} {} {} {}\n",
            feed.name,
            feed.command,
            feed.every.as_millis(),
            feed.always
        ));
    }
    text.push_str(&crate::audio_road::wire::contract_text());
    text.push_str(&crate::ice::contract_text());
    text
}

/// Who is speaking: the end deciding which side should update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    Shack,
    Computer,
}

/// Which side to update when the other end runs `app` with `contract` and this end runs `ours`
/// (an app and a contract): the older one, or `otherBuild` for two builds of one version (or a
/// version this build cannot read). `None` for the same build.
pub fn refusal(us: End, ours: (&str, &str), theirs: (&str, &str)) -> Option<&'static str> {
    if ours == theirs {
        return None;
    }
    let (update_us, update_them) = match us {
        End::Shack => ("updateStation", "updateComputer"),
        End::Computer => ("updateComputer", "updateStation"),
    };
    match (version(ours.0), version(theirs.0)) {
        (Some(mine), Some(other)) if mine < other => Some(update_us),
        (Some(mine), Some(other)) if mine > other => Some(update_them),
        _ => Some("otherBuild"),
    }
}

/// A version as Nexus numbers its builds, `major.minor.patch` with a prerelease after a dash
/// (`1.17.0-test1`), as a key that orders them: a prerelease comes before the release it leads to.
/// Two prereleases of one version share a key, and so read as two builds of one version.
fn version(text: &str) -> Option<(u64, u64, u64, bool)> {
    let (numbers, pre) = match text.split_once('-') {
        Some((numbers, pre)) => (numbers, Some(pre)),
        None => (text, None),
    };
    let mut parts = numbers.split('.').map(|part| part.parse::<u64>().ok());
    let key = (
        parts.next()??,
        parts.next()??,
        parts.next()??,
        pre.is_none(),
    );
    (parts.next().is_none() && pre.is_none_or(|pre| !pre.is_empty())).then_some(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every feed runs a read at the shack: a command classed `RunsAtShack`, never one that can
    /// start or stop a transmission, so watching a feed can change nothing at the station. The
    /// RTTY tuning scope is one of them, and its pace is no faster than the waterfall row's.
    #[test]
    fn every_feed_is_a_shack_read_and_the_tuning_scope_is_one() {
        for f in FEEDS {
            assert_eq!(
                crate::classes::class_of(f.command),
                Some(crate::classes::Class::RunsAtShack),
                "{} runs {}",
                f.name,
                f.command
            );
        }
        let scope = feed("rttyScope/1").expect("the tuning scope's feed");
        assert_eq!(scope.command, "get_rtty_scope");
        assert!(!scope.always, "pushed only when it changed");
        assert!(scope.every >= feed("spectrumRow/1").unwrap().every);
    }

    /// ★ Pieces: a message of any size goes as one frame when it fits and as pieces otherwise, each
    /// at most a frame, and comes back whole, text as text and bytes as bytes, with the pieces of
    /// two messages taken in turn. CONTROL: one byte more than fits makes two pieces.
    #[test]
    fn a_message_goes_in_frames_of_at_most_eight_kib_and_comes_back_whole() {
        let size = FRAME - PIECE_HEADER;
        for length in [0, 1, size - 1, size, size + 1, 3 * size, 100_000] {
            for whole in [
                Whole::Text("é".repeat(length / 2) + &"x".repeat(length % 2)),
                Whole::Binary((0..length).map(|n| n as u8).collect()),
            ] {
                let frames = split(whole.clone(), 7);
                assert!(frames.iter().all(|frame| frame.len() <= FRAME));
                assert_eq!(frames.len(), whole.len().div_ceil(size).max(1), "{length}");
                let mut pieces = Assembly::new(CALL_BYTES);
                let (last, before) = frames.split_last().unwrap();
                for frame in before {
                    assert_eq!(pieces.piece(frame), Ok(None));
                }
                assert_eq!(pieces.piece(last), Ok(Some(whole)), "{length}");
            }
        }
        assert!(
            fits(&Whole::Text("x".repeat(FRAME))),
            "a frame's worth is one frame"
        );
        assert!(!fits(&Whole::Text("x".repeat(FRAME + 1))));
        // Two messages' pieces taken in turn, each whole.
        let (a, b) = (
            Whole::Text("a".repeat(3 * size)),
            Whole::Binary(vec![9; 2 * size + 5]),
        );
        let (mut first, mut second) = (
            split(a.clone(), 1).into_iter(),
            split(b.clone(), 2).into_iter(),
        );
        let mut pieces = Assembly::new(CALL_BYTES);
        let mut done = Vec::new();
        loop {
            let (x, y) = (first.next(), second.next());
            if x.is_none() && y.is_none() {
                break;
            }
            for frame in x.into_iter().chain(y) {
                done.extend(pieces.piece(&frame).unwrap());
            }
        }
        assert_eq!(done, vec![a, b]);
    }

    /// ★ The pieces' refusals: a message over the limit, more messages at once than [`STREAMS`],
    /// pieces of one message that disagree on its kind, text that is not UTF-8, an unknown flag and
    /// a frame shorter than its header each end the connection. CONTROL: a message of exactly the
    /// limit, and [`STREAMS`] at once, are taken.
    #[test]
    fn a_piece_no_sender_of_this_protocol_makes_is_refused() {
        let piece = |stream: u32, flags: u8, bytes: &[u8]| {
            let mut frame = vec![KIND_PIECE];
            frame.extend_from_slice(&stream.to_le_bytes());
            frame.push(flags);
            frame.extend_from_slice(bytes);
            frame
        };
        let mut pieces = Assembly::new(10);
        assert_eq!(
            pieces.piece(&piece(1, LAST, &[b'x'; 10])),
            Ok(Some(Whole::Text("x".repeat(10))))
        );
        assert_eq!(pieces.piece(&piece(1, LAST, &[b'x'; 11])), Err(()));
        let mut pieces = Assembly::new(10);
        assert_eq!(pieces.piece(&piece(1, 0, &[b'x'; 6])), Ok(None));
        assert_eq!(
            pieces.piece(&piece(1, LAST, &[b'x'; 5])),
            Err(()),
            "over the limit in sum"
        );
        let mut pieces = Assembly::new(10);
        for stream in 0..STREAMS as u32 {
            assert_eq!(pieces.piece(&piece(stream, 0, b"x")), Ok(None));
        }
        assert_eq!(
            pieces.piece(&piece(99, 0, b"x")),
            Err(()),
            "one too many at once"
        );
        let mut pieces = Assembly::new(10);
        assert_eq!(pieces.piece(&piece(1, 0, b"x")), Ok(None));
        assert_eq!(
            pieces.piece(&piece(1, LAST | BINARY, b"x")),
            Err(()),
            "kind changed"
        );
        let mut pieces = Assembly::new(10);
        assert_eq!(
            pieces.piece(&piece(1, LAST, &[0xff, 0xfe])),
            Err(()),
            "not UTF-8"
        );
        assert_eq!(pieces.piece(&piece(1, 4, b"x")), Err(()), "unknown flag");
        assert_eq!(pieces.piece(&[KIND_PIECE, 1, 0]), Err(()), "short");
        assert_eq!(
            pieces.piece(&[KIND_FEED, 0, 0, 0, 0, 1]),
            Err(()),
            "not a piece"
        );
    }

    /// ★ What one end holds in messages not yet whole is bounded across all of them: a
    /// sender that leaves message after message unfinished is refused once the bytes held pass
    /// [`AT_ONCE`] messages of the limit, and the connection ends, where before every stream could
    /// hold the limit. A finished message gives its room back. CONTROL: [`AT_ONCE`] messages of the
    /// limit each, half-sent at once, are taken, as a real sender's interleaved big calls are.
    #[test]
    fn messages_not_yet_whole_hold_at_most_the_budget_together() {
        let piece = |stream: u32, flags: u8, bytes: &[u8]| {
            let mut frame = vec![KIND_PIECE];
            frame.extend_from_slice(&stream.to_le_bytes());
            frame.push(flags);
            frame.extend_from_slice(bytes);
            frame
        };
        // The flood: every stream filled to the limit, none finished.
        let mut pieces = Assembly::new(10);
        let taken = (0..STREAMS as u32)
            .take_while(|stream| pieces.piece(&piece(*stream, 0, &[b'x'; 10])).is_ok())
            .count();
        assert_eq!(taken, AT_ONCE, "streams taken at the limit each");
        assert_eq!(pieces.held(), AT_ONCE * 10);
        // One byte past the budget, on a stream already open, is refused too.
        let mut pieces = Assembly::new(10);
        assert_eq!(pieces.piece(&piece(1, 0, &[b'x'; 10])), Ok(None));
        assert_eq!(pieces.piece(&piece(2, 0, &[b'x'; 9])), Ok(None));
        assert_eq!(
            pieces.piece(&piece(2, 0, b"x")),
            Ok(None),
            "exactly the budget"
        );
        assert_eq!(pieces.piece(&piece(3, 0, b"x")), Err(()), "a byte past it");
        // A finished message gives its room back.
        let mut pieces = Assembly::new(10);
        assert_eq!(pieces.piece(&piece(1, 0, &[b'x'; 10])), Ok(None));
        assert_eq!(pieces.piece(&piece(2, 0, &[b'x'; 10])), Ok(None));
        assert_eq!(
            pieces.piece(&piece(1, LAST, b"")),
            Ok(Some(Whole::Text("x".repeat(10))))
        );
        assert_eq!(pieces.held(), 10);
        assert_eq!(pieces.piece(&piece(3, 0, &[b'x'; 10])), Ok(None));
        assert_eq!(pieces.held(), 20);
    }

    /// ★ A feed's deflate stream: each value comes back exactly, in order, through one inflate
    /// stream, and a value much like the one before it costs a small fraction of the first.
    /// CONTROL: the same values each compressed alone cost several times as much.
    #[test]
    fn each_feed_value_is_deflated_against_the_ones_before_it() {
        // As a snapshot changes: its radio block every time, the station list now and then.
        let value = |n: u32| {
            serde_json::json!({"radio":{"dialHz":14_074_000 + n,"smeterDb":n % 7},
                "stations":(0..40).map(|s| format!("K{s}ABC EN52 -{}", (s + n / 10) % 20))
                    .collect::<Vec<_>>()})
            .to_string()
            .into_bytes()
        };
        let (mut deflate, mut inflate) = (Deflate::default(), Inflate::default());
        let mut sizes = Vec::new();
        for n in 0..50 {
            let json = value(n);
            let deflated = deflate.value(&json).unwrap();
            sizes.push(deflated.len());
            assert_eq!(inflate.value(&deflated).unwrap(), json, "value {n}");
        }
        let alone: usize = (0..50)
            .map(|n| Deflate::default().value(&value(n)).unwrap().len())
            .sum();
        let streamed: usize = sizes.iter().sum();
        assert!(sizes[49] * 5 < sizes[0], "{sizes:?}");
        assert!(streamed * 3 < alone, "streamed {streamed}, alone {alone}");
    }

    /// ★ Inflate takes only what the shack's deflate makes: bytes that are no deflate stream, and a
    /// value over [`FEED_BYTES`] once inflated, are refused. CONTROL: a value of a megabyte inflates.
    #[test]
    fn inflate_refuses_what_no_shack_sends() {
        assert_eq!(Inflate::default().value(&[0xff; 32]), None);
        let big = vec![b'x'; FEED_BYTES + 1];
        let deflated = Deflate::default().value(&big).unwrap();
        assert_eq!(Inflate::default().value(&deflated), None);
        let fine = vec![b'x'; 1 << 20];
        let deflated = Deflate::default().value(&fine).unwrap();
        assert_eq!(Inflate::default().value(&deflated), Some(fine));
    }

    /// ★ A feed frame reads back as its name, number and value. CONTROL: a piece, a frame shorter
    /// than its name or number, and a name that is not UTF-8 read as nothing.
    #[test]
    fn a_feed_frame_reads_back_whole() {
        let frame = feed_frame("snapshot/1", 42, b"deflated");
        assert_eq!(
            read_feed(&frame),
            Some(("snapshot/1", 42, &b"deflated"[..]))
        );
        assert_eq!(read_feed(&frame[..frame.len() - 8 - 8]), None);
        assert_eq!(read_feed(&split(Whole::Text("x".into()), 1)[0]), None);
        assert_eq!(
            read_feed(&[KIND_FEED, 2, 0xff, 0xfe, 0, 0, 0, 0, 0, 0, 0, 1]),
            None
        );
    }

    /// ★ Which side to update: between two builds, the older one is told to update, from either
    /// end, a prerelease being older than the release it leads to; two builds of one version (or a
    /// version this build cannot read) are `otherBuild`. CONTROL: the same build is refused
    /// nothing.
    #[test]
    fn which_side_to_update_is_the_older_one() {
        let ours = ("1.17.0", "c1");
        assert_eq!(refusal(End::Shack, ours, ours), None);
        for (theirs, shack, computer) in [
            (("1.18.0", "c2"), "updateStation", "updateComputer"),
            (("1.17.1", "c2"), "updateStation", "updateComputer"),
            (("2.0.0", "c2"), "updateStation", "updateComputer"),
            (("1.16.9", "c2"), "updateComputer", "updateStation"),
            (("1.17.0-test1", "c2"), "updateComputer", "updateStation"),
            (("1.17.0", "c2"), "otherBuild", "otherBuild"),
            (("1.17", "c2"), "otherBuild", "otherBuild"),
            (("1.17.0-", "c2"), "otherBuild", "otherBuild"),
            (("one", "c2"), "otherBuild", "otherBuild"),
        ] {
            assert_eq!(refusal(End::Shack, ours, theirs), Some(shack), "{theirs:?}");
            assert_eq!(
                refusal(End::Computer, ours, theirs),
                Some(computer),
                "{theirs:?}"
            );
        }
        assert_eq!(
            refusal(End::Shack, ("1.17.0-test1", "c1"), ("1.17.0-test2", "c2")),
            Some("otherBuild")
        );
        assert_eq!(
            refusal(End::Shack, ("1.17.0-test1", "c1"), ("1.17.0", "c2")),
            Some("updateStation")
        );
    }

    /// ★ The contract is this build's: a SHA-256 in hex, the same every time it is asked, made from
    /// every command's class and every feed. CONTROL: the text it digests names each command once.
    #[test]
    fn the_contract_covers_every_command_and_feed() {
        let digest = contract();
        assert_eq!(digest.len(), 64);
        assert!(digest
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
        assert_eq!(digest, contract());
        let text = crate::dispatch::contract_text();
        for (cmd, class) in crate::classes::classes() {
            assert_eq!(
                text.matches(&format!("class {cmd} {class:?}\n")).count(),
                1,
                "{cmd}"
            );
        }
        for (cmd, wired) in crate::dispatch::WIRED {
            assert_eq!(
                text.matches(&format!("wired {cmd} {wired:?}\n")).count(),
                1,
                "{cmd}"
            );
        }
        assert!(text.contains("event alert\n") && text.contains("profile desktop\n"));
        assert!(crate::audio_road::wire::contract_text()
            .starts_with("audio road 1 EXPORTER-nexus-audio-road format 1 max 512 "));
        // Stop's second road is frozen, outside the build's check: a build that reads its bytes
        // as this one does is not refused for its format. CONTROL: the audio road is in it.
        let whole = contract_text();
        assert!(whole.contains("audio road 1 EXPORTER-nexus-audio-road "));
        assert!(!whole.contains("stop road") && !whole.contains("EXPORTER-nexus-stop-road"));
    }

    /// ★ A phone's profile is known by name and refused, and the contract names the served
    /// profiles alone: the desktop's line, once, as a 1.20.0 desktop's contract has it, so knowing a
    /// phone moves no digest two desktops compare. CONTROL: the desktop is served and named.
    #[test]
    fn a_phone_is_known_by_name_and_left_out_of_the_contract() {
        use crate::dispatch::Profile;
        for profile in [Profile::Desktop, Profile::Mobile] {
            assert_eq!(Profile::named(profile.name()), Some(profile));
        }
        assert_eq!(Profile::Mobile.name(), "mobile");
        assert_eq!(Profile::named("tablet"), None);
        assert!(Profile::Desktop.served() && !Profile::Mobile.served());
        let text = crate::dispatch::contract_text();
        let profiles: Vec<&str> = text.lines().filter(|l| l.starts_with("profile ")).collect();
        assert_eq!(profiles, ["profile desktop"]);
        assert!(!text.contains("mobile"), "{text}");
    }

    /// ★ A waterfall row crosses the link lossless, on this network and through the room alike
    /// (the same feed on both): a quiet band's row, its neighbouring values one f32 step apart, is
    /// read here bit for bit as the shack made it, and what the screens here are handed is the same
    /// text the shack's own screens read. No 8-bit row, so the waterfall's AGC never bands a quiet
    /// band. CONTROL: a row rounded to 8 bits, as a byte row would carry it, loses the steps.
    #[test]
    fn a_waterfall_row_crosses_the_link_bit_for_bit() {
        #[derive(serde::Serialize)]
        struct Row {
            row: Vec<f32>,
        }
        let mut level = -121.37_f32;
        let row: Vec<f32> = (0..2048)
            .map(|i| {
                level = if i % 7 == 0 {
                    f32::from_bits(level.to_bits() - 1)
                } else {
                    f32::from_bits(level.to_bits() + 1)
                };
                level
            })
            .collect();
        let shack = serde_json::to_vec(&Row { row: row.clone() }).unwrap();
        let deflated = Deflate::default().value(&shack).unwrap();
        let frame = feed_frame("spectrumRow/1", 9, &deflated);
        let (name, number, carried) = read_feed(&frame).unwrap();
        assert_eq!((name, number), ("spectrumRow/1", 9));
        let here: serde_json::Value =
            serde_json::from_slice(&Inflate::default().value(carried).unwrap()).unwrap();
        let read: Vec<u32> = here["row"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| (v.as_f64().unwrap() as f32).to_bits())
            .collect();
        assert_eq!(read, row.iter().map(|v| v.to_bits()).collect::<Vec<_>>());
        assert_eq!(serde_json::to_vec(&here).unwrap(), shack);
        let distinct = |values: &[f32]| {
            let mut bits: Vec<u32> = values.iter().map(|v| v.to_bits()).collect();
            bits.sort_unstable();
            bits.dedup();
            bits.len()
        };
        let bytes: Vec<f32> = row
            .iter()
            .map(|v| {
                f32::from((((v + 160.0) / 160.0) * 255.0).round() as u8) / 255.0 * 160.0 - 160.0
            })
            .collect();
        assert!(distinct(&row) > 4, "premise: the row has its steps");
        assert_eq!(
            distinct(&bytes),
            1,
            "control: a byte row keeps none of them"
        );
    }
}
