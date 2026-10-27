//! What Stop's second road carries, as both ends read and write it: [`Up`] from the computer
//! driving the shack, [`Down`] back, each one datagram ([`crate::datagram`]) of at most 128 bytes.
//! And the state echo's clock
//! ([`StateClock`]): when the shack took each snapshot it numbered on one connection, as the command
//! link's writer records it ([`crate::outbox`]) and the shack's road judges a heartbeat by. What the
//! shack takes, refuses and answers is its road's (src-tauri's `remote_service::lan::stop_road`);
//! this computer's end is [`crate::stop_road`].
//!
//! ## The format is frozen
//!
//! Every message's kind and body are fixed as 1.20.0 sends and reads them, recorded byte for byte in
//! the tests below, and they are not part of the build's contract (`crate::wire::contract`): a
//! computer of another build, a phone among them, can rely on them across versions. A message's
//! body is read only at its exact length, so a change to any of them is a new datagram version
//! (`datagram::VERSION`), never a new reading of these bytes.
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tempo_base::mic::{MicEnded, MicStatus};
use tempo_base::ptt_hold::HoldEnd;

/// How long a road's keys outlive its connection, for a Stop sent as the link drops or after it.
pub const AFTERLIFE: Duration = Duration::from_secs(60);
/// How old the state a heartbeat names may be and still renew presence: the stream's two seconds
/// for a picture, measured the same way, by the station's own clock.
pub const FRESH: Duration = Duration::from_millis(tempo_stream::protocol::FRESH_FRAME_MS);
/// Snapshots remembered per connection: about nineteen seconds of them, against two.
const REMEMBERED: usize = 64;

// --- What it carries -----------------------------------------------------------------------------

const STOP: u8 = 1;
const HOLD: u8 = 2;
const RELEASE: u8 = 3;
const HEARTBEAT: u8 = 4;
const STOPPED: u8 = 16;
const PRESS: u8 = 17;
const MIC: u8 = 18;
const PRESENCE: u8 = 19;

/// What a computer sends on its road.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Up {
    /// A Stop press, repeated until it is acknowledged: the same number each time, its computer's
    /// count of Stops pressed this run, itself included (the count the command link carries).
    Stop { stop: u64 },
    /// The press `press` is still held: the screen re-asserts it every 100 ms. `stops` is how many
    /// Stops its computer had pressed when the press began, the same at every hold.
    Hold { press: u64, stops: u64 },
    /// The press `press` was let go.
    Release { press: u64 },
    /// The computer's lease, and the number of the last snapshot its screen fetched.
    Heartbeat { lease: String, state: u64 },
}

/// What the shack sends back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Down {
    /// The Stop `stop` was taken.
    Stopped { stop: u64 },
    /// What became of a press: keyed, or ended and why.
    Press {
        press: u64,
        keyed: bool,
        end: Option<HoldEnd>,
    },
    /// The microphone over, as the engine last published it.
    Mic(MicStatus),
    /// Presence was renewed: this long until it lapses unless renewed again.
    Presence { left_ms: u32 },
}

impl Up {
    pub fn kind(&self) -> u8 {
        match self {
            Up::Stop { .. } => STOP,
            Up::Hold { .. } => HOLD,
            Up::Release { .. } => RELEASE,
            Up::Heartbeat { .. } => HEARTBEAT,
        }
    }

    pub fn body(&self) -> Vec<u8> {
        match self {
            Up::Stop { stop: n } | Up::Release { press: n } => n.to_be_bytes().to_vec(),
            Up::Hold { press, stops } => {
                let mut body = press.to_be_bytes().to_vec();
                body.extend_from_slice(&stops.to_be_bytes());
                body
            }
            Up::Heartbeat { lease, state } => {
                let mut body = lease.as_bytes().to_vec();
                body.extend_from_slice(&state.to_be_bytes());
                body
            }
        }
    }

    /// A body that verified, read exactly: each kind has one length, and a lease is one the
    /// station could have issued.
    pub fn read(kind: u8, body: &[u8]) -> Option<Self> {
        let number = || body.try_into().ok().map(u64::from_be_bytes);
        Some(match kind {
            STOP => Up::Stop { stop: number()? },
            HOLD if body.len() == 16 => Up::Hold {
                press: u64::from_be_bytes(body[..8].try_into().ok()?),
                stops: u64::from_be_bytes(body[8..].try_into().ok()?),
            },
            RELEASE => Up::Release { press: number()? },
            HEARTBEAT if body.len() == 36 + 8 => {
                let lease = std::str::from_utf8(&body[..36]).ok()?;
                if !tempo_stream::protocol::identifier(lease) {
                    return None;
                }
                Up::Heartbeat {
                    lease: lease.to_string(),
                    state: u64::from_be_bytes(body[36..].try_into().ok()?),
                }
            }
            _ => return None,
        })
    }
}

fn hold_end(end: Option<HoldEnd>) -> u8 {
    match end {
        None => 0,
        Some(HoldEnd::Refused) => 1,
        Some(HoldEnd::Lapsed) => 2,
        Some(HoldEnd::Released) => 3,
        Some(HoldEnd::Stopped) => 4,
    }
}

fn hold_end_of(code: u8) -> Option<Option<HoldEnd>> {
    Some(match code {
        0 => None,
        1 => Some(HoldEnd::Refused),
        2 => Some(HoldEnd::Lapsed),
        3 => Some(HoldEnd::Released),
        4 => Some(HoldEnd::Stopped),
        _ => return None,
    })
}

fn mic_ended(ended: Option<MicEnded>) -> u8 {
    match ended {
        None => 0,
        Some(MicEnded::Released) => 1,
        Some(MicEnded::Stopped) => 2,
        Some(MicEnded::AudioGap) => 3,
        Some(MicEnded::Presence) => 4,
        Some(MicEnded::Ceiling) => 5,
        Some(MicEnded::Watchdog) => 6,
        Some(MicEnded::RouteChanged) => 7,
    }
}

fn mic_ended_of(code: u8) -> Option<Option<MicEnded>> {
    Some(match code {
        0 => None,
        1 => Some(MicEnded::Released),
        2 => Some(MicEnded::Stopped),
        3 => Some(MicEnded::AudioGap),
        4 => Some(MicEnded::Presence),
        5 => Some(MicEnded::Ceiling),
        6 => Some(MicEnded::Watchdog),
        7 => Some(MicEnded::RouteChanged),
        _ => return None,
    })
}

fn flag(byte: u8) -> Option<bool> {
    match byte {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

impl Down {
    pub fn kind(&self) -> u8 {
        match self {
            Down::Stopped { .. } => STOPPED,
            Down::Press { .. } => PRESS,
            Down::Mic(_) => MIC,
            Down::Presence { .. } => PRESENCE,
        }
    }

    pub fn body(&self) -> Vec<u8> {
        match self {
            Down::Stopped { stop } => stop.to_be_bytes().to_vec(),
            Down::Press { press, keyed, end } => {
                let mut body = press.to_be_bytes().to_vec();
                body.extend_from_slice(&[u8::from(*keyed), hold_end(*end)]);
                body
            }
            Down::Mic(status) => {
                let mut body = vec![
                    u8::from(status.armed),
                    u8::from(status.keyed),
                    u8::from(status.no_power_out),
                    mic_ended(status.ended),
                ];
                body.extend_from_slice(&status.holes.to_be_bytes());
                body.extend_from_slice(&status.hole_ms.to_be_bytes());
                body
            }
            Down::Presence { left_ms } => left_ms.to_be_bytes().to_vec(),
        }
    }

    /// A body that verified, read exactly.
    pub fn read(kind: u8, body: &[u8]) -> Option<Self> {
        Some(match (kind, body.len()) {
            (STOPPED, 8) => Down::Stopped {
                stop: u64::from_be_bytes(body.try_into().ok()?),
            },
            (PRESS, 10) => Down::Press {
                press: u64::from_be_bytes(body[..8].try_into().ok()?),
                keyed: flag(body[8])?,
                end: hold_end_of(body[9])?,
            },
            (MIC, 12) => Down::Mic(MicStatus {
                armed: flag(body[0])?,
                keyed: flag(body[1])?,
                no_power_out: flag(body[2])?,
                ended: mic_ended_of(body[3])?,
                holes: u32::from_be_bytes(body[4..8].try_into().ok()?),
                hole_ms: u32::from_be_bytes(body[8..12].try_into().ok()?),
            }),
            (PRESENCE, 4) => Down::Presence {
                left_ms: u32::from_be_bytes(body.try_into().ok()?),
            },
            _ => return None,
        })
    }
}

// --- The state echo's clock ----------------------------------------------------------------------

/// When the shack took each snapshot it numbered on one connection: the state echo's clock, as the
/// stream's video clock is the picture's (`tempo_stream::frame_clock`). The command link records
/// each number as it gives it ([`crate::outbox`]), with the moment the shack began reading that
/// state, so a state read slowly reads as older, never newer.
///
/// ⛔ AN ECHO MUST NAME A SNAPSHOT THE SHACK SENT. A number it never gave, or one it has forgotten,
/// is not fresh whatever its value, so a computer cannot keep presence alive by naming a number it
/// guessed, and a garbled one reads as stale.
#[derive(Default)]
pub struct StateClock {
    taken: Mutex<VecDeque<(u64, Instant)>>,
}

impl StateClock {
    /// The snapshot numbered `number` was taken at `taken`.
    pub fn sent(&self, number: u64, taken: Instant) {
        let mut sent = self
            .taken
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if sent.len() >= REMEMBERED {
            sent.pop_front();
        }
        sent.push_back((number, taken));
    }

    /// Is the snapshot numbered `number` one this connection sent, taken no more than [`FRESH`]
    /// before `now`? Asked by the road's thread, which only tries the lock: a moment's contention
    /// reads as not fresh, and the next heartbeat asks again.
    pub fn fresh(&self, number: u64, now: Instant) -> bool {
        self.freshness(number, now).unwrap_or(false)
    }

    /// [`StateClock::fresh`], or `None` while the lock is held this moment: the road's thread never
    /// waits, and a press it cannot judge now is asked about again at its next hold, never refused.
    pub fn freshness(&self, number: u64, now: Instant) -> Option<bool> {
        match self.taken.try_lock() {
            Ok(sent) => Some(fresh_in(&sent, number, now)),
            Err(std::sync::TryLockError::Poisoned(poisoned)) => {
                Some(fresh_in(&poisoned.into_inner(), number, now))
            }
            Err(std::sync::TryLockError::WouldBlock) => None,
        }
    }

    /// [`StateClock::fresh`], waiting for the lock: for the command lane, which may wait.
    pub fn fresh_waiting(&self, number: u64, now: Instant) -> bool {
        let sent = self
            .taken
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        fresh_in(&sent, number, now)
    }
}

fn fresh_in(sent: &VecDeque<(u64, Instant)>, number: u64, now: Instant) -> bool {
    sent.iter()
        .rev()
        .find(|(sent, _)| *sent == number)
        .is_some_and(|(_, taken)| now.saturating_duration_since(*taken) <= FRESH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datagram::{End, Keys, EXPORTED};

    fn bytes(hex: &str) -> Vec<u8> {
        let hex: String = hex.split_whitespace().collect();
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }

    const LEASE: &str = "0f1e2d3c-4b5a-4968-8778-a6b5c4d3e2f1";

    /// Each message from the computer, its kind and its body as 1.20.0 writes them.
    fn ups() -> Vec<(Up, u8, Vec<u8>)> {
        vec![
            (
                Up::Stop {
                    stop: 0x0102_0304_0506_0708,
                },
                1,
                bytes("0102030405060708"),
            ),
            (
                Up::Hold { press: 7, stops: 3 },
                2,
                bytes("0000000000000007 0000000000000003"),
            ),
            (Up::Release { press: 9 }, 3, bytes("0000000000000009")),
            (
                Up::Heartbeat {
                    lease: LEASE.to_string(),
                    state: 42,
                },
                4,
                [LEASE.as_bytes(), &bytes("000000000000002a")].concat(),
            ),
        ]
    }

    /// Each message from the shack, its kind and its body as 1.20.0 writes them: every end of a
    /// press and every end of a microphone over, each by its own code.
    fn downs() -> Vec<(Down, u8, Vec<u8>)> {
        let mut downs = vec![
            (Down::Stopped { stop: 5 }, 16, bytes("0000000000000005")),
            (
                Down::Press {
                    press: 7,
                    keyed: true,
                    end: None,
                },
                17,
                bytes("0000000000000007 01 00"),
            ),
            (Down::Presence { left_ms: 5_000 }, 19, bytes("00001388")),
        ];
        for (end, code) in [
            (HoldEnd::Refused, "01"),
            (HoldEnd::Lapsed, "02"),
            (HoldEnd::Released, "03"),
            (HoldEnd::Stopped, "04"),
        ] {
            downs.push((
                Down::Press {
                    press: 8,
                    keyed: false,
                    end: Some(end),
                },
                17,
                bytes(&format!("0000000000000008 00 {code}")),
            ));
        }
        for (ended, code) in [
            (None, "00"),
            (Some(MicEnded::Released), "01"),
            (Some(MicEnded::Stopped), "02"),
            (Some(MicEnded::AudioGap), "03"),
            (Some(MicEnded::Presence), "04"),
            (Some(MicEnded::Ceiling), "05"),
            (Some(MicEnded::Watchdog), "06"),
            (Some(MicEnded::RouteChanged), "07"),
        ] {
            downs.push((
                Down::Mic(MicStatus {
                    armed: true,
                    keyed: false,
                    no_power_out: true,
                    ended,
                    holes: 2,
                    hole_ms: 0x0102_0304,
                }),
                18,
                bytes(&format!("01 00 01 {code} 00000002 01020304")),
            ));
        }
        downs
    }

    /// ★ The format is frozen: every message either way is written as the bytes 1.20.0 writes,
    /// recorded here, and those bytes read back as the same message.
    /// CONTROL: the tables hold every kind of message the road carries, both ways.
    #[test]
    fn every_message_is_the_bytes_1_20_0_writes_and_reads() {
        for (up, kind, body) in ups() {
            assert_eq!(up.kind(), kind, "{up:?}");
            assert_eq!(up.body(), body, "{up:?}");
            assert_eq!(Up::read(kind, &body), Some(up));
        }
        for (down, kind, body) in downs() {
            assert_eq!(down.kind(), kind, "{down:?}");
            assert_eq!(down.body(), body, "{down:?}");
            assert_eq!(Down::read(kind, &body), Some(down));
        }
        let kinds = |mut kinds: Vec<u8>| {
            kinds.sort_unstable();
            kinds.dedup();
            kinds
        };
        assert_eq!(kinds(ups().iter().map(|u| u.1).collect()), [1, 2, 3, 4]);
        assert_eq!(
            kinds(downs().iter().map(|d| d.1).collect()),
            [16, 17, 18, 19]
        );
    }

    /// ★ A whole datagram is frozen too: a Stop sealed by a computer, and a press's report sealed
    /// by the shack, are the bytes 1.20.0 seals from the same export, tag and sequence (recorded
    /// from an independent AES-128-GCM over the layout in `crate::datagram`), and each opens at the
    /// other end as the message it carries.
    #[test]
    fn a_sealed_stop_and_its_answer_are_the_bytes_1_20_0_seals() {
        let exported: [u8; EXPORTED] = std::array::from_fn(|n| n as u8);
        let tag = [1, 2, 3, 4, 5, 6, 7, 8];
        let mut computer = Keys::new(&exported, tag, End::Computer).unwrap();
        let mut shack = Keys::new(&exported, tag, End::Shack).unwrap();

        let stop = Up::Stop { stop: 3 };
        let sealed = computer.send.seal(stop.kind(), &stop.body()).unwrap();
        assert_eq!(
            sealed,
            bytes(
                "01 01 0102030405060708 0000000000000001 \
                 20d9732fa30a27a6 fb964146f8989de4 095d0b801bbe4acf"
            )
        );
        let (kind, body) = shack.receive.open(&sealed).unwrap();
        assert_eq!(Up::read(kind, &body), Some(stop));

        let press = Down::Press {
            press: 7,
            keyed: true,
            end: None,
        };
        let sealed = shack.send.seal(press.kind(), &press.body()).unwrap();
        assert_eq!(
            sealed,
            bytes(
                "01 11 0102030405060708 0000000000000001 \
                 b45341df9a47c43edb66 8b44e8dd92245e756aeb3c7ed76bc517"
            )
        );
        let (kind, body) = computer.receive.open(&sealed).unwrap();
        assert_eq!(Down::read(kind, &body), Some(press));
    }

    /// ★ A body that verified but is not one the format writes reads as nothing: each kind cut
    /// short and run long by a byte, every kind the road does not take (a direction's kinds read
    /// the other way among them), a flag or an end code outside the format, and a heartbeat whose
    /// lease the station could not have issued. CONTROL: the untouched bodies read (above).
    #[test]
    fn a_body_the_format_does_not_write_reads_as_nothing() {
        for (up, kind, body) in ups() {
            assert_eq!(Up::read(kind, &body[..body.len() - 1]), None, "{up:?}");
            assert_eq!(Up::read(kind, &[&body[..], &[0]].concat()), None, "{up:?}");
            assert_eq!(Down::read(kind, &body), None, "{up:?} read as the shack's");
        }
        for (down, kind, body) in downs() {
            assert_eq!(Down::read(kind, &body[..body.len() - 1]), None, "{down:?}");
            assert_eq!(
                Down::read(kind, &[&body[..], &[0]].concat()),
                None,
                "{down:?}"
            );
            assert_eq!(Up::read(kind, &body), None, "{down:?} read as a computer's");
        }
        for kind in (0..=u8::MAX).filter(|k| ![1, 2, 3, 4, 16, 17, 18, 19].contains(k)) {
            for len in [0, 4, 8, 10, 12, 16, 44] {
                assert_eq!(Up::read(kind, &vec![0; len]), None, "up kind {kind}");
                assert_eq!(Down::read(kind, &vec![0; len]), None, "down kind {kind}");
            }
        }
        assert_eq!(
            Down::read(17, &bytes("0000000000000007 02 00")),
            None,
            "keyed 2"
        );
        assert_eq!(
            Down::read(17, &bytes("0000000000000007 00 05")),
            None,
            "end 5"
        );
        assert_eq!(
            Down::read(18, &bytes("02 00 00 00 00000000 00000000")),
            None,
            "armed 2"
        );
        assert_eq!(
            Down::read(18, &bytes("00 00 00 08 00000000 00000000")),
            None,
            "ended 8"
        );
        let state = bytes("000000000000002a");
        for lease in [
            LEASE.to_uppercase(),
            LEASE.replace('-', "x"),
            "z".repeat(36),
        ] {
            assert_eq!(
                Up::read(4, &[lease.as_bytes(), &state].concat()),
                None,
                "{lease}"
            );
        }
        let mut not_text = [LEASE.as_bytes(), &state].concat();
        not_text[0] = 0xff;
        assert_eq!(Up::read(4, &not_text), None, "a lease that is not text");
    }
}
