// ISS SSTV auto-arm (opt-in, off by default). When enabled, at the start of an
// ISS pass we tune the rig to the 145.800 FM downlink and arm the SSTV decoder;
// at LOS we disarm and put the operator's dial back. It mirrors satAlarm.ts: an
// app-wide loop driven by the 30 s prop poll, with module state that makes each
// arm happen at most once per pass (the `weArmedIt` guard).
//
// The safety rules are absolute: EVERY rig-touching action is gated on the
// opt-in AND on the screen in use (`ISS_SCREENS`), and we never fight the
// operator — the dial is restored ONLY while it's still sitting on 145.800 FM (a
// mid-pass manual QSY is left alone), and SSTV is disarmed ONLY for a pass WE
// armed. `savedDial` is in-session (a mid-pass app restart forfeits the restore —
// acceptable for v1).
//
// ⛔ THE OPT-IN IS NOT A LICENCE TO TAKE THE RADIO FROM ANOTHER SCREEN. It once
// was: every ISS pass sent 145.800 FM to whatever radio was in use. An FT-710
// running FT8 on 20 m refused 2 m, its dial read back 14.074 with the app's mode
// left at FM, the band picker said "20m (custom)", and the band change had
// already turned transmit off (reproduced 2026-10-13, from a report of exactly
// that picker). Away from SSTV and Satellites the pass is announced, once, and
// nothing moves: an alert notifies, it never acts.
//
// A satellite pass the operator is working has the radio too, on any screen.
// While a transponder is held, Doppler steers the dial back to that bird within
// seconds, so a park on 145.800 is taken straight back. The ISS pass is
// announced as it is elsewhere, and the auto-arm arms if the hold ends while
// the ISS is still up.
//
// Where NO radio here can receive 145.800 (the station's only enabled radio, its
// probed coverage leaving 2 m out: an FT-710 alone), the auto-arm cannot work on
// any screen, and a tune could only be refused. It says so once a session, with
// where the switch is, and is quiet after that: no notice per pass, no tune.

//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The
// notices come from the catalog; the DIAL and the MODE are interpolated
// from the constants below and never become text, because a decimal comma
// reaching 145.800 is an operating fault rather than a cosmetic one.

import { pushToast } from '../toast'
import { t } from '../i18n'
import type { SatPass } from '../types'
import type { View } from './registry'

// The ISS SSTV downlink — the 2 m band-plan channel (bandplan.rs).
const ISS_DIAL_MHZ = 145.8
const ISS_BAND = '2m'
const ISS_MODE = 'FM'
// Dial-match tolerance (MHz) — 1 kHz. Covers CAT read-back rounding while still
// meaning "the operator is on OUR channel", not somewhere they've retuned to.
const DIAL_EPS = 0.001
/** The screens where working the ISS downlink IS what the operator is doing: SSTV,
 * where the pictures land, and Satellites, where the pass is. On any other screen
 * the radio is that screen's (FT8 on 20 m, a CW run, a net on 2 m), so the pass is
 * announced and the radio is left alone. */
const ISS_SCREENS: ReadonlySet<View> = new Set<View>(['sstv', 'sats'])

/** The dial fields the tick reads / restores (a subset of RadioStatus). */
export interface IssRadio {
  dialMhz: number
  band: string
  sideband: string
  /** The radio's probed receive coverage, `[loMhz, hiMhz]` pairs. Empty or absent is UNKNOWN,
   * which never stops the auto-arm. */
  rxRangesMhz?: [number, number][] | null
}

/** The rig actions the tick performs, injected so tests can observe them. */
export interface IssArmDeps {
  /** Park the rig on the 145.800 downlink — the APP's channel, not the operator's dial.
   * MUST be the tuneChannel verb, never setFrequency: the backend records who authored a
   * dial write, and 145.800 arriving as an operator tune would be banked as their 2 m
   * frequency by the per-(band, mode) dial memory the moment they switched modes. */
  tuneChannel: (dialMhz: number, band: string, mode: string) => void
  /** Put the operator's OWN saved dial back at LOS. This one really is theirs, so it is an
   * ordinary operator tune. */
  setFrequency: (dialMhz: number, band: string, mode: string) => void
  /** Start the SSTV receiver for the pass. Explicit, because the operator opted in. */
  sstvArm: (on: boolean) => unknown
  /** Stop it again at the end of the pass.
   *
   * ⚠️ MUST NOT be `sstvArm(false)`. That is an operator STOP, and the engine
   * remembers it for the session: the first LOS would then leave every later entry
   * to the SSTV view refusing to start the receiver — silently reinstating the
   * "SSTV never decodes anything" bug for anyone who turned this opt-in on. */
  sstvAutoDisarm: () => unknown
}

// Session state. Only ever touch the rig for a pass WE armed.
let weArmedIt = false
let savedDial: IssRadio | null = null
// The AOS of the last pass announced from another screen: one notice per pass.
let toldAos: number | null = null
// Told that no radio here can receive 145.800: once a session, then quiet.
let toldCannotReceive = false

/** Test hook — forget any in-session arm. Does NOT touch the rig. */
export function resetIssAutoArm(): void {
  weArmedIt = false
  savedDial = null
  toldAos = null
  toldCannotReceive = false
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

/** No radio here can receive the ISS downlink: `radio` is the station's only enabled radio and
 * its probed coverage leaves 145.800 out. The satellite pick asks the same two questions before it
 * holds a bird (`Engine::hold_sat_transponder`). Unknown coverage, or another radio that may take
 * the channel, is never "cannot". */
function cannotReceiveIss(radio: IssRadio | undefined, soleRadio: boolean): boolean {
  const ranges = radio?.rxRangesMhz
  if (!soleRadio || !ranges || ranges.length === 0) return false
  return !ranges.some(([lo, hi]) => ISS_DIAL_MHZ >= lo && ISS_DIAL_MHZ <= hi)
}

/** Fire-and-forget a receiver call (it may return a promise); swallow failures so
 * a decoder hiccup never rejects the poll tick. */
function fireAndForget(call: () => unknown): void {
  void Promise.resolve(call()).catch(() => {})
}

/** Undo an auto-arm: disarm SSTV, and restore the saved dial ONLY if we're still
 * on the ISS channel (never fight a manual QSY). Clears the session state. */
function unwind(radio: IssRadio | undefined, deps: IssArmDeps, reason: string): void {
  // The AUTOMATIC stop — see `IssArmDeps.sstvAutoDisarm`. Nobody pressed anything, so
  // there is no operator decision for the engine to remember.
  fireAndForget(() => deps.sstvAutoDisarm())
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
 * current/next pass (from getIssPass) and the screen in front of the operator.
 * EVERY rig-touching action is gated on `enabled`: at AOS of an in-progress pass,
 * on the SSTV or Satellites screen, it saves the dial, tunes 145.800 FM, and arms
 * the SSTV decoder; on any other screen it only says the ISS is overhead, once
 * per pass, and arms when the operator opens SSTV or Satellites during it. At LOS
 * — or when the opt-in is flipped off mid-pass — it disarms and restores the
 * saved dial. Idempotent under the repeating tick: the `weArmedIt` guard means a
 * second tick inside the same pass does nothing. `soleRadio` = `radio` is the
 * station's only enabled radio; with its coverage leaving 145.800 out, nothing is
 * tuned on any screen and the notice is said once a session. `passHoldsDial` = a
 * satellite pass holds the dial (a transponder is held); the pass is then announced
 * on every screen, as on a screen that does not work the ISS.
 */
export function tickIssAutoArm(
  pass: SatPass | null,
  radio: IssRadio | undefined,
  enabled: boolean,
  deps: IssArmDeps,
  nowSecs: number,
  screen: View,
  soleRadio = false,
  passHoldsDial = false,
): void {
  // Opt-in OFF: unwind an arm still in flight (so turning it off mid-pass puts
  // the dial back), then stay completely inert.
  if (!enabled) {
    if (weArmedIt) unwind(radio, deps, t('sat.iss.optOut'))
    return
  }

  const inPass = !!pass && pass.aosUnix <= nowSecs && nowSecs <= pass.losUnix

  // Armed, but the pass is over (or no longer reported) → LOS unwind.
  if (weArmedIt && !inPass) {
    unwind(radio, deps, t('sat.iss.los'))
    return
  }

  // In-pass, not yet armed, and the operator isn't already on 145.800 → save
  // their dial, tune to the downlink, arm the decoder. (If they're already on
  // 145.800 we leave the VFO alone and don't claim to have moved it.)
  if (inPass && !weArmedIt && !onIssDial(radio)) {
    // …unless no radio here can receive it. Then a tune would only be refused, on
    // any screen: say so once a session, then leave everything alone. Sticky,
    // because it is said once: a notice that faded before the operator looked
    // would not come again until the next session.
    if (cannotReceiveIss(radio, soleRadio)) {
      if (!toldCannotReceive) {
        toldCannotReceive = true
        pushToast(t('sat.iss.cannotReceive', { dial: ISS_DIAL_MHZ.toFixed(3) }), 'info', 0)
      }
      return
    }
    // …and only on a screen that works the ISS, with no satellite pass holding the
    // dial. Elsewhere, or under a held pass, the radio is in use: say so once, move
    // nothing, and arm if they come to SSTV or Satellites, or the hold ends.
    if (!ISS_SCREENS.has(screen) || passHoldsDial) {
      if (toldAos !== pass.aosUnix) {
        toldAos = pass.aosUnix
        // On the SSTV screen only a held pass gets here, so the notice says that, and does not ask
        // the operator to open the screen they are on.
        pushToast(
          screen === 'sstv'
            ? t('sat.iss.overhead.onSstv', { dial: ISS_DIAL_MHZ.toFixed(3), mode: ISS_MODE })
            : t('sat.iss.overhead', { dial: ISS_DIAL_MHZ.toFixed(3), mode: ISS_MODE }),
          'info',
          12000,
          { prominent: true },
        )
      }
      return
    }
    savedDial = radio
      ? { dialMhz: radio.dialMhz, band: radio.band, sideband: radio.sideband }
      : null
    deps.tuneChannel(ISS_DIAL_MHZ, ISS_BAND, ISS_MODE)
    fireAndForget(() => deps.sstvArm(true))
    weArmedIt = true
    // Finite (not persistent) so the banner can't linger past LOS reading "armed"
    // after we've disarmed — the live SSTV section shows the armed state itself.
    pushToast(
      t('sat.iss.armed', { dial: ISS_DIAL_MHZ.toFixed(3), mode: ISS_MODE }),
      'success',
      12000,
      { prominent: true },
    )
  }
}
