// ISS SSTV auto-arm (opt-in, off by default). When enabled, at the start of an
// ISS pass we tune the rig to the 145.800 FM downlink and arm the SSTV decoder;
// at LOS we disarm and put the operator's dial back. It mirrors satAlarm.ts: an
// app-wide loop driven by the 30 s prop poll, with module state that makes each
// arm happen at most once per pass (the `weArmedIt` guard).
//
// The safety rules are absolute: EVERY rig-touching action is gated on the
// opt-in, and we never fight the operator — the dial is restored ONLY while it's
// still sitting on 145.800 FM (a mid-pass manual QSY is left alone), and SSTV is
// disarmed ONLY for a pass WE armed. `savedDial` is in-session (a mid-pass app
// restart forfeits the restore — acceptable for v1).

import { pushToast } from '../toast'
import type { SatPass } from '../types'

// The ISS SSTV downlink — the 2 m band-plan channel (bandplan.rs).
const ISS_DIAL_MHZ = 145.8
const ISS_BAND = '2m'
const ISS_MODE = 'FM'
// Dial-match tolerance (MHz) — 1 kHz. Covers CAT read-back rounding while still
// meaning "the operator is on OUR channel", not somewhere they've retuned to.
const DIAL_EPS = 0.001

/** The dial fields the tick reads / restores (a subset of RadioStatus). */
export interface IssRadio {
  dialMhz: number
  band: string
  sideband: string
}

/** The two rig actions the tick performs, injected so tests can observe them. */
export interface IssArmDeps {
  setFrequency: (dialMhz: number, band: string, mode: string) => void
  sstvArm: (on: boolean) => unknown
}

// Session state. Only ever touch the rig for a pass WE armed.
let weArmedIt = false
let savedDial: IssRadio | null = null

/** Test hook — forget any in-session arm. Does NOT touch the rig. */
export function resetIssAutoArm(): void {
  weArmedIt = false
  savedDial = null
}

/** Dial is within tolerance of the 145.800 downlink (mode-agnostic). */
function onIssDial(radio: IssRadio | undefined): boolean {
  return !!radio && Math.abs(radio.dialMhz - ISS_DIAL_MHZ) < DIAL_EPS
}

/** Still parked on the ISS SSTV channel — 145.800 AND FM. The guard that keeps a
 * restore from yanking the operator back out of a manual QSY. */
function onIssChannel(radio: IssRadio | undefined): boolean {
  return onIssDial(radio) && !!radio && radio.sideband.toUpperCase() === ISS_MODE
}

/** Fire-and-forget the arm/disarm (it may return a promise); swallow failures so
 * a decoder hiccup never rejects the poll tick. */
function armSstv(deps: IssArmDeps, on: boolean): void {
  void Promise.resolve(deps.sstvArm(on)).catch(() => {})
}

/** Undo an auto-arm: disarm SSTV, and restore the saved dial ONLY if we're still
 * on the ISS channel (never fight a manual QSY). Clears the session state. */
function unwind(radio: IssRadio | undefined, deps: IssArmDeps, reason: string): void {
  armSstv(deps, false)
  const restore = savedDial
  if (restore && onIssChannel(radio)) {
    deps.setFrequency(restore.dialMhz, restore.band, restore.sideband)
  }
  weArmedIt = false
  savedDial = null
  pushToast(reason, 'success', 6000, { prominent: true })
}

/**
 * ISS SSTV auto-arm tick — call on each 30 s pass poll with the ISS's
 * current/next pass (from getIssPass). EVERY rig-touching action is gated on
 * `enabled`: at AOS of an in-progress pass it saves the dial, tunes 145.800 FM,
 * and arms the SSTV decoder; at LOS — or when the opt-in is flipped off mid-pass
 * — it disarms and restores the saved dial. Idempotent under the repeating tick:
 * the `weArmedIt` guard means a second tick inside the same pass does nothing.
 */
export function tickIssAutoArm(
  pass: SatPass | null,
  radio: IssRadio | undefined,
  enabled: boolean,
  deps: IssArmDeps,
  nowSecs: number,
): void {
  // Opt-in OFF: unwind an arm still in flight (so turning it off mid-pass puts
  // the dial back), then stay completely inert.
  if (!enabled) {
    if (weArmedIt) unwind(radio, deps, 'ISS auto-arm off — SSTV disarmed, dial restored')
    return
  }

  const inPass = !!pass && pass.aosUnix <= nowSecs && nowSecs <= pass.losUnix

  // Armed, but the pass is over (or no longer reported) → LOS unwind.
  if (weArmedIt && !inPass) {
    unwind(radio, deps, 'ISS LOS — SSTV disarmed, dial restored')
    return
  }

  // In-pass, not yet armed, and the operator isn't already on 145.800 → save
  // their dial, tune to the downlink, arm the decoder. (If they're already on
  // 145.800 we leave the VFO alone and don't claim to have moved it.)
  if (inPass && !weArmedIt && !onIssDial(radio)) {
    savedDial = radio
      ? { dialMhz: radio.dialMhz, band: radio.band, sideband: radio.sideband }
      : null
    deps.setFrequency(ISS_DIAL_MHZ, ISS_BAND, ISS_MODE)
    armSstv(deps, true)
    weArmedIt = true
    // Finite (not persistent) so the banner can't linger past LOS reading "armed"
    // after we've disarmed — the live SSTV section shows the armed state itself.
    pushToast('ISS overhead — tuned 145.800 FM, SSTV armed', 'success', 12000, { prominent: true })
  }
}
