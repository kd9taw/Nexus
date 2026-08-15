/**
 * Pre-save checks for the rig form.
 *
 * The form has exactly one check — the callsign — so every way of getting the RADIO wrong saves
 * silently and then behaves like broken hardware: no CAT, no keying, or a rig that answers nothing
 * and looks dead. Each check below is a mistake that is easy to make, invisible at save time, and
 * expensive to diagnose afterwards, because the symptom appears far from the cause.
 *
 * Pure on purpose. Everything is decided from the form plus the enumerated ports, so it is
 * unit-testable without hardware, a running app, or a rig.
 *
 * An `error` stops the save; a `warning` is stated and the operator proceeds. The split matters:
 * an operator with an unusual-but-correct setup must never be locked out of their own
 * configuration by a heuristic, so only things that CANNOT be right block.
 */
export type RigCheck = { level: 'error' | 'warning'; message: string }

/** The subset of the settings form these checks read. */
export interface RigFormFacts {
  serialPort: string
  rigConn: string
  pttMethod: string
  rigModel: number
  radios?: { id: number; name: string; serialPort: string }[]
  /** Optional on `Settings` (a pre-roster config has none); treated as radio 0. */
  activeRadio?: number
}

/** Which radio the form is describing — the one being edited, else the active one. */
function targetRadioId(form: RigFormFacts, editingRadioId: number | null | undefined): number {
  return editingRadioId ?? form.activeRadio ?? 0
}

export function checkRigForm(
  form: RigFormFacts,
  /** Names of the ports currently enumerated. Presence is all these checks need. */
  ports: string[],
  editingRadioId: number | null | undefined,
): RigCheck[] {
  const out: RigCheck[] = []
  // A network rig has no serial port at all; none of this applies.
  if (form.rigConn === 'network') return out

  const port = form.serialPort.trim()
  if (!port) {
    // A model is set, so CAT is wanted — and CAT with no port is the one case where "nothing
    // configured" and "misconfigured" look identical afterwards.
    if (form.rigModel !== 0) {
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

  // Another radio already uses it. Two profiles on one port means two daemons fighting for it; the
  // loser dies and its radio silently stops responding.
  const me = targetRadioId(form, editingRadioId)
  const clash = (form.radios ?? []).find((r) => r.id !== me && r.serialPort.trim() === port)
  if (clash) {
    out.push({
      level: 'error',
      message: `${clash.name} already uses ${port}. Two radios cannot share one CAT port.`,
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
