/**
 * Pre-save checks for the rig form.
 *
 * The form has exactly one check — the callsign — so every way of getting the RADIO wrong saves
 * silently and then behaves like broken hardware: no CAT, no keying, or a rig that answers nothing
 * and looks dead. Each check below is a mistake that is easy to make, invisible at save time, and
 * expensive to diagnose afterwards, because the symptom appears far from the cause.
 *
 * Pure on purpose. Everything is decided from the form plus facts passed in, so it is
 * unit-testable without hardware, a running app, or a rig.
 *
 * An `error` stops the save; a `warning` is stated and the operator proceeds. The split matters:
 * an operator with an unusual-but-correct setup must never be locked out of their own
 * configuration by a heuristic, so only things that CANNOT be right block.
 *
 * WHAT IS DELIBERATELY NOT HERE — two ports collide (two profiles on one serial port). The
 * backend already decides that, in `settings::serial_port_conflicts`, and App.tsx already puts
 * its verdict in the status lane as `radioConfigWarning`. That rule carries four qualifiers a
 * form-side copy loses on sight — the other profile must be `enabled`, have `rig_model > 0`, be
 * on `rig_conn == "serial"` and have a non-empty port, and the comparison is case-insensitive —
 * so the copy fires on disabled profiles and misses `COM3` vs `com3`. It is also a WARNING
 * there, correctly: a station that swaps one cable between two rigs has both profiles on one
 * port on purpose and must still be able to save. One rule, one place; this file does not get a
 * second opinion on it.
 */
export type RigCheck = { level: 'error' | 'warning'; message: string }

/** The subset of the settings form these checks read. */
export interface RigFormFacts {
  serialPort: string
  rigConn: string
  pttMethod: string
  rigModel: number
}

export function checkRigForm(
  form: RigFormFacts,
  /** Names of the ports currently enumerated. Presence is all these checks need. */
  ports: string[],
  /**
   * Models that need no serial port, from `getPortlessRigModels()` — the backend's own
   * `model <= 4 || is_software_cat_profile(model)` (crates/tempo-audio/src/usbrig.rs:266).
   * Empty means the rule could not be read, and then the port check does not BLOCK: an
   * unreadable rule must not be why a correct configuration cannot be saved.
   */
  portlessModels: number[],
): RigCheck[] {
  const out: RigCheck[] = []
  // A network rig has no serial port at all; none of this applies.
  if (form.rigConn === 'network') return out
  // Nor does an OmniRig one, and for a stronger reason: OmniRig owns the rig type, the COM
  // port and the baud, so every field these checks read belongs to another program. Blocking a
  // save on "no serial port chosen" would make a correct OmniRig configuration unsaveable —
  // and the CAT-keying check below would refuse `pttMethod: 'cat'` with no rig model, which is
  // exactly the normal OmniRig setup.
  if (form.rigConn === 'omnirig') return out

  const port = form.serialPort.trim()
  if (!port) {
    // CAT is wanted and there is nowhere to send it — the one case where "nothing configured"
    // and "misconfigured" look identical afterwards.
    //
    // Gated on the portless set, because a whole class of models is served over TCP or a
    // virtual COM pair by a program on this machine: Dummy, NET rigctl, FLRig, Thetis,
    // PowerSDR, SmartSDR, SDR Console. Those are configured with no port ON PURPOSE, and
    // before this gate the check called every one of them an error and blocked the save.
    // Model 0 (None/VOX) is in that set too, so "no model chosen" needs no separate case.
    // `Array.isArray` rather than a bare `.length`: this runs inside the save handler, and a
    // throw here would abort the save with no message at all — the exact failure mode the rest
    // of this file exists to prevent.
    const ruleKnown = Array.isArray(portlessModels) && portlessModels.length > 0
    if (ruleKnown && !portlessModels.includes(form.rigModel)) {
      out.push({
        level: 'error',
        message: 'No serial port chosen — a rig model is set, so CAT needs a port.',
      })
    }
    return out
  }

  const present = ports.includes(port)

  // Present at all. A saved port can legitimately be absent (rig switched off), so this is a
  // warning — but the operator should know now rather than wonder later why CAT never connects.
  if (!present) {
    out.push({
      level: 'warning',
      message: `${port} is not connected right now — check the rig is powered on, or pick another port.`,
    })
  }

  // Callout vs dial-in. `/dev/tty.*` blocks on carrier detect and simply hangs; `/dev/cu.*` is the
  // one to open. They are offered as a pair, differ by four characters, and look interchangeable —
  // and the failure is a hang, not an error, so nothing ever says which one was wrong.
  if (port.startsWith('/dev/tty.')) {
    out.push({
      level: 'error',
      message: `${port} is a dial-in device and will hang waiting for carrier. Use the matching /dev/cu.… port instead.`,
    })
  }

  // CAT keying with no rig model. `pttMethod: 'cat'` and model 0 (None/VOX) cannot both be true —
  // there is nothing to send the keying command to, so the rig never keys and nothing reports why.
  if (form.pttMethod === 'cat' && form.rigModel === 0) {
    out.push({
      level: 'error',
      message:
        'PTT is set to CAT but the rig model is None/VOX — pick your rig model, or choose a different PTT method.',
    })
  }

  return out
}

/** Convenience: does anything here stop a save? */
export function blocks(checks: RigCheck[]): boolean {
  return checks.some((c) => c.level === 'error')
}
