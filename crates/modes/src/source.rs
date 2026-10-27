//! The user-selectable [`SignalSource`] — the "native engine vs companion" switch.
//!
//! Per the product design, the operator chooses where decodes come from:
//! - [`NativeSource`] runs a [`Mode`] over locally captured audio (Nexus *is*
//!   the decoder), or
//! - [`WsjtxUdpSource`] consumes the decode stream of an upstream
//!   WSJT-X/JTDX/MSHV over UDP (Nexus is a companion).
//!
//! Both implement [`SignalSource`], so the engine drives whichever is active at
//! each slot boundary through one uniform call and the rest of the app is blind
//! to the choice.

use std::collections::VecDeque;
use std::net::{ToSocketAddrs, UdpSocket};

use crate::decode::Decode;
use crate::mode::{make_mode, Mode, ModeKind, PassCarry};

/// Inputs for one decode interval (slot). Native sources decode `iwave`; passive
/// sources (UDP) ignore the audio and drain queued network decodes.
pub struct DecodeRequest<'a> {
    /// Captured int16 audio @ 12 kHz (≥ the active mode's frame size).
    pub iwave: &'a [i16],
    /// Audio search band edges (Hz).
    pub nfa: i32,
    pub nfb: i32,
    /// Decode aggressiveness (≤ 0 ⇒ mode default).
    pub ndepth: i32,
    /// Callsigns for a-priori decoding (`""` if unknown).
    pub mycall: &'a str,
    pub hiscall: &'a str,
    /// QSO progress index (AP pass schedule).
    pub nqso_progress: i32,
    /// QSO/RX audio frequency (Hz) being worked — WSJT-X's nfqso; centers the
    /// deep AP passes + sync for FT8/FT4. 0 / out-of-band ⇒ band center.
    pub nfqso: i32,
    /// TX audio frequency (Hz) — WSJT-X's nftx (`mainwindow.cpp:3722`), i.e. the
    /// operator's transmit offset, which is independent of `nfqso` whenever
    /// "Hold Tx Freq" is on. It is the SECOND deep-AP window: `ft8b.f90:305`
    /// skips a candidate for the iaptype ≥ 3 masks only when it is outside BOTH
    /// `nfqso ± napwid` and `nftx ± napwid`, because the station answering your
    /// CQ usually answers on *your* transmit frequency. FT8 only. Equal to
    /// `nfqso` (or 0) ⇒ one window, which is exactly right when RX/TX coincide.
    pub nftx: i32,
    /// Monotonic ms timestamp for this frame (cross-frame IR-HARQ keying; FT1).
    /// 0 disables cross-frame combining.
    pub frame_time_ms: i64,
    /// A-priori decoding enabled (WSJT-X "Enable AP"). Consumed by FT8 only
    /// (ft8b's `lft8apon`; `false` also disables the a7 cross-cycle replay).
    /// The vendored FT4 decoder has no AP on/off flag (AP runs whenever
    /// `ndepth > 1`), so FT4 honestly ignores this. `true` is stock.
    pub ap: bool,
    /// Restrict AP to the CQ hypothesis (ft8b/ft4_decode `lapcqonly`,
    /// iaptype 1). Consumed by FT8 AND FT4. `false` is stock.
    pub ap_cq_only: bool,
    /// This is WSJT-X's EARLY decode over a partial, tail-zeroed frame
    /// (`nzhsym = 41`, `mainwindow.cpp:1878`) rather than the full-frame pass
    /// (`nzhsym = 50`, `mainwindow.cpp:1877`). Upstream runs the early pass
    /// deliberately cheap — sync floor 2.0 instead of 1.3
    /// (`ft8_decode.f90:178`) and the AP passes 5-8 off (`ft8b.f90:275`) —
    /// because the authoritative boundary pass re-decodes the same audio a
    /// couple of seconds later, and a slow early pass is what makes the
    /// boundary decode land late and cost the whole period. FT8 only.
    ///
    /// **Not the inverse of `decode_a7`'s `a7_final`.** Three passes exist, not
    /// two: Boundary is `a7_final`, Early is `partial`, and the F6 review
    /// re-decode is NEITHER — it runs over the retained FULL audio and must
    /// keep the deep passes it is invoked to get.
    pub partial: bool,
}

impl<'a> DecodeRequest<'a> {
    /// A plain full-band decode request over `iwave` with no AP / QSO context.
    pub fn full_band(iwave: &'a [i16]) -> Self {
        Self {
            iwave,
            nfa: 200,
            nfb: 2900,
            ndepth: 3,
            mycall: "",
            hiscall: "",
            nqso_progress: 0,
            nfqso: 0, // band center (no QSO freq)
            nftx: 0,  // no split — the deep-AP windows coincide with nfqso
            frame_time_ms: 0,
            ap: true,          // stock WSJT-X: AP on
            ap_cq_only: false, // stock WSJT-X: all AP hypotheses
            partial: false,    // full frame
        }
    }
}

/// Where a decoder reports while its pass runs, as WSJT-X's decoders call back with each new
/// decode (`ft8_decode.f90:233-238`, `ft4_decode.f90:456`) for jt9 to write and flush at once, so a
/// decode reaches its reader the moment it is found instead of when the pass ends.
pub trait DecodeSink {
    /// A decode the pass has just accepted. It is also in the list the pass returns, in the same
    /// place: the decodes come in the decoder's own order, the RX frequency's candidates first.
    fn found(&mut self, d: &Decode);
    /// Asked after each FT8 sync candidate is tried: `true` stops the pass there, with what it has
    /// found (WSJT-X asks its early pass's give-up at exactly this point, `ft8_decode.f90:240-242`).
    /// Never asked by FT4, which has no give-up.
    fn give_up(&mut self) -> bool {
        false
    }
    /// Asked before FT8's rescue pass starts and after each of its sync candidates (the full pass
    /// once more on the raw audio, after a full pass from the early pass's residual): `true` gives
    /// the rescue up there, keeping what it found; before it starts, the period's result is the
    /// full pass's alone.
    fn give_up_rescue(&mut self) -> bool {
        false
    }
}

/// A source of [`Decode`]s, driven once per slot boundary by the engine.
pub trait SignalSource: Send {
    /// Human-readable label for the UI (e.g. `"Native (FT8)"`, `"WSJT-X UDP"`).
    fn label(&self) -> String;

    /// Mode identity of this source's decodes, if known. `None` for a UDP source
    /// (it carries whatever the upstream app is running).
    fn mode_kind(&self) -> Option<ModeKind>;

    /// Produce the decodes available for this interval.
    fn decode(&mut self, req: &DecodeRequest) -> Vec<Decode>;

    /// [`decode`](SignalSource::decode) plus the a7 cross-cycle final-pass flag.
    ///
    /// `a7_final` is `true` on the authoritative full-audio (slot-boundary)
    /// pass — for a native FT8 source this saves the slot's decodes into the a7
    /// table and runs WSJT-X's cross-cycle replay (iaptype=7, keyed on
    /// `req.frame_time_ms`) — and `false` on the early partial pass. Passive
    /// sources and modes without a cross-cycle path ignore it; this default
    /// delegates to [`decode`](SignalSource::decode).
    fn decode_a7(&mut self, req: &DecodeRequest, a7_final: bool) -> Vec<Decode> {
        let _ = a7_final;
        self.decode(req)
    }

    /// [`decode_a7`](SignalSource::decode_a7), telling `sink` of each decode the moment the
    /// decoder finds it (a native FT8 or FT4 source). Every other source hands its decodes over
    /// only in the returned list; this default tells `sink` nothing.
    fn decode_streaming(
        &mut self,
        req: &DecodeRequest,
        a7_final: bool,
        sink: &mut dyn DecodeSink,
    ) -> Vec<Decode> {
        let _ = sink;
        self.decode_a7(req, a7_final)
    }

    /// WSJT-X's FT8 47 step for a native FT8 source ([`Mode::subtract_early`]): subtract this
    /// period's early decodes from `req.iwave`, the period's first 13.536 s, for the full pass that
    /// follows. Decodes nothing; `sink` is asked only whether to give up. Every other source has
    /// no such step and does nothing.
    fn subtract_early(&mut self, req: &DecodeRequest, sink: &mut dyn DecodeSink) -> usize {
        let _ = (req, sink);
        0
    }
}

/// Native decode: run the active [`Mode`] over locally captured audio.
pub struct NativeSource {
    mode: Box<dyn Mode>,
    /// What the mode keeps between the passes of one period ([`PassCarry`]): FT8's early decodes
    /// and the 47 step's residual. This decoder's own, so one radio chain's can never reach
    /// another's; a new mode starts a new one.
    carry: PassCarry,
}

impl NativeSource {
    /// Wrap an explicit boxed mode.
    pub fn new(mode: Box<dyn Mode>) -> Self {
        Self {
            mode,
            carry: PassCarry::default(),
        }
    }

    /// Build from a [`ModeKind`].
    pub fn from_kind(kind: ModeKind) -> Self {
        Self::new(make_mode(kind))
    }

    /// The active mode.
    pub fn mode(&self) -> &dyn Mode {
        self.mode.as_ref()
    }

    /// Switch modes at runtime (e.g. the user picks FT4 instead of FT8). Nothing the old mode
    /// carried between passes survives the switch.
    pub fn set_mode(&mut self, mode: Box<dyn Mode>) {
        self.mode = mode;
        self.carry = PassCarry::default();
    }

    /// What this decoder carries between the passes of the period ([`PassCarry`]).
    pub fn carry(&self) -> &PassCarry {
        &self.carry
    }
}

impl SignalSource for NativeSource {
    fn label(&self) -> String {
        format!("Native ({})", self.mode.name())
    }

    fn mode_kind(&self) -> Option<ModeKind> {
        Some(self.mode.kind())
    }

    fn decode(&mut self, req: &DecodeRequest) -> Vec<Decode> {
        let kind = self.mode.kind();
        let mut decs = self.mode.decode_frame(
            req.iwave,
            req.nfa,
            req.nfb,
            req.ndepth,
            req.mycall,
            req.hiscall,
            req.nqso_progress,
            req.nfqso,
            req.frame_time_ms,
            req.ap,
            req.ap_cq_only,
        );
        // Tag each decode with the mode that produced it (the conversion can't
        // know; we do).
        for d in &mut decs {
            d.mode = Some(kind);
        }
        decs
    }

    fn decode_a7(&mut self, req: &DecodeRequest, a7_final: bool) -> Vec<Decode> {
        let kind = self.mode.kind();
        let mut decs = self.mode.decode_frame_a7(
            req.iwave,
            req.nfa,
            req.nfb,
            req.ndepth,
            req.mycall,
            req.hiscall,
            req.nqso_progress,
            req.nfqso,
            req.nftx,
            req.frame_time_ms,
            a7_final,
            req.partial,
            req.ap,
            req.ap_cq_only,
        );
        for d in &mut decs {
            d.mode = Some(kind);
        }
        decs
    }

    fn decode_streaming(
        &mut self,
        req: &DecodeRequest,
        a7_final: bool,
        sink: &mut dyn DecodeSink,
    ) -> Vec<Decode> {
        /// Tags each decode with the mode that found it before `sink` sees it, as the returned
        /// list is tagged below.
        struct Tagged<'a> {
            kind: ModeKind,
            sink: &'a mut dyn DecodeSink,
        }
        impl DecodeSink for Tagged<'_> {
            fn found(&mut self, d: &Decode) {
                let mut d = d.clone();
                d.mode = Some(self.kind);
                self.sink.found(&d);
            }
            fn give_up(&mut self) -> bool {
                self.sink.give_up()
            }
            fn give_up_rescue(&mut self) -> bool {
                self.sink.give_up_rescue()
            }
        }
        let kind = self.mode.kind();
        // With this decoder's carry: FT8's full pass starts from the early pass's residual, as
        // WSJT-X's does ([`Mode::decode_frame_carry`]).
        let mut decs = self.mode.decode_frame_carry(
            req.iwave,
            req.nfa,
            req.nfb,
            req.ndepth,
            req.mycall,
            req.hiscall,
            req.nqso_progress,
            req.nfqso,
            req.nftx,
            req.frame_time_ms,
            a7_final,
            req.partial,
            req.ap,
            req.ap_cq_only,
            &mut self.carry,
            &mut Tagged { kind, sink },
        );
        for d in &mut decs {
            d.mode = Some(kind);
        }
        decs
    }

    fn subtract_early(&mut self, req: &DecodeRequest, sink: &mut dyn DecodeSink) -> usize {
        self.mode
            .subtract_early(req.iwave, req.ndepth, &mut self.carry, sink)
    }
}

/// Map an upstream WSJT-X/JTDX/MSHV `Decode` mode field to a [`ModeKind`].
///
/// WSJT-X reports the mode as a single-character code in the Decode message; some
/// apps send the full name instead, so both are accepted. The characters are the
/// ones the decoders actually print, read off WSJT-X 3.0.2's own `lib/decoder.f90`
/// output formats rather than from memory:
///
/// | code | mode | source |
/// |------|------|--------|
/// | `~` | FT8 | `ft8_decoded`, format 1001 |
/// | `+` | FT4 | `ft4_decoded`, format 1001 |
/// | `` ` `` | FST4 | `fst4_decoded`, format 1001 |
/// | `:` | Q65 | `q65_decoded`, format 1001 |
/// | `#` | JT65 | `jt65_decoded`, the `csync` field (`#`/`##`/`# `) |
/// | `&` `^` | MSK144 | `mainwindow.cpp:8931` accepts both |
///
/// ⚠️ **FT2 HAS NO CHARACTER HERE, AND CANNOT.** Decodium prints FT2 decodes with
/// `' + '` (`decoder.f90:1923`, `ft2_decoded`'s format 1001) — byte-identical to
/// FT4's marker, and `postDecode` (`mainwindow.cpp:16955`) puts that same token
/// straight into the UDP Decode message's mode field. So a companion `+` is
/// genuinely ambiguous on the wire, and mapping it to FT2 would mislabel every
/// FT4 decode from every WSJT-X in the world for the sake of the rarer mode. FT2
/// is matched by its FULL NAME only, which apps that send the name do supply;
/// a `+` from a Decodium station lands on FT4, and the tier label is the only
/// thing that is wrong.
///
/// ⭐ THE RETURNED PERIOD/SUBMODE ARE PLACEHOLDERS, and that is sound only because
/// of what reads this. The single character does not carry a T/R period — a `:` is
/// Q65 at any of 5 periods and 5 submodes. The one consumer is the decode row's
/// tier label via `Tier::from_mode_kind`, which collapses every `Q65 { .. }` to
/// `Tier::Q65` and discards the parameters. Nothing here sizes a buffer or a slot
/// clock: these decodes arrive already decoded from the upstream app. If a caller
/// ever needs the real period off a companion decode, it has to come from the
/// upstream Status message, not from this character.
fn wsjtx_mode_to_kind(m: &str) -> Option<ModeKind> {
    match m.trim() {
        "~" => Some(ModeKind::Ft8),
        "+" => Some(ModeKind::Ft4),
        "`" => Some(ModeKind::Fst4 {
            period_s: 60,
            wspr: false,
        }),
        ":" => Some(ModeKind::Q65 {
            period_s: 60,
            submode: 0,
        }),
        "#" | "##" => Some(ModeKind::Jt65 { submode: 0 }),
        "&" | "^" => Some(ModeKind::Msk144 { period_s: 15 }),
        other if other.eq_ignore_ascii_case("FT8") => Some(ModeKind::Ft8),
        other if other.eq_ignore_ascii_case("FT4") => Some(ModeKind::Ft4),
        // Name only — see the ⚠️ above on why `+` is not mapped.
        other if other.eq_ignore_ascii_case("FT2") => Some(ModeKind::Ft2),
        // JS8Call's UDP API is a different (JSON) protocol; this only lets a WSJT-X-style
        // status line that says "JS8" label its rows truly. Normal is the representative.
        other if other.eq_ignore_ascii_case("JS8") => Some(ModeKind::JS8_NORMAL),
        other if other.eq_ignore_ascii_case("Q65") => Some(ModeKind::Q65 {
            period_s: 60,
            submode: 0,
        }),
        other if other.eq_ignore_ascii_case("JT65") => Some(ModeKind::Jt65 { submode: 0 }),
        other if other.eq_ignore_ascii_case("MSK144") => Some(ModeKind::Msk144 { period_s: 15 }),
        other if other.eq_ignore_ascii_case("FST4") => Some(ModeKind::Fst4 {
            period_s: 60,
            wspr: false,
        }),
        other if other.eq_ignore_ascii_case("FST4W") => Some(ModeKind::Fst4 {
            period_s: 120,
            wspr: true,
        }),
        other if other.eq_ignore_ascii_case("WSPR") => Some(ModeKind::Wspr),
        _ => None,
    }
}

/// Companion decode: ingest the decode stream of an upstream WSJT-X/JTDX/MSHV
/// over UDP. Inbound datagrams are parsed with
/// [`tempo_net::wsjtx::parse_inbound`]; `Decode` messages are mapped to the
/// unified [`Decode`] and queued, then drained each interval.
///
/// Either [`bind`](WsjtxUdpSource::bind) a non-blocking UDP socket (the real
/// path), or feed raw datagrams via
/// [`ingest_datagram`](WsjtxUdpSource::ingest_datagram) (tests / an external
/// receive loop).
pub struct WsjtxUdpSource {
    socket: Option<UdpSocket>,
    queue: VecDeque<Decode>,
}

impl WsjtxUdpSource {
    /// A queue-only source (no socket); feed it via [`ingest_datagram`].
    pub fn new() -> Self {
        Self {
            socket: None,
            queue: VecDeque::new(),
        }
    }

    /// Bind a non-blocking UDP socket on `addr` (e.g. `"127.0.0.1:2237"`), the
    /// default sink WSJT-X/JTDX/MSHV transmit their telemetry to.
    pub fn bind(addr: impl ToSocketAddrs) -> std::io::Result<Self> {
        let socket = UdpSocket::bind(addr)?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket: Some(socket),
            queue: VecDeque::new(),
        })
    }

    /// The bound local address, if a socket is bound (diagnostics / tests).
    pub fn local_addr(&self) -> Option<std::net::SocketAddr> {
        self.socket.as_ref().and_then(|s| s.local_addr().ok())
    }

    /// Parse one inbound datagram; if it is a WSJT-X `Decode`, map it to the
    /// unified [`Decode`] and queue it. Returns `true` if a decode was queued.
    pub fn ingest_datagram(&mut self, bytes: &[u8]) -> bool {
        match tempo_net::wsjtx::parse_inbound(bytes) {
            Some(tempo_net::wsjtx::Inbound::Decode {
                snr,
                delta_time,
                delta_freq,
                mode,
                message,
                low_confidence,
                ..
            }) => {
                self.queue.push_back(Decode {
                    message,
                    sync: 0.0,
                    snr,
                    // The upstream `Decode` reports dt already in the WSJT-X
                    // `xdt = t - 0.5` convention and freq as the audio offset.
                    dt: delta_time as f32,
                    freq: delta_freq as f32,
                    nap: 0,
                    qual: if low_confidence { 0.0 } else { 1.0 },
                    rv: None,
                    // Carry the upstream app's mode so the feed labels it truly,
                    // not as our selected tier.
                    mode: wsjtx_mode_to_kind(&mode),
                    // A companion (UDP) row is never a native JS8 decode: no Word87 crosses
                    // the wire, so the typed carrier stays None. Only `Js8Mode` sets it.
                    raw: None,
                });
                true
            }
            _ => false,
        }
    }

    /// Drain all datagrams currently pending on the bound socket into the queue.
    fn drain_socket(&mut self) {
        // Collect first (immutable socket borrow), then parse (mutable self).
        let mut packets: Vec<Vec<u8>> = Vec::new();
        if let Some(sock) = &self.socket {
            let mut buf = [0u8; 4096];
            loop {
                match sock.recv_from(&mut buf) {
                    Ok((n, _)) => packets.push(buf[..n].to_vec()),
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => break,
                }
            }
        }
        for p in packets {
            self.ingest_datagram(&p);
        }
    }
}

impl Default for WsjtxUdpSource {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalSource for WsjtxUdpSource {
    fn label(&self) -> String {
        "WSJT-X UDP".to_string()
    }

    fn mode_kind(&self) -> Option<ModeKind> {
        None
    }

    fn decode(&mut self, _req: &DecodeRequest) -> Vec<Decode> {
        // The audio in `_req` is irrelevant: decodes come from the network.
        self.drain_socket();
        self.queue.drain(..).collect()
    }
}
