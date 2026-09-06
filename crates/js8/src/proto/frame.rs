//! JS8 frame codec — the 72 payload bits ⇄ `Frame`, and JS8Call's display line.
//!
//! CONTRACT. `Frame::unpack(payload, i3, speed)` is the receive side of JS8Call's
//! `DecodedText` strategy chain (decodedtext.h:64-70: FastData, Data, Heartbeat, Compound,
//! Directed — tried in that order until one accepts). `Frame::pack(speed)` is the transmit
//! side of `Varicode::pack*Message` / `packCompoundFrame` / `packDirectedMessage`
//! (varicode.cpp:1326-1683) and the two data packers (:1797-1921). `render()` is the string
//! JS8Call shows and writes to ALL.TXT (decodedtext.cpp `tryUnpack*` — trailing space
//! included), so Nexus's activity pane, ALL.TXT and UDP lines agree with JS8Call's byte for
//! byte. `decode_word` / `encode_frame` are the ONLY raw ⇄ Frame entries: decode verifies the
//! CRC-12 first, encode stamps it through `Word87::new`. Every layout below is a protocol FACT
//! read from varicode.cpp (cited per arm); no code is copied. The whole file is pinned by
//! `tests/golden_frames.rs` against 1333 frames JS8Call itself logged.
//!
//! WHY the payload never carries the i3 bits: JS8Call keeps First/Last/Data OUTSIDE the 72
//! bits (bits 72..=74 of the 87-bit word, phy::frame). The Data flag decides which unpacker
//! runs, so `unpack` needs `i3`; `pack` returns the Data flag it chose and the caller sets
//! First/Last (`encode_frame`).
//!
//! Speed only matters for Data frames: Normal still EMITS the deprecated `[1][compressed][70]`
//! form with i3.data clear (varicode.cpp:1852-1870 — "DEPRECATED in 2.2"), every other speed
//! the 72-bit JSC form with i3.data set (:1904-1918, JS8_FAST_DATA_CAN_USE_HUFF 0). Both are
//! ACCEPTED at every speed on receive; the flag, not the speed, selects the unpacker.
use crate::phy::{Payload72, Speed, Word87, I3};
use crate::proto::alphabet::CQS;
use crate::proto::callsign::{pack28, pack50, unpack28, unpack50, CallRef};
use crate::proto::command::{pack_cmd, pack_num, unpack_cmd, unpack_num, Command};
use crate::proto::grid::{pack_grid15, unpack_grid15, NBASEGRID, NMAXGRID, NUSERGRID};
use crate::proto::{huffman, jsc};

/// The 3-bit payload type (varicode.h `FrameType`); `1xx` is data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    Heartbeat,
    Compound,
    CompoundDirected,
    Directed,
    Data,
}

impl FrameType {
    pub fn from_type_bits(b: u8) -> FrameType {
        match b & 7 {
            0 => FrameType::Heartbeat,
            1 => FrameType::Compound,
            2 => FrameType::CompoundDirected,
            3 => FrameType::Directed,
            _ => FrameType::Data,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    /// 000 — `is_cq` = the isAlt bit; `idx` = CQS index when is_cq, else the (deprecated) HB
    /// status flags — every value renders "HEARTBEAT" (varicode.cpp:296-306).
    Heartbeat {
        call: String,
        grid: Option<String>,
        is_cq: bool,
        idx: u8,
    },
    /// 001 — the compound-callsign announcement (`\`CALL GRID` on the TX side).
    Compound { call: String, grid: Option<String> },
    /// 010 — compound-directed: `num ≥ NUSERGRID` carries `pack_cmd(cmd, num)`.
    CompoundDirected {
        call: String,
        cmd: Command,
        num: Option<i8>,
    },
    /// 011 — `[28 from][28 to][5 cmd][1 /P from][1 /P to][6 num]`.
    Directed {
        from: CallRef,
        to: CallRef,
        cmd: Command,
        num: Option<i8>,
        portable_from: bool,
        portable_to: bool,
    },
    /// 1xx / i3.data. `dense` = the 72-bit JSC fast-data form (i3.data set); false = the
    /// deprecated Normal-speed `[1][compressed][70]` Huffman-or-JSC form (i3.data clear).
    Data { text: String, dense: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    /// A callsign / group name this frame type cannot carry (the text names it).
    NotPackable(String),
    BadType,
    BadCall,
    BadGrid,
    TextTooLong,
    Crc,
}

/// Varicode::formatSNR (varicode.cpp:471-477): `+NN` / `-NN`, empty outside -60..=60.
pub fn format_snr(snr: i32) -> String {
    if !(-60..=60).contains(&snr) {
        return String::new();
    }
    if snr < 0 {
        format!("-{:02}", -snr)
    } else {
        format!("+{:02}", snr)
    }
}

// ---- bit helpers over the 72-bit payload (MSB-first, the Payload72::bits convention) ----

fn get(bits: &[u8; 72], pos: usize, n: usize) -> u64 {
    bits[pos..pos + n]
        .iter()
        .fold(0u64, |a, &b| (a << 1) | b as u64)
}

fn put(bits: &mut [u8; 72], pos: usize, n: usize, v: u64) {
    for k in 0..n {
        bits[pos + k] = ((v >> (n - 1 - k)) & 1) as u8;
    }
}

/// `[3 type][50 pack50(call)][11 num>>5][5 num&31][3 bits3]` (varicode.cpp:1469-1500).
fn pack_compound(call: &str, ftype: u8, num: u16, bits3: u8) -> Result<Payload72, FrameError> {
    let v = pack50(call).ok_or(FrameError::BadCall)?;
    if v == 0 {
        return Err(FrameError::BadCall); // upstream: `packed_callsign == 0` → no frame
    }
    let mut bits = [0u8; 72];
    put(&mut bits, 0, 3, ftype as u64);
    put(&mut bits, 3, 50, v);
    put(&mut bits, 53, 11, (num >> 5) as u64);
    put(&mut bits, 64, 5, (num & 31) as u64);
    put(&mut bits, 69, 3, (bits3 & 7) as u64);
    Ok(Payload72::from_bits(&bits))
}

/// Pad `prefix ++ bits` to 72 with one `0` then `1`s (varicode.cpp:1824-1832); unpad = last 0.
fn pad72(prefix: &[u8], bits: &[u8]) -> Payload72 {
    let mut out = [1u8; 72];
    let n = prefix.len() + bits.len();
    debug_assert!(
        n < 72,
        "callers keep strictly under the budget so a 0 pad bit always exists"
    );
    out[..prefix.len()].copy_from_slice(prefix);
    out[prefix.len()..n].copy_from_slice(bits);
    if n < 72 {
        out[n] = 0;
    }
    Payload72::from_bits(&out)
}

/// Pack as many leading chars of `text` as one data frame holds at `speed`; returns the
/// payload, the i3 Data flag it must fly with, and the chars consumed. `None` when nothing
/// fits (an empty text, or a text whose first char no coder accepts).
pub fn pack_data_prefix(text: &str, speed: Speed) -> Option<(Payload72, I3, usize)> {
    if speed.uses_fast_data() {
        // packFastDataMessage: JSC only, no prefix, full 72 bits, i3.data set (:1904-1918).
        let (bits, n) = jsc::compress(text, 72);
        if n == 0 {
            return None;
        }
        return Some((
            pad72(&[], &bits),
            I3 {
                first: false,
                last: false,
                data: true,
            },
            n,
        ));
    }
    // packDataMessage (deprecated, Normal only): [1][0]+Huffman vs [1][1]+JSC, whichever packs
    // MORE chars — Huffman must win strictly (:1855-1870). The budget is 72 minus the 2-bit prefix.
    let huff = huffman::pack(text, 70);
    let (jbits, jn) = jsc::compress(text, 70);
    match huff {
        Some((hb, hn)) if hn > jn => Some((
            pad72(&[1, 0], &hb),
            I3 {
                first: false,
                last: false,
                data: false,
            },
            hn,
        )),
        _ if jn > 0 => Some((
            pad72(&[1, 1], &jbits),
            I3 {
                first: false,
                last: false,
                data: false,
            },
            jn,
        )),
        _ => None,
    }
}

fn call_text(c: &CallRef, portable: bool) -> String {
    match (c, portable) {
        (CallRef::Base(b), true) => format!("{b}/P"),
        _ => c.render(),
    }
}

impl Frame {
    pub fn frame_type(&self) -> FrameType {
        match self {
            Frame::Heartbeat { .. } => FrameType::Heartbeat,
            Frame::Compound { .. } => FrameType::Compound,
            Frame::CompoundDirected { .. } => FrameType::CompoundDirected,
            Frame::Directed { .. } => FrameType::Directed,
            Frame::Data { .. } => FrameType::Data,
        }
    }

    /// Pack for `speed` (Data chooses the 70- vs 72-bit form by `speed.uses_fast_data()`).
    /// The returned `I3` carries ONLY the data bit; the caller sets first/last.
    pub fn pack(&self, speed: Speed) -> Result<(Payload72, I3), FrameError> {
        let no_data = I3 {
            first: false,
            last: false,
            data: false,
        };
        match self {
            Frame::Heartbeat {
                call,
                grid,
                is_cq,
                idx,
            } => {
                // packHeartbeatMessage (:1326-1370): grid or NMAXGRID, bit 15 = CQ, bits3 = CQ index.
                let mut num = match grid {
                    Some(g) => pack_grid15(g).ok_or(FrameError::BadGrid)?,
                    None => NMAXGRID,
                };
                if *is_cq {
                    num |= 1 << 15;
                }
                Ok((pack_compound(call, 0, num, *idx)?, no_data))
            }
            Frame::Compound { call, grid } => {
                let num = match grid {
                    Some(g) => pack_grid15(g).ok_or(FrameError::BadGrid)?,
                    None => NMAXGRID,
                };
                Ok((pack_compound(call, 1, num, 0)?, no_data))
            }
            Frame::CompoundDirected { call, cmd, num } => {
                // packCompoundMessage (:1416-1425): extra = nusergrid + packCmd(cmd, num).
                let extra = NUSERGRID + pack_cmd(*cmd, *num) as u16;
                Ok((pack_compound(call, 2, extra, 0)?, no_data))
            }
            Frame::Directed {
                from,
                to,
                cmd,
                num,
                portable_from,
                portable_to,
            } => {
                let f = pack28(from).ok_or(FrameError::BadCall)?;
                let t = pack28(to).ok_or(FrameError::BadCall)?;
                let mut bits = [0u8; 72];
                put(&mut bits, 0, 3, 3);
                put(&mut bits, 3, 28, f as u64);
                put(&mut bits, 31, 28, t as u64);
                put(&mut bits, 59, 5, (cmd.id() % 32) as u64);
                put(&mut bits, 64, 1, *portable_from as u64);
                put(&mut bits, 65, 1, *portable_to as u64);
                put(&mut bits, 66, 6, pack_num(*num) as u64);
                Ok((Payload72::from_bits(&bits), no_data))
            }
            Frame::Data { text, .. } => {
                let (payload, i3, n) =
                    pack_data_prefix(text, speed).ok_or(FrameError::TextTooLong)?;
                if n < text.chars().count() {
                    return Err(FrameError::TextTooLong);
                }
                Ok((payload, i3))
            }
        }
    }

    /// The DecodedText strategy chain: FastData (i3.data), Data (bit 0), then by type bits.
    pub fn unpack(payload: &Payload72, i3: I3, _speed: Speed) -> Result<Frame, FrameError> {
        let bits = payload.bits();
        if i3.data {
            // unpackFastDataMessage (:1937-1948): everything before the LAST 0 is JSC.
            let n = bits
                .iter()
                .rposition(|&b| b == 0)
                .ok_or(FrameError::BadType)?;
            let text = jsc::decompress(&bits[..n]);
            return if text.is_empty() {
                Err(FrameError::BadType)
            } else {
                Ok(Frame::Data { text, dense: true })
            };
        }
        if bits[0] == 1 {
            // unpackDataMessage (:1873-1901): [1][compressed][…][0][1…]; a data frame that
            // unpacks to nothing falls through to the other strategies, which all reject a
            // type ≥ 4 — so it is simply undecodable.
            let s = &bits[1..];
            let compressed = s[0] == 1;
            let n = s.iter().rposition(|&b| b == 0).ok_or(FrameError::BadType)?;
            let payload = if n >= 1 { &s[1..n] } else { &s[1..] };
            let text = if compressed {
                jsc::decompress(payload)
            } else {
                huffman::decode70(payload).unwrap_or_default()
            };
            return if text.is_empty() {
                Err(FrameError::BadType)
            } else {
                Ok(Frame::Data { text, dense: false })
            };
        }
        let ftype = get(&bits, 0, 3) as u8;
        match ftype {
            0..=2 => {
                // unpackCompoundFrame (:1502-1536)
                let call = unpack50(get(&bits, 3, 50));
                let num = ((get(&bits, 53, 11) as u16) << 5) | get(&bits, 64, 5) as u16;
                let bits3 = get(&bits, 69, 3) as u8;
                if call.is_empty() {
                    return Err(FrameError::BadCall);
                }
                match ftype {
                    0 => Ok(Frame::Heartbeat {
                        call,
                        grid: unpack_grid15(num & 0x7FFF),
                        is_cq: num & 0x8000 != 0,
                        idx: bits3,
                    }),
                    1 => Ok(Frame::Compound {
                        call,
                        grid: if num <= NBASEGRID {
                            unpack_grid15(num)
                        } else {
                            None
                        },
                    }),
                    _ => {
                        if (NUSERGRID..NMAXGRID).contains(&num) {
                            let (cmd, n) =
                                unpack_cmd((num - NUSERGRID) as u8).ok_or(FrameError::BadType)?;
                            Ok(Frame::CompoundDirected { call, cmd, num: n })
                        } else {
                            Err(FrameError::BadType)
                        }
                    }
                }
            }
            3 => {
                // unpackDirectedMessage (:1640-1683)
                let from = unpack28(get(&bits, 3, 28) as u32).ok_or(FrameError::BadCall)?;
                let to = unpack28(get(&bits, 31, 28) as u32).ok_or(FrameError::BadCall)?;
                let cmd = Command::from_id(get(&bits, 59, 5) as u8).ok_or(FrameError::BadType)?;
                Ok(Frame::Directed {
                    from,
                    to,
                    cmd,
                    num: unpack_num(get(&bits, 66, 6) as u8),
                    portable_from: bits[64] == 1,
                    portable_to: bits[65] == 1,
                })
            }
            _ => Err(FrameError::BadType),
        }
    }

    /// JS8Call's display line (decodedtext.cpp), byte-exact — the ALL.TXT text column.
    pub fn render(&self) -> String {
        match self {
            Frame::Heartbeat {
                call,
                grid,
                is_cq,
                idx,
            } => {
                let body = if *is_cq {
                    format!("@ALLCALL {}", CQS[(*idx & 7) as usize])
                } else {
                    "@HB HEARTBEAT".to_string()
                };
                format!("{call}: {body} {} ", grid.as_deref().unwrap_or(""))
            }
            Frame::Compound { call, .. } => format!("{call}: "),
            Frame::CompoundDirected { call, cmd, num } => {
                let snr = if cmd.carries_snr() {
                    format!(" {}", format_snr(num.unwrap_or(-31) as i32))
                } else {
                    String::new()
                };
                format!("{call}{}{snr} ", cmd.text())
            }
            Frame::Directed {
                from,
                to,
                cmd,
                num,
                portable_from,
                portable_to,
            } => {
                let extra = match num {
                    Some(n) if cmd.carries_snr() => format!(" {}", format_snr(*n as i32)),
                    Some(n) => format!(" {n}"),
                    None => String::new(),
                };
                format!(
                    "{}: {}{}{extra} ",
                    call_text(from, *portable_from),
                    call_text(to, *portable_to),
                    cmd.text()
                )
            }
            Frame::Data { text, .. } => text.clone(),
        }
    }
}

/// Verify the CRC-12, then unpack. The ONLY raw → Frame entry.
pub fn decode_word(w: &Word87, speed: Speed) -> Result<(Frame, I3), FrameError> {
    if !w.verify() {
        return Err(FrameError::Crc);
    }
    let i3 = w.i3();
    Ok((Frame::unpack(&w.payload72(), i3, speed)?, i3))
}

/// Pack, then stamp the CRC with first/last from `i3` and the data bit from the packer.
/// The ONLY Frame → raw entry.
pub fn encode_frame(f: &Frame, i3: I3, speed: Speed) -> Result<Word87, FrameError> {
    let (payload, data) = f.pack(speed)?;
    Ok(Word87::new(
        payload,
        I3 {
            first: i3.first,
            last: i3.last,
            data: data.data,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(s: &str) -> CallRef {
        CallRef::Base(s.to_string())
    }

    #[test]
    fn format_snr_matches_varicode_format_snr() {
        assert_eq!(format_snr(7), "+07");
        assert_eq!(format_snr(0), "+00");
        assert_eq!(format_snr(-5), "-05");
        assert_eq!(format_snr(-21), "-21");
        assert_eq!(format_snr(31), "+31");
        assert_eq!(format_snr(61), "");
        assert_eq!(format_snr(-61), "");
    }

    #[test]
    fn heartbeat_and_cq_render_like_js8call() {
        let hb = Frame::Heartbeat {
            call: "KD2UWR".into(),
            grid: Some("FN30".into()),
            is_cq: false,
            idx: 0,
        };
        assert_eq!(hb.render(), "KD2UWR: @HB HEARTBEAT FN30 ");
        let cq = Frame::Heartbeat {
            call: "W0IND".into(),
            grid: Some("EN52".into()),
            is_cq: true,
            idx: 0,
        };
        assert_eq!(cq.render(), "W0IND: @ALLCALL CQ CQ CQ EN52 ");
        let cq7 = Frame::Heartbeat {
            call: "W3BMD".into(),
            grid: None,
            is_cq: true,
            idx: 7,
        };
        assert_eq!(cq7.render(), "W3BMD: @ALLCALL CQ  ");
        for f in [&hb, &cq, &cq7] {
            let (p, i3) = f.pack(Speed::Normal).unwrap();
            assert!(!i3.data);
            assert_eq!(Frame::unpack(&p, i3, Speed::Normal).unwrap(), *f);
        }
    }

    #[test]
    fn directed_renders_with_the_command_text_and_num() {
        let f = Frame::Directed {
            from: base("NO1ZE"),
            to: base("KD2UWR"),
            cmd: Command::HeartbeatSnr,
            num: Some(7),
            portable_from: false,
            portable_to: false,
        };
        assert_eq!(f.render(), "NO1ZE: KD2UWR HEARTBEAT SNR +07 ");
        let q = Frame::Directed {
            from: base("W4WCA"),
            to: base("K0EMP"),
            cmd: Command::Relay,
            num: None,
            portable_from: false,
            portable_to: false,
        };
        assert_eq!(q.render(), "W4WCA: K0EMP> ");
        let ft = Frame::Directed {
            from: base("N9WH"),
            to: base("KO6HXN"),
            cmd: Command::Freetext,
            num: None,
            portable_from: false,
            portable_to: false,
        };
        assert_eq!(ft.render(), "N9WH: KO6HXN  ");
        let p = Frame::Directed {
            from: base("W1AW"),
            to: CallRef::AllCall,
            cmd: Command::SnrQuery,
            num: None,
            portable_from: true,
            portable_to: false,
        };
        assert_eq!(p.render(), "W1AW/P: @ALLCALL SNR? ");
        for f in [&f, &q, &ft, &p] {
            let (pl, i3) = f.pack(Speed::Fast).unwrap();
            assert_eq!(Frame::unpack(&pl, i3, Speed::Fast).unwrap(), *f);
        }
    }

    #[test]
    fn compound_and_compound_directed_render_like_js8call() {
        let c = Frame::Compound {
            call: "VA3MMU".into(),
            grid: Some("FN03".into()),
        };
        assert_eq!(
            c.render(),
            "VA3MMU: ",
            "the grid is NOT rendered (decodedtext.cpp tryUnpackCompound)"
        );
        let d = Frame::CompoundDirected {
            call: "KD4JRX/WFD".into(),
            cmd: Command::SnrQuery,
            num: None,
        };
        assert_eq!(d.render(), "KD4JRX/WFD SNR? ");
        let s = Frame::CompoundDirected {
            call: "N3CHX/P1".into(),
            cmd: Command::Snr,
            num: Some(-13),
        };
        assert_eq!(s.render(), "N3CHX/P1 SNR -13 ");
        for f in [&c, &d, &s] {
            let (pl, i3) = f.pack(Speed::Slow).unwrap();
            assert_eq!(Frame::unpack(&pl, i3, Speed::Slow).unwrap(), *f);
        }
    }

    #[test]
    fn data_picks_the_form_by_speed_and_unpacks_by_the_flag() {
        let d = Frame::Data {
            text: "TNX 73 GL".into(),
            dense: false,
        };
        let (p_norm, i3n) = d.pack(Speed::Normal).unwrap();
        assert!(!i3n.data, "Normal emits the deprecated 70-bit form");
        assert_eq!(p_norm.bits()[0], 1, "type bit 1xx");
        assert_eq!(
            Frame::unpack(&p_norm, i3n, Speed::Normal).unwrap(),
            Frame::Data {
                text: "TNX 73 GL".into(),
                dense: false
            }
        );
        let (p_fast, i3f) = d.pack(Speed::Fast).unwrap();
        assert!(i3f.data, "every other speed emits 72-bit dense data");
        assert_eq!(
            Frame::unpack(&p_fast, i3f, Speed::Turbo).unwrap(),
            Frame::Data {
                text: "TNX 73 GL".into(),
                dense: true
            }
        );
        // A 72-bit dense frame is accepted at Normal too (the flag, not the speed, selects).
        assert_eq!(
            Frame::unpack(&p_fast, i3f, Speed::Normal).unwrap(),
            Frame::Data {
                text: "TNX 73 GL".into(),
                dense: true
            }
        );
        // Too long for one frame → TextTooLong from pack; pack_data_prefix still takes a prefix.
        let long = Frame::Data {
            text: "THE QUICK BROWN FOX JUMPS OVER THE LAZY DOG AGAIN AND AGAIN".into(),
            dense: false,
        };
        assert_eq!(long.pack(Speed::Fast), Err(FrameError::TextTooLong));
        let (_, _, n) = pack_data_prefix(
            "THE QUICK BROWN FOX JUMPS OVER THE LAZY DOG AGAIN AND AGAIN",
            Speed::Fast,
        )
        .unwrap();
        assert!(n > 0 && n < 59);
    }

    #[test]
    fn a_frame_that_unpacks_to_nothing_is_an_error_not_an_empty_frame() {
        // i3.data with an all-ones payload (no pad zero) and a data frame whose bits decode to "".
        let ones = Payload72::from_bits(&[1u8; 72]);
        assert_eq!(
            Frame::unpack(
                &ones,
                I3 {
                    first: false,
                    last: false,
                    data: true
                },
                Speed::Fast
            ),
            Err(FrameError::BadType)
        );
        let mut b = [1u8; 72];
        b[2] = 0; // [1][1][0][1…]: compressed, empty payload
        assert_eq!(
            Frame::unpack(&Payload72::from_bits(&b), I3::default(), Speed::Normal),
            Err(FrameError::BadType)
        );
        // type 2 without a command range (grid-range num) is rejected, see the header
        let mut c = [0u8; 72];
        put(&mut c, 0, 3, 2);
        put(&mut c, 3, 50, pack50("W1AW").unwrap());
        assert_eq!(
            Frame::unpack(&Payload72::from_bits(&c), I3::default(), Speed::Normal),
            Err(FrameError::BadType)
        );
    }
}
