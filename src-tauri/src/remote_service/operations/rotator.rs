//! The rotator over Remote: point at an azimuth, point at a callsign's entity, stop.
//!
//! The rotctld address and the bearing are resolved from the station's own Settings while the
//! caller holds the Engine lock, exactly as the desktop commands resolve them. The rotctld
//! exchange itself runs on its own thread, so the Engine lock is never held across network I/O,
//! and the receipt stays pending until rotctld answers. A command whose reply failed is UNKNOWN,
//! never applied: the line may still have reached the mast.
//!
//! An operator gesture only. Nothing here is called on a timer or from a spot, and nothing here
//! can arm, key or touch the transmit latch.
use std::time::Instant;
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

/// The desktop `point_rotator_at_call` bearing: great circle from the station grid to the
/// callsign's DXCC entity. No grid or no entity is not a bearing.
pub fn bearing_to_call(settings: &Settings, call: &str) -> Result<f64, Reason> {
    let me = propagation::geo::maidenhead_to_latlon(settings.mygrid.trim())
        .ok_or(Reason::InvalidAction)?;
    let info = propagation::dxcc::resolve(call).ok_or(Reason::InvalidAction)?;
    Ok(propagation::geo::bearing_deg(me, (info.lat, info.lon)))
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
        Command::Point(az) => tempo_audio::rotator::point(addr, az),
        Command::Stop => tempo_audio::rotator::stop(addr),
    }
}

/// Under test the exact rotctld line goes to an in-process fake at that address; an address with
/// no fake is an error, so a test can never open a socket to a real rotator.
#[cfg(test)]
fn send(addr: &str, command: Command) -> std::io::Result<()> {
    let line = match command {
        Command::Point(az) => tempo_audio::rotator::point_line(az),
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
    }

    impl Fake {
        pub fn addr(&self) -> &str {
            &self.addr
        }
        /// Every rotctld line this fake received, in order.
        pub fn lines(&self) -> Vec<String> {
            self.lines.lock().unwrap().clone()
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
        });
        FAKES
            .lock()
            .unwrap()
            .get_or_insert_with(HashMap::new)
            .insert(addr.into(), fake.clone());
        fake
    }

    pub(super) fn call(addr: &str, line: &str) -> std::io::Result<()> {
        let fake = FAKES
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|fakes| fakes.get(addr).cloned());
        let Some(fake) = fake else {
            return Err(std::io::Error::other("no test rotator at this address"));
        };
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
