//! Serial-port enumeration for the rig-control UI (feature `serial`).
//!
//! [`available_ports`] is public in both builds so UI code can call it
//! unconditionally. With the `serial` feature it lists the OS serial ports via
//! `serialport`; without it (the headless build, which has no libudev) it
//! returns an empty list so a port can still be typed in by hand.

/// Names of the serial ports currently present (e.g. `"COM5"`, `"/dev/ttyUSB0"`).
///
/// Returns an empty `Vec` when built without the `serial` feature, or if the
/// platform enumeration fails.
#[cfg(feature = "serial")]
pub fn available_ports() -> Vec<String> {
    match serialport::available_ports() {
        Ok(ports) => ports.into_iter().map(|p| p.port_name).collect(),
        Err(_) => Vec::new(),
    }
}

/// Names of the serial ports currently present.
///
/// Without the `serial` feature there is no enumeration backend, so this
/// returns an empty `Vec`; the operator can still type a port name manually.
#[cfg(not(feature = "serial"))]
pub fn available_ports() -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn available_ports_is_callable() {
        // We can't assert hardware is present; just prove the function exists
        // and returns a Vec in either build configuration.
        let _ports: Vec<String> = available_ports();
    }
}
