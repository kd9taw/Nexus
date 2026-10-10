//! The rotator over Remote: point at an azimuth, point at a callsign's station, stop.
//!
//! The rotctld address and the bearing are resolved from the station's own Settings and Engine
//! while the caller holds the Engine lock, exactly as the desktop commands resolve them: a
//! point-at-call turns where the desktop's does, by the desktop's own resolver. The rotctld
//! exchange itself runs on its own thread, so the Engine lock is never held across network I/O,
//! and the receipt stays pending until rotctld answers. A command whose reply failed is UNKNOWN,
//! never applied: the line may still have reached the mast.
//!
//! An operator gesture only. Nothing here is called on a timer or from a spot, and nothing here
//! can arm, key or touch the transmit latch.
use std::time::Instant;
use tempo_app::engine::Engine;
use tempo_app::remote_control::{Completion, Evidence, Outcome, Permit, Reason};
use tempo_app::settings::Settings;

#[derive(Clone, Copy, Debug)]
pub enum Command {
    /// Point at an absolute azimuth, in degrees.
    Point(f64),
    /// Stop rotation (rotctld `S`).
    Stop,
}

/// A browser azimuth the rotctld line carries exactly: 0 ≤ az < 360, to a tenth of a degree.
pub fn valid_azimuth(az: f64) -> bool {
    az.is_finite() && (0.0..360.0).contains(&az) && ((az * 10.0).round() - az * 10.0).abs() < 1e-6
}

/// A callsign as the grammar admits it: uppercase letters, digits and `/`, 3 to 32 bytes.
pub fn valid_call(call: &str) -> bool {
    (3..=32).contains(&call.len())
        && call
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'/')
}

/// The bearing the desktop's `point_rotator_at_call` turns to, short path: its own resolver
/// (`crate::aim_at_call`) over its own inputs, the station grid and what the engine knows of the
/// call (`crate::station_fixes`: the log form's grid, the grids heard, the callbook's answer, the
/// grid logged before), and only when it knows none the centre of the call's DXCC entity. No grid,
/// or nothing that places the call, is not a bearing.
///
/// ⚠️ This was the entity's centre alone, while the desktop has aimed at the station since
/// 2026-09-29: from FN31, a browser's point at a Boston W1 went to the middle of the United
/// States, 200° away from where the desktop's went.
pub fn bearing_to_call(engine: &Engine, call: &str) -> Result<f64, Reason> {
    crate::aim_at_call(
        &engine.settings().mygrid,
        call,
        crate::station_fixes(engine, call),
    )
    .map(|aim| aim.bearing)
    .map_err(|_| Reason::InvalidAction)
}

/// Admit one rotator command and hand it to its own worker thread. `satellite_track` is whether
/// the desktop's satellite loop is steering the mast: a point would only fight it, so it is
/// refused, but a Stop is never refused for it.
pub fn queue(
    settings: &Settings,
    command: Command,
    permit: Permit,
    satellite_track: bool,
) -> Result<Completion, Reason> {
    if !permit.valid(Instant::now()) {
        return Err(Reason::AuthorityExpired);
    }
    let addr = crate::effective_rotator_addr(settings).ok_or(Reason::HardwareUnavailable)?;
    if let Command::Point(az) = command {
        if !az.is_finite() {
            return Err(Reason::InvalidAction);
        }
        if satellite_track {
            return Err(Reason::StationBusy);
        }
    }
    let completion = Completion::guarded(permit);
    let worker = completion.clone();
    std::thread::Builder::new()
        .name("remote-rotator".into())
        .spawn(move || write(&worker, &addr, command))
        .map_err(|_| Reason::StationBusy)?;
    Ok(completion)
}

/// The worker body: write only while the gesture's authority still holds, then finish the
/// receipt from rotctld's own answer.
pub fn write(completion: &Completion, addr: &str, command: Command) {
    if !completion.begin_write(Instant::now()) {
        // Lapsed before the write: nothing reached rotctld, so this is a refusal, not unknown.
        completion.refuse(Reason::AuthorityExpired);
        return;
    }
    completion.finish(match send(addr, command) {
        Ok(()) => Outcome::Applied {
            evidence: Evidence::StationState,
        },
        Err(_) => Outcome::Unknown {
            reason: Reason::HardwareUnconfirmed,
        },
    });
}

#[cfg(not(test))]
fn send(addr: &str, command: Command) -> std::io::Result<()> {
    match command {
        // A move by hand, as the desktop's: an az/el mast keeps its elevation where it is. The
        // pass's `rotator::point` sends `P <az> 0`, which on a G-5500 is an elevation of zero.
        Command::Point(az) => tempo_audio::rotator::point_keeping(addr, Some(az), None),
        Command::Stop => tempo_audio::rotator::stop(addr),
    }
}

/// Under test the exact rotctld line goes to an in-process fake at that address; an address with
/// no fake is an error, so a test can never open a socket to a real rotator. A point is decided by
/// the desktop's own rule (`keeping_line`, which `point_keeping` runs over TCP), with the fake
/// answering what its mast is and where.
#[cfg(test)]
fn send(addr: &str, command: Command) -> std::io::Result<()> {
    let line = match command {
        Command::Point(az) => tempo_audio::rotator::keeping_line(
            test_rotor::limits(addr),
            || test_rotor::position(addr),
            Some(az),
            None,
        )?,
        Command::Stop => "S\n".to_string(),
    };
    test_rotor::call(addr, &line)
}

#[cfg(test)]
pub mod test_rotor {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc, Mutex};

    pub struct Fake {
        addr: String,
        lines: Mutex<Vec<String>>,
        fail: AtomicBool,
        hold: Mutex<Option<mpsc::Receiver<()>>>,
        mast: Mutex<Mast>,
    }

    /// What the fake rotctld says the mast is when asked (`\dump_state`), and where it is (`p`).
    #[derive(Clone, Copy, Debug)]
    pub enum Mast {
        /// An azimuth-only Hamlib backend, declared as the Rotor-EZ family declares itself. The
        /// default: it gets exactly the line a Remote azimuth move always sent.
        AzimuthOnly,
        /// An az/el rotator (a Yaesu G-5500 on its GS-232B) at `az` / `el`.
        AzEl { az: f64, el: f64 },
        /// A rotctld too busy to answer `\dump_state` before its deadline.
        Silent,
    }

    /// What real backends answer `\dump_state` with (rotctld 4.5.5, over a pty): the Rotor-EZ
    /// (401) and the GS-232B (603).
    const ROTOR_EZ_STATE: &str = "1\n401\nmin_az=0.000000\nmax_az=360.000000\nmin_el=0.000000\n\
                                  max_el=0.000000\nsouth_zero=0\nrot_type=Other\ndone\n";
    const GS232B_STATE: &str = "1\n603\nmin_az=-180.000000\nmax_az=450.000000\nmin_el=0.000000\n\
                                max_el=180.000000\nsouth_zero=0\nrot_type=AzEl\ndone\n";

    impl Fake {
        pub fn addr(&self) -> &str {
            &self.addr
        }
        /// Every rotctld command line this fake received, in order: the moves and stops that reach
        /// the mast. Its questions (`\dump_state`, `p`) are answered from [`Mast`], not recorded.
        pub fn lines(&self) -> Vec<String> {
            self.lines.lock().unwrap().clone()
        }
        /// What the mast is, from now on.
        pub fn mast(&self, mast: Mast) {
            *self.mast.lock().unwrap() = mast;
        }
        /// Answer every later command with a rotctld error.
        pub fn fail(&self, on: bool) {
            self.fail.store(on, Ordering::SeqCst);
        }
        /// Hold the next command until the returned sender fires.
        pub fn hold(&self) -> mpsc::Sender<()> {
            let (tx, rx) = mpsc::channel();
            *self.hold.lock().unwrap() = Some(rx);
            tx
        }
    }

    static FAKES: Mutex<Option<HashMap<String, Arc<Fake>>>> = Mutex::new(None);

    pub fn install(addr: &str) -> Arc<Fake> {
        let fake = Arc::new(Fake {
            addr: addr.into(),
            lines: Mutex::new(Vec::new()),
            fail: AtomicBool::new(false),
            hold: Mutex::new(None),
            mast: Mutex::new(Mast::AzimuthOnly),
        });
        FAKES
            .lock()
            .unwrap()
            .get_or_insert_with(HashMap::new)
            .insert(addr.into(), fake.clone());
        fake
    }

    fn get(addr: &str) -> std::io::Result<Arc<Fake>> {
        FAKES
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|fakes| fakes.get(addr).cloned())
            .ok_or_else(|| std::io::Error::other("no test rotator at this address"))
    }

    /// The fake's answer to `\dump_state`, as `tempo_audio::rotator::read_limits` returns one.
    pub(super) fn limits(addr: &str) -> std::io::Result<Option<tempo_audio::rotator::Limits>> {
        let mast = *get(addr)?.mast.lock().unwrap();
        let state = match mast {
            Mast::AzimuthOnly => ROTOR_EZ_STATE,
            Mast::AzEl { .. } => GS232B_STATE,
            Mast::Silent => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "rotctld did not answer \\dump_state",
                ))
            }
        };
        Ok(tempo_audio::rotator::parse_limits(state))
    }

    /// The fake's answer to `p`, as `tempo_audio::rotator::read_position` returns one.
    pub(super) fn position(addr: &str) -> std::io::Result<(f64, Option<f64>)> {
        let mast = *get(addr)?.mast.lock().unwrap();
        match mast {
            Mast::AzEl { az, el } => Ok((az, Some(el))),
            // An azimuth-only backend reports its elevation as 0.
            _ => Ok((0.0, Some(0.0))),
        }
    }

    pub(super) fn call(addr: &str, line: &str) -> std::io::Result<()> {
        let fake = get(addr)?;
        let hold = fake.hold.lock().unwrap().take();
        if let Some(release) = hold {
            let _ = release.recv();
        }
        fake.lines.lock().unwrap().push(line.into());
        if fake.fail.load(Ordering::SeqCst) {
            Err(std::io::Error::other("RPRT -1"))
        } else {
            Ok(())
        }
    }
}
