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
 * TWO CHECKS ARE DECIDED FROM USB TOPOLOGY, and they carry an extra rule. Topology is only
 * readable on some platforms (macOS today), and even there the IO registry can answer nothing —
 * so both are DIAGNOSTIC and both are WARNINGS: they say nothing when the facts are absent or
 * unproven, and neither may ever be the reason a save is refused. A caller that passes no
 * topology gets exactly the checks that existed before it, unchanged. The bias is deliberate:
 * a topology finding that turns out to be wrong on somebody's station must cost them a sentence,
 * never their configuration.
 *
 * ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every message comes
 * from the catalog. What does NOT: the port name itself, which is interpolated as the operator's
 * own `COM5` / `/dev/cu.usbserial-A` — a device name, never translated and never reformatted.
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
import type { SerialPortInfo } from './api'
import type { AudioDevices } from './types'
import { t } from './i18n'

export type RigCheck = { level: 'error' | 'warning'; message: string }

/** The subset of the settings form these checks read. */
export interface RigFormFacts {
  serialPort: string
  rigConn: string
  pttMethod: string
  rigModel: number
  /** Chosen sound-card device NAMES, for the same-radio check. Absent = nothing chosen yet. */
  audioIn?: string
  audioOut?: string
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
  /**
   * USB topology for the enumerated ports, when the platform can prove any (macOS today; see
   * `tempo_audio::usbtopo`). Purely ADDITIVE — every field on it is optional, every check below
   * that reads it stays silent when it is absent or unproven, and NOTHING is refused on its
   * absence. A caller that does not pass it gets exactly the checks it got before.
   */
  portInfos?: SerialPortInfo[],
  /** The device lists the audio pickers are showing, for the same-radio check. */
  audio?: AudioDevices,
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
      out.push({ level: 'error', message: t('settings.radio.check.noPort') })
    }
    return out
  }

  const present = ports.includes(port)

  // Present at all. A saved port can legitimately be absent (rig switched off), so this is a
  // warning — but the operator should know now rather than wonder later why CAT never connects.
  if (!present) {
    out.push({ level: 'warning', message: t('settings.radio.check.portMissing', { port }) })
  }

  // Callout vs dial-in. `/dev/tty.*` blocks on carrier detect and simply hangs; `/dev/cu.*` is the
  // one to open. They are offered as a pair, differ by four characters, and look interchangeable —
  // and the failure is a hang, not an error, so nothing ever says which one was wrong.
  if (port.startsWith('/dev/tty.')) {
    out.push({ level: 'error', message: t('settings.radio.check.dialIn', { port }) })
  }

  // Everything below is decided from USB TOPOLOGY, which only some platforms can report. A check
  // that cannot prove its case says nothing: `info` is undefined off macOS and on any Mac where
  // the IO registry answered nothing, and then this whole section is inert.
  const info = portInfos?.find((p) => p.name === port)

  // The silent half of a dual bridge. A CP2105 exposes two interfaces and only the first carries
  // CAT on these rigs — choosing the second is the most convincing way to make a working radio
  // look dead: the port opens, the writes succeed, and nothing ever answers. A WARNING, not an
  // error: "normally" is doing real work in that sentence, and an operator whose rig genuinely
  // uses the second interface must still be able to save.
  //
  // ⚠️ `siblingPorts > 1` is not decoration, it is the whole precondition. A single-interface
  // device may number its one interface anything: an LG monitor's control port on the desk this
  // was measured on enumerates as interface 2, and without this guard the check told the operator
  // their ONLY port was "port 3 of this device, CAT is normally on port 1". Advice to pick a
  // different port is only meaningful when a different port exists.
  if (
    info?.interfaceIndex != null &&
    info.interfaceIndex > 0 &&
    info.siblingPorts != null &&
    info.siblingPorts > 1
  ) {
    out.push({
      level: 'warning',
      message: `${port} is port ${info.interfaceIndex + 1} of this device. CAT is normally on port 1 — the other one answers nothing.`,
    })
  }

  // The sound card must be INSIDE the radio on this CAT port.
  //
  // This is the failure a name cannot see. Audio devices are stored BY NAME; two rigs with the
  // same codec chip both enumerate as "USB Audio Device", and the positional " #2" that separates
  // them is assigned by enumeration order. Moving one rig to a different USB socket therefore
  // SWAPS which radio each saved name refers to — every profile silently starts pointing at the
  // other rig, and nothing warns, because both names still resolve to a real device.
  //
  // Topology does not move when names do: a rig that carries CAT and audio down one cable is
  // internally a hub, so its codec shares the CAT port's parent. Comparing the two catches the
  // swap the moment it happens. Only ever a warning — a rig whose audio legitimately is not on its
  // own CAT device (a separate interface box, an analogue card) is a normal setup, not a mistake.
  //
  // And a second reason it can only ever warn: sharing a PARENT is weaker than being the same
  // device. Two unrelated things plugged into one external hub share a parent, so a USB headset
  // beside a rig's CAT adapter can look "inside" it. That is why this speaks only when both sides
  // are proven AND they disagree — the reading is used to raise a doubt, never to settle one.
  if (audio && info?.pairedAudio != null) {
    for (const [field, list, chosen] of [
      ['Input', audio.input, form.audioIn],
      ['Output', audio.output, form.audioOut],
    ] as const) {
      if (!chosen) continue
      const dev = list.find((d) => d.name === chosen)
      const expected = list.find((d) => d.name === info.pairedAudio)
      // Speak only when BOTH sides are proven AND they disagree. An unknown hub on either side is
      // silence, not a finding — the check would otherwise fire on every non-USB sound card.
      if (dev?.usbHub == null || expected?.usbHub == null) continue
      if (dev.usbHub !== expected.usbHub) {
        out.push({
          level: 'warning',
          message: `${field} device “${chosen}” is not inside the radio on ${port} — “${info.pairedAudio}” is. Device names can swap when a rig moves USB port.`,
        })
      }
    }
  }

  // CAT keying with no rig model. `pttMethod: 'cat'` and model 0 (None/VOX) cannot both be true —
  // there is nothing to send the keying command to, so the rig never keys and nothing reports why.
  if (form.pttMethod === 'cat' && form.rigModel === 0) {
    out.push({ level: 'error', message: t('settings.radio.check.catNoModel') })
  }

  return out
}

/** Convenience: does anything here stop a save? */
export function blocks(checks: RigCheck[]): boolean {
  return checks.some((c) => c.level === 'error')
}

/**
 * Hamlib model numbers whose radios Nexus can drive with its OWN CI-V engine.
 *
 * ⚠️ MIRRORS `icom_scope_model` in crates/tempo-audio/src/rigmodels.rs, and
 * `rigFormChecks.test.ts` reads that file and fails if the two drift. It exists because the
 * screen used to ask a DIFFERENT question from the engine: the engine asked "is this model
 * 3078?", the screen asked "does the model NAME look like an IC-7610?" — and a radio stored as
 * `Icom 7610`, `IC-7610M`, or with an empty model name passed the first and failed the second,
 * so the option vanished for a radio Nexus fully supports. A user reported exactly that
 * (2026-08-19). One question, asked from one list.
 */
export const NATIVE_CIV_MODELS: readonly number[] = [3073, 3078, 3081, 3085, 3090]

/** Can this radio's CI-V be driven natively, and if not, why not? `null` = yes. */
export function nativeCivBlockedReason(rigModel: number, rigConn: string): string | null {
  if (!NATIVE_CIV_MODELS.includes(rigModel)) return 'not-supported'
  if (rigConn === 'network') return 'network'
  if (rigConn === 'omnirig') return 'omnirig'
  return null
}

/**
 * Icoms with more than ONE data mode, where `1A 06`'s first byte selects D1/D2/D3.
 *
 * ⚠️ NEEDS BENCH. The IC-7300 has a single DATA mode and is deliberately absent; the rest are
 * listed from their CI-V references and none of it has been confirmed against a radio here. The
 * selector defaults to D1 — today's behaviour — so an operator who never touches it is
 * unaffected either way.
 */
export const MULTI_DATA_MODE_ICOMS: readonly number[] = [3078, 3081, 3085, 3090]
