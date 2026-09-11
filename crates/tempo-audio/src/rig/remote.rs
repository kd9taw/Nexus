//! Bounded Remote retuning through the existing CAT connection owner.
//!
//! The caller resolves station policy first. This module reads the expected
//! position and PTT, writes mode before dial, permits one in-command correction
//! for a band-stack override, and reads back the reported result. Every write
//! carries the original permission to Rig's final socket boundary.
//!
//! A returned readback is not a committed station setting or a receipt of RF
//! output. The owning worker must still validate the native context and commit
//! the station state before completing the operation. No caller may retry an
//! uncertain transaction. Radio capability is not advertised by this module.

use super::tuning::{passband_for, retune_passband, same_named_band};
use super::Rig;
use std::time::Instant;
use tempo_app::remote_control::{Reason, WritePermission};

mod filter;

/// A station-resolved CAT position, not an arbitrary browser command string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Position {
    dial_hz: u64,
    mode: String,
}

impl Position {
    pub fn new(dial_hz: u64, mode: &str) -> Result<Self, Reason> {
        if dial_hz == 0 || !supported_mode(mode) {
            return Err(Reason::InvalidAction);
        }
        Ok(Self {
            dial_hz,
            mode: mode.into(),
        })
    }

    pub fn dial_hz(&self) -> u64 {
        self.dial_hz
    }

    pub fn mode(&self) -> &str {
        &self.mode
    }
}

fn supported_mode(mode: &str) -> bool {
    matches!(
        mode,
        "USB"
            | "LSB"
            | "CW"
            | "CWR"
            | "RTTY"
            | "RTTYR"
            | "PKTUSB"
            | "PKTLSB"
            | "AM"
            | "FM"
            | "PKTFM"
    )
}

/// Consumed once by the radio owner; no deferred settings mutation or retry.
pub struct Retune {
    expected: Position,
    target: Position,
    power_limit: Option<f32>,
}

impl Retune {
    pub fn new(expected: Position, target: Position) -> Self {
        Self {
            expected,
            target,
            power_limit: None,
        }
    }

    /// A mode-entry ceiling, not a request to raise the radio's current power.
    pub fn with_power_limit(mut self, limit: f32) -> Result<Self, Reason> {
        if !limit.is_finite() || !(0.0..=1.0).contains(&limit) {
            return Err(Reason::InvalidAction);
        }
        self.power_limit = Some(limit);
        Ok(self)
    }
}

/// Only the readback path constructs this value. Freshness and station identity
/// still need checking at the caller's canonical-state commit boundary.
#[derive(Debug)]
pub struct Readback {
    position: Position,
    sampled_at: Instant,
    power: Option<f32>,
    passband: Option<u32>,
}

impl Readback {
    pub fn position(&self) -> &Position {
        &self.position
    }

    pub fn sampled_at(&self) -> Instant {
        self.sampled_at
    }
    pub fn power(&self) -> Option<f32> {
        self.power
    }
    pub fn passband(&self) -> Option<u32> {
        self.passband
    }
}

impl Rig {
    fn remote_read_power(&mut self, permission: &WritePermission) -> Result<f32, Reason> {
        permission.check(Instant::now())?;
        let value = self.read_level("RFPOWER");
        permission.check(Instant::now())?;
        value
            .ok()
            .filter(|p| p.is_finite() && (0.0..=1.0).contains(p))
            .ok_or(Reason::ReadingUnavailable)
    }

    fn remote_limit_power(
        &mut self,
        limit: f32,
        permission: &WritePermission,
    ) -> Result<f32, Reason> {
        self.remote_require_idle(permission)?;
        let before = self.remote_read_power(permission)?;
        if before > limit + 0.001 {
            self.remote_require_idle(permission)?;
            let reply = self
                .command_permitted(
                    &super::level_line("RFPOWER", &format!("{:.3}", limit)),
                    None,
                    Some(permission),
                )
                .map_err(|_| Reason::HardwareUnconfirmed)?;
            if !super::reply_ok(&reply) {
                return Err(Reason::HardwareUnconfirmed);
            }
        }
        let after = self.remote_read_power(permission)?;
        if after > limit + 0.001 {
            return Err(Reason::HardwareUnconfirmed);
        }
        Ok(after)
    }

    fn remote_reported_mode(&mut self, permission: &WritePermission) -> Result<String, Reason> {
        permission.check(Instant::now())?;
        let mode = self.read_mode();
        permission.check(Instant::now())?;
        mode.filter(|m| supported_mode(m))
            .ok_or(Reason::ReadingUnavailable)
    }

    fn remote_reported_position(
        &mut self,
        permission: &WritePermission,
    ) -> Result<Position, Reason> {
        permission.check(Instant::now())?;
        let dial = self.read_freq();
        permission.check(Instant::now())?;
        let dial_hz = dial.map_err(|_| Reason::ReadingUnavailable)?;
        let mode = self.remote_reported_mode(permission)?;
        Position::new(dial_hz, &mode)
    }

    fn remote_require_idle(&mut self, permission: &WritePermission) -> Result<(), Reason> {
        permission.check(Instant::now())?;
        if self.keyed {
            return Err(Reason::StationBusy);
        }
        let keyed = self.read_ptt();
        permission.check(Instant::now())?;
        match keyed {
            Some(false) => Ok(()),
            Some(true) => Err(Reason::StationBusy),
            None => Err(Reason::ReadingUnavailable),
        }?;
        // A cached simplex flag cannot protect a selected-VFO write. Read the
        // actual split state on this same connection at every retune boundary.
        let split = self.read_split();
        permission.check(Instant::now())?;
        match split {
            Some((false, _)) => Ok(()),
            Some((true, _)) => Err(Reason::StationBusy),
            None => Err(Reason::ReadingUnavailable),
        }
    }

    /// Execute one retune without changing Engine settings or retrying after a
    /// refusal. The completion stays pending until the caller commits the result.
    pub fn remote_retune(
        &mut self,
        retune: Retune,
        permission: &WritePermission,
    ) -> Result<Readback, Reason> {
        let result = self.remote_retune_inner(retune, permission);
        if let Err(reason) = result {
            permission.refuse(reason);
        }
        result
    }

    fn remote_retune_inner(
        &mut self,
        retune: Retune,
        permission: &WritePermission,
    ) -> Result<Readback, Reason> {
        permission.check(Instant::now())?;
        if self.control.is_none() {
            return Err(Reason::HardwareUnavailable);
        }
        if self.keyed {
            return Err(Reason::StationBusy);
        }
        let before = self.remote_reported_position(permission)?;
        if before != retune.expected {
            return Err(Reason::ContextChanged);
        }
        self.remote_require_idle(permission)?;
        // Prove power can be observed before any mode/dial mutation. Some rigs
        // cannot report this level; they cannot confirm a capped transition.
        if retune.power_limit.is_some() {
            self.remote_read_power(permission)?;
        }
        let target = retune.target;
        let mode_changed = target.mode != before.mode;
        self.remote_set_mode(
            &target.mode,
            retune_passband(&target.mode, mode_changed, before.dial_hz, target.dial_hz),
            permission,
        )
        .map_err(|_| Reason::HardwareUnconfirmed)?;
        // A mode acknowledgement alone does not establish the convention in
        // which the following dial would be interpreted (CW pitch-offset rigs).
        if self.remote_reported_mode(permission)? != target.mode {
            return Err(Reason::HardwareUnconfirmed);
        }
        if target.dial_hz != before.dial_hz || mode_changed {
            self.remote_require_idle(permission)?;
            self.remote_set_freq(target.dial_hz, permission)
                .map_err(|_| Reason::HardwareUnconfirmed)?;
            let after_mode = self.remote_reported_mode(permission)?;
            if after_mode != target.mode {
                if same_named_band(before.dial_hz, target.dial_hz) {
                    // A same-band discrepancy is not evidence of band stacking.
                    return Err(Reason::HardwareUnconfirmed);
                }
                self.remote_require_idle(permission)?;
                self.remote_set_mode(&target.mode, passband_for(&target.mode), permission)
                    .map_err(|_| Reason::HardwareUnconfirmed)?;
                if self.remote_reported_mode(permission)? != target.mode {
                    return Err(Reason::HardwareUnconfirmed);
                }
                self.remote_require_idle(permission)?;
                self.remote_set_freq(target.dial_hz, permission)
                    .map_err(|_| Reason::HardwareUnconfirmed)?;
            }
        }
        // Mode/band registers may recall a power level. Apply the ceiling only
        // after reaching the target, and carry any already-lower level forward.
        let sampled_at = Instant::now();
        let power = retune
            .power_limit
            .map(|limit| self.remote_limit_power(limit, permission))
            .transpose()?;
        let after = self.remote_reported_position(permission)?;
        if after != target {
            return Err(Reason::HardwareUnconfirmed);
        }
        self.remote_require_idle(permission)?;
        Ok(Readback {
            position: after,
            sampled_at,
            power,
            passband: None,
        })
    }
}
