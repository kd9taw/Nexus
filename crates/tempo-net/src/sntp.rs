//! Minimal SNTP (RFC 4330) client — a single UDP query to estimate the local
//! clock's offset from UTC. `std`-only, keeping `tempo-net` dependency-free.
//!
//! FT1/DX1 are slot-timed to UTC, so an accurate PC clock is essential.
//! [`query`] returns the **local-clock-minus-UTC** offset in milliseconds
//! (positive = the PC clock is ahead / fast). It is best-effort: any network or
//! parse failure is an `Err`, which callers should treat as "unknown / offline".
//!
//! ⚠️ **[`query`] and [`query_any`] are RAW SAMPLES and must never steer the
//! transmitter on their own.** A single UDP packet from a single server is one
//! unvalidated reading: a misconfigured or hijacked responder, a captive portal
//! that answers anything, or a stratum-16 server still warming up will all hand
//! back a number that looks exactly like a good one. Until 2026-09 the radio
//! loop's UTC steering was fed by `query_any` — the FIRST server that answered,
//! held for 600 s — so one bad packet moved every TX key and decode window.
//! [`measure`] is the steering entry point: it asks every server, requires
//! [`MIN_AGREEING`] of them to land inside [`AGREE_WINDOW_MS`] of each other,
//! and returns the **median** of the agreeing cluster. Fewer than that is
//! `None`, which the caller must treat as "no new measurement", never as zero.

use std::net::{ToSocketAddrs, UdpSocket};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Seconds between the NTP epoch (1900-01-01) and the Unix epoch (1970-01-01).
const NTP_UNIX_OFFSET: f64 = 2_208_988_800.0;
/// 2^32, for converting the NTP fractional-seconds field.
const TWO_POW_32: f64 = 4_294_967_296.0;

/// Query one NTP server (`host`, e.g. `"pool.ntp.org:123"`) and return the local
/// clock's offset from UTC in milliseconds (positive = local clock is ahead).
pub fn query(host: &str, timeout: Duration) -> std::io::Result<i64> {
    let addr = host
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no NTP address"))?;
    let sock = UdpSocket::bind("0.0.0.0:0")?;
    sock.set_read_timeout(Some(timeout))?;
    sock.set_write_timeout(Some(timeout))?;

    // Client request: LI=0, VN=4, Mode=3 (client) in byte 0; the rest zeroed.
    let mut req = [0u8; 48];
    req[0] = 0x23;

    let t1 = unix_now();
    sock.send_to(&req, addr)?;
    let mut resp = [0u8; 48];
    let n = sock.recv(&mut resp)?;
    let t4 = unix_now();
    if n < 48 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "short NTP reply",
        ));
    }
    // Server receive (T2, bytes 32..40) and transmit (T3, bytes 40..48) stamps.
    let t2 = ntp_to_unix(&resp[32..40]);
    let t3 = ntp_to_unix(&resp[40..48]);
    if t3 <= 0.0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid NTP timestamp",
        ));
    }
    // NTP offset = ((T2−T1)+(T3−T4))/2 = UTC − local; report local − UTC.
    let offset_secs = ((t2 - t1) + (t3 - t4)) / 2.0;
    Ok((-offset_secs * 1000.0).round() as i64)
}

/// Try several servers in order, returning the first success.
///
/// ⚠️ One packet, one server, no corroboration — see the module header. Fine for
/// a display hint; **never** for anything that positions a transmission.
pub fn query_any(hosts: &[&str], timeout: Duration) -> std::io::Result<i64> {
    let mut last = std::io::Error::other("no NTP servers");
    for h in hosts {
        match query(h, timeout) {
            Ok(ms) => return Ok(ms),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// How many servers must agree before a round is believed (guard 1).
///
/// Two is the smallest number that can *corroborate* anything: with one sample
/// there is nothing to disagree with, so a wrong answer is indistinguishable
/// from a right one. It is deliberately not three — a station with one reachable
/// server behind a restrictive firewall would then never get a measurement at
/// all, and B2's expiring hold is the layer that covers a thin quorum.
pub const MIN_AGREEING: usize = 2;

/// Full width of the agreement window, in ms (guard 1).
///
/// Two samples further apart than this are not describing the same clock. 200 ms
/// is well inside TempoFast's −0.30 s cliff (`crates/modes/src/mode.rs`), so a
/// cluster that passes cannot itself contribute a slot-fatal error, and it is
/// wide enough to absorb ordinary internet path asymmetry between two public
/// servers (tens of ms) without splitting an honest pair.
pub const AGREE_WINDOW_MS: i64 = 200;

/// One completed measurement round — the corroborated offset and how it was reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Measurement {
    /// Agreed local−UTC offset in ms (the MEDIAN of the agreeing cluster;
    /// positive = the PC clock is ahead of UTC).
    pub offset_ms: i64,
    /// How many servers landed inside the agreement window.
    pub agreeing: u8,
    /// How many servers answered at all (≥ `agreeing`).
    pub answered: u8,
}

/// Query **every** `host` and return the corroborated offset, or `None`.
///
/// This is the entry point anything that steers the radio must use. Guard 1: at
/// least [`MIN_AGREEING`] servers must land inside [`AGREE_WINDOW_MS`] of each
/// other; the answer is the median of that cluster, so a single wild reading
/// is outvoted rather than averaged in. `None` means "no measurement this
/// round" — the caller holds its previous value (with an age) rather than
/// falling back to zero.
///
/// Queries run sequentially, so a fully unreachable list costs
/// `hosts.len() * timeout`. It runs on the background probe thread, never the
/// audio loop.
pub fn measure(hosts: &[&str], timeout: Duration) -> Option<Measurement> {
    let mut samples: Vec<i64> = Vec::with_capacity(hosts.len());
    for h in hosts {
        if let Ok(ms) = query(h, timeout) {
            samples.push(ms);
        }
    }
    let answered = samples.len() as u8;
    agree(&mut samples).map(|(offset_ms, agreeing)| Measurement {
        offset_ms,
        agreeing,
        answered,
    })
}

/// The agreement kernel of [`measure`], split out so it is testable without a
/// socket: the largest cluster of samples spanning ≤ [`AGREE_WINDOW_MS`], and
/// its median. `None` when no such cluster reaches [`MIN_AGREEING`].
///
/// Sorts in place, then slides a window: for each start `i`, the cluster runs to
/// the last `j` with `s[j] − s[i] ≤ window`. Widest wins; ties keep the earliest
/// (lowest-offset) cluster, which is arbitrary but deterministic.
fn agree(samples: &mut [i64]) -> Option<(i64, u8)> {
    samples.sort_unstable();
    let (mut best_i, mut best_len) = (0usize, 0usize);
    for i in 0..samples.len() {
        let mut j = i;
        while j + 1 < samples.len() && samples[j + 1] - samples[i] <= AGREE_WINDOW_MS {
            j += 1;
        }
        if j - i + 1 > best_len {
            best_len = j - i + 1;
            best_i = i;
        }
    }
    if best_len < MIN_AGREEING {
        return None;
    }
    let cluster = &samples[best_i..best_i + best_len];
    Some((median(cluster), best_len as u8))
}

/// Median of a sorted slice; the mean of the two middle values when even.
/// (`i64` midpoint via `i128` so a hostile pair cannot overflow the add.)
fn median(sorted: &[i64]) -> i64 {
    let n = sorted.len();
    if n % 2 == 1 {
        sorted[n / 2]
    } else {
        ((sorted[n / 2 - 1] as i128 + sorted[n / 2] as i128) / 2) as i64
    }
}

fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Parse an 8-byte NTP timestamp (u32 seconds + u32 fraction, big-endian) into
/// Unix seconds.
fn ntp_to_unix(b: &[u8]) -> f64 {
    let secs = u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f64;
    let frac = u32::from_be_bytes([b[4], b[5], b[6], b[7]]) as f64 / TWO_POW_32;
    secs + frac - NTP_UNIX_OFFSET
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ntp_timestamp_parses_to_unix() {
        // NTP seconds for 2024-01-01T00:00:00Z = 3_913_056_000 → Unix 1_704_067_200.
        let secs: u32 = 3_913_056_000;
        let mut b = [0u8; 8];
        b[..4].copy_from_slice(&secs.to_be_bytes());
        let unix = ntp_to_unix(&b);
        assert!((unix - 1_704_067_200.0).abs() < 1.0, "got {unix}");
    }

    #[test]
    fn fraction_field_is_half_a_second() {
        let mut b = [0u8; 8];
        b[..4].copy_from_slice(&(NTP_UNIX_OFFSET as u32).to_be_bytes()); // unix 0
        b[4] = 0x80; // top bit of the fraction = 0.5 s
        let unix = ntp_to_unix(&b);
        assert!((unix - 0.5).abs() < 1e-6, "got {unix}");
    }

    // ── Guard 1: never act on one sample ──────────────────────────────────────
    //
    // A throwaway SNTP responder on 127.0.0.1, the shape `tests/rigctld_dummy.rs`
    // uses for CAT: bind an ephemeral port, hand the caller its address, answer
    // forever on a detached thread. `skew_ms` is what the server PRETENDS the
    // truth is: it answers with `now − skew`, so `query` reports `+skew` (the
    // local clock is that far AHEAD of this server's UTC).

    /// Spawn a mock NTP server answering with a clock `skew_ms` behind ours.
    /// Returns its `127.0.0.1:PORT` address.
    fn mock_ntp(skew_ms: i64) -> String {
        let sock = UdpSocket::bind("127.0.0.1:0").expect("bind mock NTP");
        let addr = sock.local_addr().expect("mock NTP addr").to_string();
        std::thread::spawn(move || {
            let mut req = [0u8; 48];
            while let Ok((_, from)) = sock.recv_from(&mut req) {
                let t = unix_now() - skew_ms as f64 / 1000.0;
                let mut resp = [0u8; 48];
                resp[0] = 0x24; // LI=0, VN=4, Mode=4 (server)
                resp[1] = 1; // stratum 1
                let stamp = unix_to_ntp(t);
                resp[32..40].copy_from_slice(&stamp); // T2 (receive)
                resp[40..48].copy_from_slice(&stamp); // T3 (transmit)
                if sock.send_to(&resp, from).is_err() {
                    break;
                }
            }
        });
        addr
    }

    /// Inverse of [`ntp_to_unix`], for the mock server only.
    fn unix_to_ntp(unix: f64) -> [u8; 8] {
        let ntp = unix + NTP_UNIX_OFFSET;
        let secs = ntp.trunc() as u32;
        let frac = ((ntp - ntp.trunc()) * TWO_POW_32) as u32;
        let mut b = [0u8; 8];
        b[..4].copy_from_slice(&secs.to_be_bytes());
        b[4..].copy_from_slice(&frac.to_be_bytes());
        b
    }

    /// Loopback round-trips are sub-millisecond, but a loaded test runner can
    /// still put tens of ms between `t1` and `t4`. Every assertion below allows
    /// that much slop and no more — tighter would flake, looser would stop
    /// distinguishing the clusters these tests are about.
    const SLOP_MS: i64 = 60;

    fn timeout() -> Duration {
        Duration::from_millis(500)
    }

    #[test]
    fn three_agreeing_servers_give_their_median() {
        let a = mock_ntp(1000);
        let b = mock_ntp(1080);
        let c = mock_ntp(1150);
        let hosts = [a.as_str(), b.as_str(), c.as_str()];
        let m = measure(&hosts, timeout()).expect("three agreeing servers publish");
        assert_eq!(m.answered, 3, "all three answered");
        assert_eq!(m.agreeing, 3, "all three inside the 200 ms window");
        assert!(
            (m.offset_ms - 1080).abs() <= SLOP_MS,
            "median of 1000/1080/1150 is 1080, got {}",
            m.offset_ms
        );
    }

    #[test]
    fn a_lone_outlier_is_excluded_from_the_median() {
        // Two honest servers and one 9-second liar — the shipped `query_any`
        // would have returned whichever answered first, liar included.
        let a = mock_ntp(1000);
        let b = mock_ntp(1100);
        let liar = mock_ntp(10_000);
        let hosts = [liar.as_str(), a.as_str(), b.as_str()];
        let m = measure(&hosts, timeout()).expect("two agreeing servers publish");
        assert_eq!(m.answered, 3, "all three answered");
        assert_eq!(m.agreeing, 2, "only the honest pair agreed");
        assert!(
            (m.offset_ms - 1050).abs() <= SLOP_MS,
            "median of the agreeing pair is 1050, got {} (the liar leaked in)",
            m.offset_ms
        );
    }

    /// ⚠️ THE POSITIVE CONTROL. Three servers that disagree with each other must
    /// publish NOTHING. If this test ever passes with a `Some(_)`, the agreement
    /// check is decorative and guard 1 is not holding the transmitter up.
    #[test]
    fn three_disagreeing_servers_publish_nothing() {
        let a = mock_ntp(0);
        let b = mock_ntp(5_000);
        let c = mock_ntp(11_000);
        let hosts = [a.as_str(), b.as_str(), c.as_str()];
        assert_eq!(
            measure(&hosts, timeout()),
            None,
            "no two of 0 / 5 s / 11 s are within 200 ms — nothing may be published"
        );
    }

    /// The leap-smear case, which is why one smearing server may stay on the
    /// probe's list (`service.rs`'s `CLOCK_SERVERS`). Google says not to mix
    /// smearing and non-smearing servers; guard 1's agreeing-cluster median is
    /// what makes mixing safe, PROVIDED the smearers are a minority — during a
    /// smear the two non-smearing servers form the cluster and the smeared
    /// sample is outvoted rather than averaged in.
    #[test]
    fn a_leap_smearing_server_in_the_minority_is_outvoted() {
        let straight_a = mock_ntp(0);
        let straight_b = mock_ntp(40);
        let smeared = mock_ntp(500); // mid-smear, half a leap second out
        let hosts = [straight_a.as_str(), smeared.as_str(), straight_b.as_str()];
        let m = measure(&hosts, timeout()).expect("the honest pair still publishes");
        assert_eq!(m.agreeing, 2, "the smeared sample is not in the cluster");
        assert!(
            m.offset_ms.abs() <= SLOP_MS + 20,
            "the published offset must be the straight pair's, got {}",
            m.offset_ms
        );
    }

    #[test]
    fn a_single_answering_server_publishes_nothing() {
        // One live server, two dead ports: one sample cannot corroborate itself.
        let only = mock_ntp(1000);
        let dead1 = dead_port();
        let dead2 = dead_port();
        let hosts = [only.as_str(), dead1.as_str(), dead2.as_str()];
        assert_eq!(
            measure(&hosts, timeout()),
            None,
            "one sample is not a quorum"
        );
    }

    #[test]
    fn no_answering_servers_publish_nothing() {
        let d1 = dead_port();
        let d2 = dead_port();
        let hosts = [d1.as_str(), d2.as_str()];
        assert_eq!(measure(&hosts, timeout()), None, "offline means unknown");
    }

    /// A `127.0.0.1` address nothing is listening on: bind, read the port, drop
    /// the socket. (Racy in principle; the port is not reused in these tests.)
    fn dead_port() -> String {
        let s = UdpSocket::bind("127.0.0.1:0").expect("bind");
        s.local_addr().expect("addr").to_string()
    }

    // The kernel, without sockets — the cases the mock cannot make deterministic.

    #[test]
    fn agree_takes_the_widest_cluster_not_the_first() {
        // A tight pair at the bottom and a tighter TRIPLE above it.
        let mut s = vec![0, 50, 5_000, 5_100, 5_180];
        assert_eq!(agree(&mut s), Some((5_100, 3)));
    }

    #[test]
    fn agree_window_is_inclusive_at_exactly_200ms() {
        let mut s = vec![0, AGREE_WINDOW_MS];
        assert_eq!(agree(&mut s), Some((100, 2)), "200 ms apart still agrees");
        let mut s = vec![0, AGREE_WINDOW_MS + 1];
        assert_eq!(agree(&mut s), None, "201 ms apart does not");
    }

    #[test]
    fn median_of_an_even_cluster_is_the_midpoint() {
        assert_eq!(median(&[10, 20]), 15);
        assert_eq!(median(&[10, 20, 30]), 20);
        // The midpoint is computed in i128, so extremes cannot overflow.
        assert_eq!(median(&[i64::MIN, i64::MAX]), 0);
    }
}
