//! Multi-frame reassembly — JS8Call's RX message buffer (mainwindow.cpp:3955-4470, 8378-8690,
//! read as the spec). `Reassembler::feed` turns a stream of decoded frames into (1) one
//! `MessageEvent::Frame` per frame (the activity-pane row) and (2) a `MessageEvent::Message`
//! whenever a directed message CLOSES — complete, or force-closed/incomplete, but NEVER
//! silently dropped.
//!
//! CONTRACT (mainwindow.cpp):
//! - Frames group by AUDIO OFFSET with drift tolerance: a frame joins an open buffer at the same
//!   speed when `|Δf| ≤ speed.drift_hz()` (JS8::Submode::rxThreshold — Slow/Normal 10, Fast 16,
//!   Turbo 32; identical to `phy::Speed::drift_hz`). A `First` frame CLEARS any stale buffer at
//!   that offset (:3999-4002). A `Last` frame CLOSES the buffer (:8443). 60 s past the newest
//!   frame force-closes it as if Last; 90 s drops it `complete:false` (interfaces §1.12).
//! - Heartbeats are logged as FAUX directed commands (:4082-4132): an HB frame → `CALL: @HB
//!   HEARTBEAT`, a CQ (the "Alt" bit) → `CALL: @ALLCALL CQ ` + the full frame render (the
//!   DIRECTED.TXT doubling, :8656-8665). The grid rides the message but is NOT in the directed
//!   line.
//! - A directed command is BUFFERED when `isCommandBuffered(cmd)` and it is not the Last frame,
//!   OR when its from/to is the `<....>` compound placeholder (:4145). Data frames at the offset
//!   append to the body (:4004-4008); a queued `Compound` frame resolves a `<....>` (:8420-8440).
//! - On close, buffered commands that are checksummed (`Command::is_checksummed`) VERIFY the
//!   trailing 3-char base-41 CRC-16/KERMIT (`crc16::verify_checksum3`); on failure the message is
//!   emitted `complete:false` with `Checksum::Bad` and the raw body — never a silently merged text.
//! - `render_directed` reproduces the DIRECTED.TXT line JS8Call writes (:8648-8686): the join of
//!   `"{from}: {to}{cmd}"`, the SNR/number extra, and the reassembled body, rstripped (the ♢ EOT
//!   and trailing space are added at write time and are what the fixture sanitiser removed).
//!
//! ⚠️ KNOWN UPSTREAM FLAW, reproduced for parity but surfaced honestly: buffers are keyed by
//! offset alone, so two stations transmitting on the same 50 Hz slot MERGE into one buffer. We
//! reproduce the merge (a `First` at the offset clears the prior buffer; a stray tail attaches to
//! whatever is open) but never present a merged body as confident text — a checksummed command
//! whose CRC then fails closes `complete:false`/`Checksum::Bad`, and a non-checksummed merge is
//! marked by `frames`/`complete` for the caller to judge, not asserted as correct.
//!
//! `to: Option<CallRef>` (the interfaces field) cannot represent a compound destination
//! ("N3CHX/P1") or an unlisted group ("@SITREP"), both of which appear in real traffic, so the
//! rendered destination is carried in `to_text` and `to` is the structured form when one exists.
use crate::phy::{RawDecode, Speed, I3};
use crate::proto::alphabet::CQS;
use crate::proto::callsign::CallRef;
use crate::proto::command::Command;
use crate::proto::crc16::verify_checksum3;
use crate::proto::frame::{decode_word, format_snr, Frame};

/// One decoded frame in arrival order — the activity-pane row.
#[derive(Debug, Clone, PartialEq)]
pub struct RxFrame {
    pub frame: Frame,
    pub i3: I3,
    pub speed: Speed,
    pub freq_hz: f32,
    pub snr_db: i32,
    pub dt_s: f32,
    pub at_ms: u64,
    /// `Frame::render` — JS8Call's ALL.TXT text column, byte for byte.
    pub display: String,
}

/// Whether a closed message's buffered-command checksum was checked and, if so, passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checksum {
    NotRequired,
    Ok,
    Bad,
}

/// A closed directed message — the DIRECTED.TXT line, reassembled.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub from: String,
    pub to: Option<CallRef>,
    /// The rendered destination (holds compound calls / unlisted groups `to` cannot).
    pub to_text: String,
    pub cmd: Option<Command>,
    pub num: Option<i8>,
    pub text: String,
    pub checksum: Checksum,
    /// Relay hops parsed from `*DE*` / `>` tokens, sender first (station layer fills the chain).
    pub path: Vec<String>,
    pub freq_hz: f32,
    pub snr_db: i32,
    pub speed: Speed,
    pub first_ms: u64,
    pub last_ms: u64,
    pub frames: u16,
    /// false on 60 s force-close / 90 s drop / `Checksum::Bad`.
    pub complete: bool,
    pub compound_from: Option<String>,
    /// HB/CQ grid (rides the message; NOT in the directed line).
    pub grid: Option<String>,
    /// CQ index (`CQS`) when this is a CQ; `None` for a plain HB or a directed message.
    pub cq: Option<u8>,
}

impl Message {
    /// A faux-directed heartbeat/CQ (to `@HB` / `@ALLCALL`) rather than a real directed command.
    pub fn is_heartbeat(&self) -> bool {
        self.cmd.is_none() && (self.to_text == "@HB" || self.to_text == "@ALLCALL")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum MessageEvent {
    /// Every decoded frame, in arrival order.
    Frame(RxFrame),
    /// A message closed — complete or incomplete, never silently dropped.
    Message(Message),
}

/// JS8Call's DIRECTED.TXT line (mainwindow.cpp:8648-8686), the golden oracle for the message
/// layer. The ♢ end-of-transmission marker and trailing space are added at write time and are
/// NOT part of this string (the fixture sanitiser removes them).
pub fn render_directed(m: &Message) -> String {
    if m.is_heartbeat() {
        if let Some(cq) = m.cq {
            // "{from}: @ALLCALL CQ " + the full frame render, rstripped (the DIRECTED.TXT doubling)
            let grid = m.grid.as_deref().unwrap_or("");
            let full = format!("{}: @ALLCALL {} {} ", m.from, CQS[(cq & 7) as usize], grid);
            return format!("{}: @ALLCALL CQ {}", m.from, full.trim_end());
        }
        return format!("{}: @HB HEARTBEAT", m.from);
    }
    let cmd_text = m.cmd.map(|c| c.text()).unwrap_or("");
    let head = format!("{}: {}{}", m.from, m.to_text, cmd_text);
    let mut parts = vec![head];
    if let Some(c) = m.cmd {
        match m.num {
            Some(n) if c.carries_snr() => parts.push(format_snr(n as i32)),
            Some(n) => parts.push(n.to_string()),
            None => {}
        }
    }
    if !m.text.is_empty() {
        parts.push(m.text.clone());
    }
    // Upstream rstrips the line before appending the EOT marker (mainwindow.cpp:8681); a
    // Freetext directed command with no body would otherwise keep the command's own space.
    parts.join(" ").trim_end().to_string()
}

const CLOSE_MS: u64 = 60_000;
const DROP_MS: u64 = 90_000;
/// Hard ceiling on simultaneously-open reassembly buffers. `age` bounds `open` in practice
/// (buffers past DROP_MS drop), so this fires only against a pathological flood of one-frame
/// decodes at distinct offsets; the least-recently-touched buffer is evicted (LRU), matching
/// the count caps in station.rs. The JS8 decode band holds ~78 50 Hz slots per speed, so 128
/// clears a very busy real band across speeds with headroom.
const MAX_OPEN_BUFFERS: usize = 128;

/// A compound-callsign announcement queued to resolve a `<....>` placeholder.
#[derive(Debug, Clone)]
struct Compound {
    call: String,
    grid: Option<String>,
}

/// The directed head of an open buffer (before its body and any placeholders resolve).
#[derive(Debug, Clone)]
struct Head {
    from: String,
    to: String,
    cmd: Option<Command>,
    num: Option<i8>,
    checksummed: bool,
}

/// One open message buffer at an audio offset.
#[derive(Debug, Clone)]
struct Buffer {
    offset: f32,
    speed: Speed,
    head: Option<Head>,
    compounds: Vec<Compound>,
    msgs: Vec<String>,
    first_ms: u64,
    last_ms: u64,
    frames: u16,
    freq_hz: f32,
    snr_db: i32,
}

/// JS8Call's RX message buffer. Feed decoded frames in arrival order; collect events.
#[derive(Debug, Default)]
pub struct Reassembler {
    open: Vec<Buffer>,
}

impl Reassembler {
    pub fn new() -> Reassembler {
        Reassembler { open: Vec::new() }
    }

    /// Decode `rx`, emit its `Frame` event, thread it into the offset buffers, and emit any
    /// `Message` that closes as a result (a `Last` frame, or a single-frame directed command).
    pub fn feed(&mut self, rx: &RawDecode, now_ms: u64) -> Vec<MessageEvent> {
        let mut events = Vec::new();
        let Ok((frame, i3)) = decode_word(&rx.word, rx.speed) else {
            return events; // a CRC-bad word never reaches here from the decoder; ignore defensively
        };
        let display = frame.render();
        let rxf = RxFrame {
            frame: frame.clone(),
            i3,
            speed: rx.speed,
            freq_hz: rx.freq_hz,
            snr_db: rx.snr_db,
            dt_s: rx.dt_s,
            at_ms: now_ms,
            display,
        };
        events.push(MessageEvent::Frame(rxf));

        match &frame {
            // Heartbeat / CQ: a single-frame faux-directed message, emitted immediately.
            Frame::Heartbeat {
                call,
                grid,
                is_cq,
                idx,
            } => {
                let (to_text, cq) = if *is_cq {
                    ("@ALLCALL", Some(*idx))
                } else {
                    ("@HB", None)
                };
                events.push(MessageEvent::Message(Message {
                    from: call.clone(),
                    to: CallRef::parse(to_text),
                    to_text: to_text.to_string(),
                    cmd: None,
                    num: None,
                    text: String::new(),
                    checksum: Checksum::NotRequired,
                    path: Vec::new(),
                    freq_hz: rx.freq_hz,
                    snr_db: rx.snr_db,
                    speed: rx.speed,
                    first_ms: now_ms,
                    last_ms: now_ms,
                    frames: 1,
                    complete: true,
                    compound_from: None,
                    grid: grid.clone(),
                    cq,
                }));
            }
            // Compound announcement: queue it at the offset to resolve a placeholder.
            Frame::Compound { call, grid } => {
                let buf = self.buffer_for(rx, now_ms, i3.first);
                buf.compounds.push(Compound {
                    call: call.clone(),
                    grid: grid.clone(),
                });
                if i3.last {
                    if let Some(m) = self.try_close_offset(rx.freq_hz, rx.speed, now_ms, true) {
                        events.push(MessageEvent::Message(m));
                    }
                }
            }
            // Compound-directed: `to` = the compound call, `from` = the queued announcer.
            Frame::CompoundDirected { call, cmd, num } => {
                let buf = self.buffer_for(rx, now_ms, i3.first);
                buf.head = Some(Head {
                    from: "<....>".into(),
                    to: call.clone(),
                    cmd: Some(*cmd),
                    num: *num,
                    checksummed: cmd.is_checksummed(),
                });
                if i3.last || !cmd_is_buffered(*cmd) {
                    if let Some(m) = self.try_close_offset(rx.freq_hz, rx.speed, now_ms, i3.last) {
                        events.push(MessageEvent::Message(m));
                    }
                }
            }
            // Directed command.
            Frame::Directed {
                from,
                to,
                cmd,
                num,
                portable_from,
                portable_to,
            } => {
                let from_s = call_display(from, *portable_from);
                let to_s = call_display(to, *portable_to);
                let placeholder =
                    matches!(from, CallRef::Placeholder) || matches!(to, CallRef::Placeholder);
                let buffered = (cmd_is_buffered(*cmd) && !i3.last) || placeholder;
                if buffered {
                    let buf = self.buffer_for(rx, now_ms, i3.first);
                    buf.head = Some(Head {
                        from: from_s,
                        to: to_s,
                        cmd: Some(*cmd),
                        num: *num,
                        checksummed: cmd.is_checksummed(),
                    });
                    if i3.last {
                        if let Some(m) = self.try_close_offset(rx.freq_hz, rx.speed, now_ms, true) {
                            events.push(MessageEvent::Message(m));
                        }
                    }
                } else {
                    // single-frame directed command
                    events.push(MessageEvent::Message(Message {
                        from: from_s,
                        to: CallRef::parse(&to_s),
                        to_text: to_s,
                        cmd: Some(*cmd),
                        num: *num,
                        text: String::new(),
                        checksum: Checksum::NotRequired,
                        path: Vec::new(),
                        freq_hz: rx.freq_hz,
                        snr_db: rx.snr_db,
                        speed: rx.speed,
                        first_ms: now_ms,
                        last_ms: now_ms,
                        frames: 1,
                        complete: true,
                        compound_from: None,
                        grid: None,
                        cq: None,
                    }));
                }
            }
            // Data: append to an open buffer at the offset; a standalone data frame is activity only.
            Frame::Data { text, .. } => {
                if let Some(buf) = self.find_buffer(rx.freq_hz, rx.speed) {
                    buf.msgs.push(text.clone());
                    buf.last_ms = now_ms;
                    buf.frames = buf.frames.saturating_add(1);
                    if i3.last {
                        if let Some(m) = self.try_close_offset(rx.freq_hz, rx.speed, now_ms, true) {
                            events.push(MessageEvent::Message(m));
                        }
                    }
                }
            }
        }
        events
    }

    /// Age buffers: > 60 s past the newest frame force-closes as if Last; > 90 s drops
    /// `complete:false`. Call once per clock tick.
    pub fn age(&mut self, now_ms: u64) -> Vec<MessageEvent> {
        let mut events = Vec::new();
        let mut i = 0;
        while i < self.open.len() {
            let age = now_ms.saturating_sub(self.open[i].last_ms);
            // > 60 s force-closes a HEADED buffer (as if Last, but marked incomplete: its Last
            // never arrived); > 90 s drops ANY remaining buffer (a headless compound/data stub
            // that never resolved is removed — close_buffer returns None for it, so no message).
            if age > DROP_MS || (age > CLOSE_MS && self.open[i].head.is_some()) {
                let buf = self.open.remove(i);
                if let Some(m) = close_buffer(buf, now_ms, false) {
                    events.push(MessageEvent::Message(m));
                }
            } else {
                i += 1;
            }
        }
        events
    }

    pub fn clear(&mut self) {
        self.open.clear();
    }

    #[cfg(test)]
    fn open_len(&self) -> usize {
        self.open.len()
    }

    /// Find (or, clearing on `first`, create) the buffer for this frame's offset.
    fn buffer_for(&mut self, rx: &RawDecode, now_ms: u64, first: bool) -> &mut Buffer {
        if first {
            // A First frame clears any stale buffer at this offset.
            self.open.retain(|b| {
                !(b.speed == rx.speed && (b.offset - rx.freq_hz).abs() <= rx.speed.drift_hz())
            });
        }
        if let Some(idx) = self.buffer_index(rx.freq_hz, rx.speed) {
            let b = &mut self.open[idx];
            b.last_ms = now_ms;
            b.frames = b.frames.saturating_add(1);
            return &mut self.open[idx];
        }
        if self.open.len() >= MAX_OPEN_BUFFERS {
            // LRU eviction: abandon the least-recently-touched buffer incomplete, exactly as
            // `age` does past DROP_MS, so `open` can never grow without bound under a flood
            // of distinct-offset one-frame decodes.
            if let Some(lru) = (0..self.open.len()).min_by_key(|&i| self.open[i].last_ms) {
                self.open.remove(lru);
            }
        }
        self.open.push(Buffer {
            offset: rx.freq_hz,
            speed: rx.speed,
            head: None,
            compounds: Vec::new(),
            msgs: Vec::new(),
            first_ms: now_ms,
            last_ms: now_ms,
            frames: 1,
            freq_hz: rx.freq_hz,
            snr_db: rx.snr_db,
        });
        self.open.last_mut().unwrap()
    }

    fn buffer_index(&self, freq_hz: f32, speed: Speed) -> Option<usize> {
        self.open
            .iter()
            .position(|b| b.speed == speed && (b.offset - freq_hz).abs() <= speed.drift_hz())
    }

    fn find_buffer(&mut self, freq_hz: f32, speed: Speed) -> Option<&mut Buffer> {
        self.buffer_index(freq_hz, speed).map(|i| &mut self.open[i])
    }

    /// Close the buffer at an offset if it has a head and (last, or a single-frame command).
    fn try_close_offset(
        &mut self,
        freq_hz: f32,
        speed: Speed,
        now_ms: u64,
        last: bool,
    ) -> Option<Message> {
        let idx = self.buffer_index(freq_hz, speed)?;
        self.open[idx].head.as_ref()?;
        let buf = self.open.remove(idx);
        close_buffer(buf, now_ms, last)
    }
}

fn cmd_is_buffered(cmd: Command) -> bool {
    cmd.is_buffered() || cmd.text().contains(' ')
}

fn call_display(c: &CallRef, portable: bool) -> String {
    match (c, portable) {
        (CallRef::Base(b), true) => format!("{b}/P"),
        _ => c.render(),
    }
}

/// Resolve placeholders from queued compounds, assemble the body, verify the checksum, build the
/// closed `Message`. `complete` is `full_close` unless the checksum fails.
fn close_buffer(mut buf: Buffer, now_ms: u64, full_close: bool) -> Option<Message> {
    let head = buf.head.take()?;
    let mut from = head.from;
    let mut to = head.to;
    let mut compound_from = None;
    let mut grid = None;
    // Resolve `<....>` placeholders from the queued compounds (from first, then to). The
    // announcer that resolves `from` also carries that station's grid (a Heard fact).
    let mut compounds = buf.compounds.into_iter();
    if from == "<....>" {
        if let Some(c) = compounds.next() {
            from = c.call.clone();
            compound_from = Some(c.call);
            grid = c.grid;
        }
    }
    if to == "<....>" {
        if let Some(c) = compounds.next() {
            to = c.call;
        }
    }
    // An unresolved compound placeholder never reaches DIRECTED.TXT: upstream `continue`s past
    // it (mainwindow.cpp:8588) rather than logging a `<....>` line.
    if from == "<....>" || to == "<....>" {
        return None;
    }
    let mut complete = full_close;
    let body = buf.msgs.join("");
    let (text, checksum) = if body.is_empty() {
        (String::new(), Checksum::NotRequired)
    } else if head.checksummed {
        match verify_checksum3(&body) {
            Some(stripped) => (stripped.to_string(), Checksum::Ok),
            None => {
                complete = false;
                (body, Checksum::Bad)
            }
        }
    } else {
        (body, Checksum::NotRequired)
    };
    Some(Message {
        from,
        to: CallRef::parse(&to),
        to_text: to,
        cmd: head.cmd,
        num: head.num,
        text,
        checksum,
        path: Vec::new(),
        freq_hz: buf.freq_hz,
        snr_db: buf.snr_db,
        speed: buf.speed,
        first_ms: buf.first_ms,
        last_ms: now_ms.max(buf.last_ms),
        frames: buf.frames,
        complete,
        compound_from,
        grid,
        cq: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::compose::frames;
    use crate::proto::crc16::checksum3;
    use crate::proto::frame::encode_frame;

    fn raw(f: &Frame, i3: I3, speed: Speed, freq: f32) -> RawDecode {
        let word = encode_frame(f, i3, speed).unwrap();
        RawDecode {
            speed,
            freq_hz: freq,
            dt_s: 0.0,
            snr_db: -10,
            sync: 0.0,
            word,
            nharderrors: 0,
            quality: 1.0,
        }
    }

    fn messages(evs: Vec<MessageEvent>) -> Vec<Message> {
        evs.into_iter()
            .filter_map(|e| {
                if let MessageEvent::Message(m) = e {
                    Some(m)
                } else {
                    None
                }
            })
            .collect()
    }

    /// FINDING 3: `open` has a hard cap with LRU eviction, so a flood of distinct-offset
    /// one-frame decodes cannot grow it without bound (aging bounds it in practice; this is
    /// the ceiling). Feeding the open head of a multi-frame message at many distinct offsets
    /// keeps the count at `MAX_OPEN_BUFFERS`, and the oldest are the ones evicted.
    #[test]
    fn open_buffers_are_capped_with_lru_eviction() {
        let to = CallRef::Base("W1AW".into());
        let seq = frames(
            "KD9TAW",
            Some(&to),
            "A LONG MULTIFRAME MESSAGE FOR THE OPEN-BUFFER CAP TEST",
            Speed::Fast,
        )
        .unwrap();
        assert!(
            seq.len() > 1,
            "need a multi-frame message so the head stays open"
        );
        let mut r = Reassembler::new();
        for i in 0..(MAX_OPEN_BUFFERS + 20) {
            let freq = 200.0 + i as f32 * 30.0; // distinct offsets, well past drift
            r.feed(
                &raw(&seq[0].0, seq[0].1, Speed::Fast, freq),
                i as u64 * 1000,
            );
            assert!(
                r.open_len() <= MAX_OPEN_BUFFERS,
                "open buffers never exceed the cap (at i={i})"
            );
        }
        assert_eq!(
            r.open_len(),
            MAX_OPEN_BUFFERS,
            "the cap holds after the flood"
        );
    }

    #[test]
    fn render_directed_matches_the_directed_txt_shapes() {
        // faux heartbeat (grid not shown)
        let hb = Message {
            from: "KD2UWR".into(),
            to: CallRef::parse("@HB"),
            to_text: "@HB".into(),
            cmd: None,
            num: None,
            text: String::new(),
            checksum: Checksum::NotRequired,
            path: vec![],
            freq_hz: 645.0,
            snr_db: -11,
            speed: Speed::Normal,
            first_ms: 0,
            last_ms: 0,
            frames: 1,
            complete: true,
            compound_from: None,
            grid: Some("FN30".into()),
            cq: None,
        };
        assert!(hb.is_heartbeat());
        assert_eq!(render_directed(&hb), "KD2UWR: @HB HEARTBEAT");
        // CQ doubling
        let cq = Message {
            cq: Some(0),
            grid: Some("FN41".into()),
            to_text: "@ALLCALL".into(),
            to: CallRef::parse("@ALLCALL"),
            from: "KC1HZX".into(),
            cmd: None,
            num: None,
            text: String::new(),
            checksum: Checksum::NotRequired,
            path: vec![],
            freq_hz: 0.0,
            snr_db: 0,
            speed: Speed::Normal,
            first_ms: 0,
            last_ms: 0,
            frames: 1,
            complete: true,
            compound_from: None,
        };
        assert_eq!(
            render_directed(&cq),
            "KC1HZX: @ALLCALL CQ KC1HZX: @ALLCALL CQ CQ CQ FN41"
        );
        // directed SNR ack (extra) via a real frame through feed
        let f = Frame::Directed {
            from: CallRef::Base("NO1ZE".into()),
            to: CallRef::Base("KD2UWR".into()),
            cmd: Command::HeartbeatSnr,
            num: Some(7),
            portable_from: false,
            portable_to: false,
        };
        let mut r = Reassembler::new();
        let ms = messages(r.feed(
            &raw(
                &f,
                I3 {
                    first: true,
                    last: true,
                    data: false,
                },
                Speed::Normal,
                943.0,
            ),
            0,
        ));
        assert_eq!(ms.len(), 1);
        assert_eq!(render_directed(&ms[0]), "NO1ZE: KD2UWR HEARTBEAT SNR +07");
    }

    #[test]
    fn a_heartbeat_frame_emits_a_frame_then_a_faux_directed_message() {
        let hb = Frame::Heartbeat {
            call: "KD2UWR".into(),
            grid: Some("FN30".into()),
            is_cq: false,
            idx: 0,
        };
        let mut r = Reassembler::new();
        let evs = r.feed(
            &raw(
                &hb,
                I3 {
                    first: true,
                    last: true,
                    data: false,
                },
                Speed::Normal,
                645.0,
            ),
            1000,
        );
        assert!(matches!(evs[0], MessageEvent::Frame(_)));
        let m = &messages(evs)[0];
        assert!(m.is_heartbeat() && m.cq.is_none());
        assert_eq!(m.grid.as_deref(), Some("FN30"));
        assert_eq!(render_directed(m), "KD2UWR: @HB HEARTBEAT");
    }

    #[test]
    fn a_multiframe_msg_reassembles_its_body_and_strips_the_checksum() {
        // compose builds the MSG head + data frames with the trailing KERMIT checksum.
        let to = CallRef::Base("W1AW".into());
        let seq = frames("KD9TAW", Some(&to), "MSG HELLO WORLD", Speed::Fast).unwrap();
        let mut r = Reassembler::new();
        let mut closed = Vec::new();
        for (n, (f, i3)) in seq.iter().enumerate() {
            closed.extend(messages(
                r.feed(&raw(f, *i3, Speed::Fast, 1500.0), n as u64 * 6000),
            ));
        }
        assert_eq!(closed.len(), 1, "one directed message closes on Last");
        let m = &closed[0];
        assert_eq!(m.cmd, Some(Command::Msg));
        assert_eq!(
            m.text, "HELLO WORLD",
            "the 3-char checksum is stripped after it verifies"
        );
        assert_eq!(m.checksum, Checksum::Ok);
        assert!(m.complete && m.frames >= 2);
        assert_eq!(render_directed(m), "KD9TAW: W1AW MSG HELLO WORLD");
    }

    #[test]
    fn a_corrupted_checksum_closes_incomplete_never_as_confident_text() {
        // Build a MSG whose body checksum will not verify (append a wrong sum).
        let head = Frame::Directed {
            from: CallRef::Base("KD9TAW".into()),
            to: CallRef::Base("W1AW".into()),
            cmd: Command::Msg,
            num: None,
            portable_from: false,
            portable_to: false,
        };
        let bad_body = format!("HELLO {}", checksum3("GOODBYE")); // sum of a different string
        let data = Frame::Data {
            text: bad_body.clone(),
            dense: true,
        };
        let mut r = Reassembler::new();
        r.feed(
            &raw(
                &head,
                I3 {
                    first: true,
                    last: false,
                    data: false,
                },
                Speed::Fast,
                1500.0,
            ),
            0,
        );
        let closed = messages(r.feed(
            &raw(
                &data,
                I3 {
                    first: false,
                    last: true,
                    data: true,
                },
                Speed::Fast,
                1500.0,
            ),
            6000,
        ));
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].checksum, Checksum::Bad);
        assert!(
            !closed[0].complete,
            "a bad checksum is never presented as complete"
        );
    }

    #[test]
    fn first_clears_a_stale_buffer_and_last_closes() {
        let to = CallRef::Base("W1AW".into());
        let seq = frames("KD9TAW", Some(&to), "MSG ONE", Speed::Fast).unwrap();
        let mut r = Reassembler::new();
        // feed only the First (head) — buffer opens, nothing closes
        assert!(messages(r.feed(&raw(&seq[0].0, seq[0].1, Speed::Fast, 1500.0), 0)).is_empty());
        // a NEW First at the same offset clears the stale head (a different MSG)
        let seq2 = frames(
            "N0CALL",
            Some(&CallRef::Base("K1ABC".into())),
            "MSG TWO",
            Speed::Fast,
        )
        .unwrap();
        for (n, (f, i3)) in seq2.iter().enumerate() {
            messages(r.feed(&raw(f, *i3, Speed::Fast, 1502.0), 6000 + n as u64 * 6000));
        }
        // the stale first message never emerges; only the second closed
        let all = r.age(10_000_000);
        assert!(messages(all)
            .iter()
            .all(|m| m.from == "N0CALL" || m.text.contains("TWO")));
    }

    #[test]
    fn age_force_closes_at_60s_and_drops_incomplete_at_90s() {
        let head = Frame::Directed {
            from: CallRef::Base("KD9TAW".into()),
            to: CallRef::Base("W1AW".into()),
            cmd: Command::Msg,
            num: None,
            portable_from: false,
            portable_to: false,
        };
        // 60 s: a head with a partial body force-closes (as if Last).
        let mut r = Reassembler::new();
        r.feed(
            &raw(
                &head,
                I3 {
                    first: true,
                    last: false,
                    data: false,
                },
                Speed::Fast,
                1500.0,
            ),
            0,
        );
        r.feed(
            &raw(
                &Frame::Data {
                    text: "PARTIAL".into(),
                    dense: true,
                },
                I3 {
                    first: false,
                    last: false,
                    data: true,
                },
                Speed::Fast,
                1500.0,
            ),
            1000,
        );
        let closed = messages(r.age(70_000));
        assert_eq!(closed.len(), 1);
        assert!(!closed[0].complete, "a 60 s force-close is not complete");
        // 90 s with no new frames also yields a (dropped) message, never a silent loss.
        let mut r2 = Reassembler::new();
        r2.feed(
            &raw(
                &head,
                I3 {
                    first: true,
                    last: false,
                    data: false,
                },
                Speed::Fast,
                1500.0,
            ),
            0,
        );
        assert_eq!(messages(r2.age(100_000)).len(), 1);
    }

    #[test]
    fn a_compound_from_directed_resolves_the_placeholder() {
        // Compose a compound-from directed message: Compound(announcer) then Directed(<....> to W1AW).
        let seq = frames(
            "KD9TAW/QRP",
            Some(&CallRef::Base("W1AW".into())),
            "SNR?",
            Speed::Normal,
        )
        .unwrap();
        // seq = [Compound{KD9TAW/QRP}, CompoundDirected{W1AW, SnrQuery}] (compose CASE)
        let mut r = Reassembler::new();
        let mut closed = Vec::new();
        for (n, (f, i3)) in seq.iter().enumerate() {
            closed.extend(messages(
                r.feed(&raw(f, *i3, Speed::Normal, 1200.0), n as u64 * 15000),
            ));
        }
        assert_eq!(closed.len(), 1);
        assert_eq!(
            closed[0].from, "KD9TAW/QRP",
            "the <....> from resolved from the compound announcer"
        );
        assert_eq!(render_directed(&closed[0]), "KD9TAW/QRP: W1AW SNR?");
    }
}
