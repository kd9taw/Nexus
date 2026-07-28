//! Beacon transmit scheduling for WSPR and FST4W.
//!
//! These modes are **not QSO modes**. There is no exchange to sequence and no
//! station to answer: the operator picks a fraction of intervals to transmit on,
//! and the rest are spent listening. That decision belongs here rather than in the
//! auto-sequencer, which would otherwise try to work the beacons it hears.
//!
//! # Why a percentage at all
//! A beacon that transmits every interval hears nothing, and WSPR's value is the
//! two-way picture — your reports of others matter as much as theirs of you. The
//! convention is to transmit a minority of intervals.
//!
//! Both schedulers here are ports of WSJT-X 3.0.2, cited inline. They are pure
//! functions of their own state so they can be tested without a clock or a radio.

/// The beacon message format, `"CALL GRID DBM"`.
///
/// Re-exported from the `wspr` crate so callers reach one definition: WSPR and
/// FST4W carry the SAME payload shape, differing only in the code that carries it.
pub use wspr::message;

/// WSJT-X's transmit-percentage scheduler, from `WSPR/WSPRBandHopping.cpp:255`:
///
/// ```cpp
/// bool WSPRBandHopping::impl::simple_scheduler ()
/// {
///   auto tx = carry_ || tx_percent_ > dist_ (gen_);
///   if (carry_) { carry_ = false; }
///   else if (tx_percent_ < 40 && last_was_tx_ && tx) {
///     // if percentage is less than 40 then avoid consecutive tx but
///     // always catch up on the next round
///     tx = false;
///     carry_ = true;
///   }
///   last_was_tx_ = tx;
///   return tx;
/// }
/// ```
///
/// ⭐ THE `carry_` RULE IS THE SUBTLE PART, and dropping it would look harmless.
/// Below 40 % it suppresses back-to-back transmissions — two in a row from a
/// low-duty beacon wastes the listening window that makes the mode useful — but it
/// sets `carry_` so the suppressed transmission is taken on the NEXT interval
/// unconditionally. Without the carry the achieved duty cycle would sag well below
/// what the operator asked for; without the suppression a 20 % beacon would
/// occasionally key twice in four minutes. It is a rate-preserving spread, not a
/// simple coin flip.
#[derive(Debug, Clone)]
pub struct BeaconScheduler {
    /// Percentage of intervals to transmit on, 0..=100.
    tx_percent: u8,
    /// A transmission was suppressed by the anti-consecutive rule and is owed.
    carry: bool,
    /// Whether the previous interval transmitted.
    last_was_tx: bool,
    /// `dist_(gen_)` — a uniform draw in 0..=99. Upstream uses `std::mt19937`; the
    /// distribution matters, the generator does not, so this is a small LCG that
    /// keeps the crate dependency-free and the tests deterministic.
    rng: u32,
}

impl BeaconScheduler {
    /// `tx_percent` is clamped to 0..=100. `seed` fixes the draw sequence.
    pub fn new(tx_percent: u8, seed: u32) -> Self {
        Self {
            tx_percent: tx_percent.min(100),
            carry: false,
            last_was_tx: false,
            // Never zero: a zero state would make the LCG emit a constant.
            rng: seed | 1,
        }
    }

    /// The operator's requested duty cycle.
    pub fn tx_percent(&self) -> u8 {
        self.tx_percent
    }

    /// Change the duty cycle, preserving the carry/last state so an adjustment
    /// mid-session does not reset the spread.
    pub fn set_tx_percent(&mut self, pct: u8) {
        self.tx_percent = pct.min(100);
    }

    /// Uniform draw in 0..=99, standing in for upstream's `dist_(gen_)`.
    fn draw(&mut self) -> u8 {
        self.rng = self.rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        // High bits: an LCG's low bits have short periods.
        ((self.rng >> 16) % 100) as u8
    }

    /// Decide whether to transmit on the next interval.
    ///
    /// 100 % always transmits and 0 % never does — both short-circuit before the
    /// draw, matching `next_is_tx`'s own `if (100 == tx_percent_) return true`.
    pub fn next_is_tx(&mut self) -> bool {
        if self.tx_percent >= 100 {
            self.last_was_tx = true;
            return true;
        }
        if self.tx_percent == 0 {
            self.last_was_tx = false;
            return false;
        }
        let mut tx = self.carry || self.tx_percent > self.draw();
        if self.carry {
            self.carry = false;
        } else if self.tx_percent < 40 && self.last_was_tx && tx {
            tx = false;
            self.carry = true;
        }
        self.last_was_tx = tx;
        tx
    }
}

/// FST4W's deterministic **Round Robin** slot assignment, from
/// `mainwindow.cpp:13713`:
///
/// ```cpp
/// qint64 ms = QDateTime::currentMSecsSinceEpoch() % 86400000;
/// int nsec=ms/1000;
/// int ntr=m_TRperiod;
/// int j=((nsec+ntr-1) % (n*ntr))/ntr;
/// m_WSPR_tx_next = i == j;
/// ```
///
/// Where `i` is this station's slot (0-based) and `n` the number of slots.
///
/// ⭐ THE POINT IS COLLISION AVOIDANCE, not duty cycle. Several stations agreeing
/// on `n` and each taking a different `i` will never transmit in the same interval,
/// because the assignment is a pure function of UTC — no coordination at run time,
/// no drift. That is why this is separate from [`BeaconScheduler`], which is random
/// and would occasionally collide.
///
/// `utc_secs_of_day` is seconds since UTC midnight; `period_s` the T/R period.
/// Returns false for a nonsensical configuration rather than transmitting.
pub fn fst4w_round_robin_is_tx(utc_secs_of_day: u32, period_s: u16, slot: u8, slots: u8) -> bool {
    if slots == 0 || period_s == 0 || slot >= slots {
        return false;
    }
    let ntr = u32::from(period_s);
    let n = u32::from(slots);
    // The `+ntr-1` biases the boundary so an interval is claimed by the slot that
    // OWNS it rather than the one that just ended — keep it, or every station
    // transmits one interval late.
    let j = ((utc_secs_of_day + ntr - 1) % (n * ntr)) / ntr;
    j == u32::from(slot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hundred_percent_always_transmits_and_zero_never_does() {
        let mut always = BeaconScheduler::new(100, 1);
        let mut never = BeaconScheduler::new(0, 1);
        for _ in 0..50 {
            assert!(always.next_is_tx());
            assert!(!never.next_is_tx());
        }
    }

    #[test]
    fn the_achieved_duty_cycle_tracks_the_requested_one() {
        // The property that actually matters to an operator: ask for 20 % and get
        // roughly 20 % of intervals over a session, not 12 % because the
        // anti-consecutive rule silently ate transmissions.
        for pct in [10u8, 20, 30, 50, 80] {
            let mut s = BeaconScheduler::new(pct, 0xBEEF);
            let n = 4000;
            let hits = (0..n).filter(|_| s.next_is_tx()).count();
            let achieved = 100.0 * hits as f64 / n as f64;
            assert!(
                (achieved - f64::from(pct)).abs() < 4.0,
                "asked {pct}%, achieved {achieved:.1}% — the carry rule is not \
                 preserving the rate"
            );
        }
    }

    #[test]
    fn below_forty_percent_consecutive_transmissions_are_suppressed() {
        // Upstream's rule, and the reason `carry` exists. A low-duty beacon must
        // not key twice in a row: the listening interval is the point.
        let mut s = BeaconScheduler::new(20, 12345);
        let mut prev = false;
        for _ in 0..4000 {
            let tx = s.next_is_tx();
            assert!(!(tx && prev), "two consecutive transmissions below 40%");
            prev = tx;
        }
    }

    #[test]
    fn at_or_above_forty_percent_consecutive_transmissions_are_allowed() {
        // The threshold is real: above it, back-to-back overs are how the rate is
        // met at all. If this ever stopped happening, high percentages could not be
        // achieved and the previous test would be passing for the wrong reason.
        let mut s = BeaconScheduler::new(80, 999);
        let mut consecutive = false;
        let mut prev = false;
        for _ in 0..500 {
            let tx = s.next_is_tx();
            if tx && prev {
                consecutive = true;
                break;
            }
            prev = tx;
        }
        assert!(consecutive, "80% must be able to transmit twice in a row");
    }

    #[test]
    fn a_suppressed_transmission_is_taken_on_the_next_interval() {
        // The carry, pinned directly rather than inferred from the rate: once the
        // anti-consecutive rule fires, the very next interval transmits
        // unconditionally.
        let mut s = BeaconScheduler::new(30, 7);
        let mut prev = false;
        for _ in 0..2000 {
            let carry_before = s.carry;
            let tx = s.next_is_tx();
            if carry_before {
                assert!(tx, "an owed transmission must be taken immediately");
            }
            prev = tx;
        }
        let _ = prev;
    }

    #[test]
    fn round_robin_gives_every_slot_exactly_one_interval_in_the_cycle() {
        // The collision-avoidance property: across n consecutive intervals, each of
        // the n stations transmits exactly once and never at the same time.
        let period = 120u16;
        for slots in [2u8, 3, 4, 5] {
            let mut seen = vec![0usize; usize::from(slots)];
            for step in 0..u32::from(slots) {
                let t = step * u32::from(period);
                let firing: Vec<u8> = (0..slots)
                    .filter(|&i| fst4w_round_robin_is_tx(t, period, i, slots))
                    .collect();
                assert_eq!(
                    firing.len(),
                    1,
                    "exactly one station transmits per interval, got {firing:?}"
                );
                seen[usize::from(firing[0])] += 1;
            }
            assert!(
                seen.iter().all(|&c| c == 1),
                "every slot transmits exactly once per cycle: {seen:?}"
            );
        }
    }

    #[test]
    fn round_robin_refuses_a_nonsensical_configuration() {
        assert!(!fst4w_round_robin_is_tx(0, 120, 0, 0), "zero slots");
        assert!(!fst4w_round_robin_is_tx(0, 0, 0, 3), "zero period");
        assert!(
            !fst4w_round_robin_is_tx(0, 120, 5, 3),
            "slot 5 of 3 does not exist — refuse rather than transmit"
        );
    }

    #[test]
    fn round_robin_is_a_pure_function_of_utc() {
        // No state, no drift: two stations that never talk to each other stay in
        // step forever because the assignment depends only on the clock.
        let day = 86_400u32;
        for t in (0..day).step_by(120) {
            let a = fst4w_round_robin_is_tx(t, 120, 0, 3);
            let b = fst4w_round_robin_is_tx(t, 120, 1, 3);
            let c = fst4w_round_robin_is_tx(t, 120, 2, 3);
            assert_eq!(
                usize::from(a) + usize::from(b) + usize::from(c),
                1,
                "exactly one slot owns t={t}"
            );
        }
    }
}
