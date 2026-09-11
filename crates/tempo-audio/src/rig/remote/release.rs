//! Idempotent PTT release for an idle radio being selected. This is not a
//! transmit permission or an emergency-stop API: initial physical idle and
//! the caller's expected position are required. Local PTT behavior is unchanged.
use super::*;
use crate::rig::PttMode;

impl Rig {
    /// Native handoff's best-effort CAT Morse flush, with the original permit
    /// at the socket. Some rigs do not implement this command; its reply alone
    /// is not evidence that all external keyer queues stopped.
    pub(crate) fn remote_stop_morse(
        &mut self,
        permission: &WritePermission,
    ) -> std::io::Result<()> {
        self.command_permitted("\\stop_morse\n", None, Some(permission))
            .map(|_| ())
    }

    /// Preserve unkey-on-adopt through the profile's real PTT method. The caller
    /// must set that method before calling; a monitor normally carries Vox.
    /// Separate keyer queues still require their existing native abort paths.
    pub fn remote_unkey_idle(
        &mut self,
        expected: Position,
        permission: &WritePermission,
    ) -> Result<Readback, Reason> {
        let result = (|| {
            if self.remote_idle_position(permission)?.position != expected {
                return Err(Reason::ContextChanged);
            }
            match &self.ptt_mode {
                PttMode::Vox => {}
                PttMode::Cat => {
                    let reply = self
                        .command_permitted(
                            &crate::rig::ptt_line(false),
                            Some(crate::rig::PTT_DEADLINE_MS),
                            Some(permission),
                        )
                        .map_err(|_| Reason::HardwareUnconfirmed)?;
                    if !crate::rig::reply_ok(&reply) && !reply.is_empty() {
                        return Err(Reason::HardwareUnconfirmed);
                    }
                }
                PttMode::Serial { .. } => self.remote_release_serial(permission)?,
            }
            // As on the native path, only a successful release clears belief.
            self.keyed = false;
            let after = self.remote_idle_position(permission)?;
            if after.position != expected {
                return Err(Reason::HardwareUnconfirmed);
            }
            Ok(after)
        })();
        if let Err(reason) = &result {
            permission.refuse(*reason);
        }
        result
    }

    #[cfg(feature = "serial")]
    fn remote_release_serial(&mut self, permission: &WritePermission) -> Result<(), Reason> {
        let PttMode::Serial { port, line } = &self.ptt_mode else {
            return Err(Reason::InvalidAction);
        };
        let line = *line;
        if self.serial.is_none() {
            let opened = crate::control_line::open_control_line_port_permitted(port, permission);
            // Retain a successful handle even if authority expired while the
            // OS opened it. The shared opener has already idled both lines.
            self.serial = Some(opened.map_err(|_| Reason::HardwareUnconfirmed)?);
        }
        release_line(
            self.serial.as_mut().expect("opened serial PTT"),
            line,
            permission,
        )
    }

    #[cfg(not(feature = "serial"))]
    fn remote_release_serial(&mut self, _permission: &WritePermission) -> Result<(), Reason> {
        Err(Reason::UnsupportedAction)
    }
}

#[cfg(feature = "serial")]
fn release_line(
    pins: &mut impl crate::control_line::ControlLinePins,
    line: crate::rig::SerialLine,
    permission: &WritePermission,
) -> Result<(), Reason> {
    permission.begin_write(Instant::now())?;
    match line {
        crate::rig::SerialLine::Rts => pins.set_rts(false),
        crate::rig::SerialLine::Dtr => pins.set_dtr(false),
    }
    .map_err(|_| Reason::HardwareUnconfirmed)
}

#[cfg(all(test, feature = "serial"))]
mod tests {
    use super::*;
    use crate::control_line::ControlLinePins;
    use crate::rig::SerialLine;
    use tempo_app::remote_control::{Completion, Outcome, Revocation};

    #[derive(Default)]
    struct Pins(Vec<(SerialLine, bool)>);
    impl ControlLinePins for Pins {
        fn set_dtr(&mut self, on: bool) -> std::io::Result<()> {
            self.0.push((SerialLine::Dtr, on));
            Ok(())
        }
        fn set_rts(&mut self, on: bool) -> std::io::Result<()> {
            self.0.push((SerialLine::Rts, on));
            Ok(())
        }
    }

    #[test]
    fn serial_release_only_deasserts_the_selected_line_under_original_permission() {
        for line in [SerialLine::Rts, SerialLine::Dtr] {
            for revoke in [false, true] {
                let authority = Revocation::default();
                let native = Revocation::default();
                let deadline = Instant::now() + std::time::Duration::from_secs(5);
                let permit = authority.permit(deadline).unwrap();
                let completion = Completion::guarded(permit.clone());
                let permission = WritePermission::new(
                    permit,
                    native.permit(deadline).unwrap(),
                    completion.clone(),
                );
                if revoke {
                    authority.revoke();
                }
                let mut pins = Pins::default();
                let result = release_line(&mut pins, line, &permission);
                if revoke {
                    assert_eq!(result, Err(Reason::AuthorityExpired));
                    assert!(pins.0.is_empty());
                    assert_eq!(
                        completion.outcome(),
                        Outcome::Rejected {
                            reason: Reason::AuthorityExpired
                        }
                    );
                } else {
                    assert_eq!(result, Ok(()));
                    assert_eq!(pins.0, [(line, false)]);
                    authority.revoke();
                    assert_eq!(
                        completion.outcome(),
                        Outcome::Unknown {
                            reason: Reason::HardwareUnconfirmed
                        }
                    );
                }
            }
        }
    }
}
