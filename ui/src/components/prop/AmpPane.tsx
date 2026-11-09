// The Amplifier pane — a status readout for a linear on its own serial port, with exactly ONE
// control: the Operate/Standby button (operator decision, 2026-10-31: "It's missing in the
// amplifier section in a color button format. This is an important feature for an amp owner").
//
// ⛔ NOTHING HERE STOPS A TRANSMISSION, AND NOTHING HERE MAY EVER BE MADE TO. Putting an
// amplifier in standby is not a way to stop an over — the exciter keeps keying and the drive
// passes straight through — so this pane appears on no cockpit's stop-line census and must never
// be added to a sweep's `stopControls`. The button's every title says so in words.
//
// THE ONE CONTROL, AND ITS RULES. They are the cockpit header strip's (AmpStrip.tsx), held in one
// hook both use (useAmpControl.ts), so the two cannot drift apart:
//   • ITS STATE IS THE AMPLIFIER'S, NEVER OURS. OPERATE is a toggle with no idempotent "set", so a
//     lost or duplicated frame inverts it. The button draws `amp.operate` from the last status
//     frame and nothing else; a click sets no local state, and the next poll moves it.
//   • DEAD WHILE KEYED — the amplifier's own flag OR the radio's ON AIR answer (`radioOnAir`) —
//     and while the link is down, drawn with a dashed edge (a dimming faded the state colour under
//     4.5:1 at Night). A courtesy: the poll thread's interlock (the engine's amplifier permit) is the
//     real guard, and it DROPS a queued press without a word — nothing on screen says so, and the
//     button simply stays as it was. It drops one whenever it cannot be sure the radio is not
//     transmitting: anything keyed, the amplifier showing output power, and — for an Elecraft, which
//     reports no transmit state of its own — EVERY press while Nexus has no fresh PTT reading from
//     the radio over CAT (CAT not connected, or no PTT reading in the last second). It also drops
//     one when the amplifier answering is not the model set in Settings. "Not sent" appears only
//     when Nexus could not QUEUE the press (the queue was full, the call failed, or a Remote client
//     does not hold the station), as on the strip.
//   • ONE PRESS AT A TIME, HELD FROM THE CLICK. The click itself puts the button on hold, before
//     the call to queue the press is even answered, so the second click of a double click lands on
//     a dead button; a press that could not be queued releases it. While held it says "Sent —
//     waiting for the amplifier" until the amplifier's reported state changes or WAIT_MS passes (a
//     dropped press never changes it). A second click in that time would toggle the amplifier
//     straight back. The hold is the WINDOW's, not the component's (`hold`, below), so a remount
//     (a screen switch right after a click) keeps it, with its original deadline. It still shows
//     the state the amplifier last REPORTED, never the one we expect.
//   • DEAD, AND SAYING WHY, WHEN THE STATE IS UNKNOWN. A configured amplifier that is not
//     answering, or answers without an Operate state, draws '—' and the reason. Toggling a state
//     we cannot see would be toggling blind; this is the one place it refuses more than the strip.
//   • RED IS OPERATE ("Active": the amplifier is in line), GREEN IS STANDBY — the operator's
//     colours, deliberately the reverse of the strip's quiet green Operate. Colour is never the
//     only cue: the word, a second word (Active / Not active) and a shape (a lit lamp, a pause
//     mark) say the same, for a colour-blind operator and in every theme.
//
// TWO STATES, and keeping them apart is the whole honesty argument (settings.rs, `amp_port`:
// "unconfigured shows nothing, configured-and-silent shows '—'"):
//   • no amplifier CONFIGURED → `amp == null` → no readout, only the box's one line saying where
//     one is set up. A readout with no amplifier behind it would be an ornament. The line is
//     drawn here: the frame is handed this component and never sees a null from it.
//   • configured and NOT ANSWERING → the pane STAYS, every reading '—', with the reason named.
//     RotorPane.tsx records what the other choice costs: `if (az == null) return null` deleted
//     the rose, the slew and the STOP button the moment a readback failed.
//
// ⚠️ THIS FILE IS ON THE **MIGRATED** LIST (i18n/hardcoded-strings.test.ts): every sentence is
// in the catalog under `amp.*`. What stays in the code is the instrument's own vocabulary —
// the unit symbols W, V, A, ° and the `:1` of an SWR ratio, and the three named constants
// below. Every NUMBER is built here and rounded before display: `invariantNumber` is
// `String(n)`, so a raw f32 SWR of 1.2000000476837158 would interpolate verbatim.
//
// POWER OUT AND SWR ARE THE BIG READINGS, drawn the way the Clock box draws its digits
// (ClockPane.tsx): SVG text in a viewBox that holds BIG_CHARS characters, so a reading follows its
// cell's width in any box (the dashboard rail at its 200 px floor, a wide Connect strip) and never
// wraps or runs past its cell, and a height cap in styles.css that rides --text-scale stops it growing
// past what Text size asks for in a wide one. The other readings are text a step larger than the body.
import { useEffect, useSyncExternalStore } from 'react'
import type { AmpStatus } from '../../types'
import { t } from '../../i18n'
import { T } from '../../i18n/T'
import { useAmpControl } from '../useAmpControl'

/** The instrument's own marks — technical tokens exactly as a meter name is, gathered here so
 *  the catalog guard reads them as the deliberate constants they are (TxMeters.tsx's pattern). */
const SWR = 'SWR'
const ATU = 'ATU'
const VDC = 'Vdc'
/** Absence. Not prose — the em dash IS the reading here, exactly as it is on the S-meter. */
const DASH = '—'
/** A big reading's viewBox holds this many characters, "1500 W" and "99.9:1", and half of one more as
 *  air before the next reading. A longer one widens its own viewBox rather than run past it. */
const BIG_CHARS = 6
/** Between the Operate button's word and its second word, in its accessible name. A mark, not prose. */
const SEP = ' · '
/** How long a queued press holds the button before giving up on an answer. The new state reaches the
 *  screen two poll cycles after the click, not one: the poll thread sleeps a second, reads, sends the
 *  press, and publishes the reading it took BEFORE sending, so the change shows on the NEXT cycle's
 *  read, plus up to 300 ms for the snapshot: about 1 + R + 1 + R + 0.3 s, R the read time. That is
 *  about 2.3 s for an SPE, whose read takes milliseconds. An Elecraft's read is six verbs at up to
 *  0.4 s each in the normal case, which five seconds covers (it covers R up to about 1.35 s); but a
 *  slow KPA read can take up to 2.4 s, about 7 s click to screen, and a press deferred one cycle
 *  behind a Remote browser's command adds another 1 + R. So five seconds covers an SPE and a
 *  TYPICAL Elecraft, not every Elecraft. Chosen over a longer wait because every press the poll
 *  thread drops (an Elecraft with no fresh CAT PTT reading drops all of them) holds the button for
 *  the full wait, and an early release never makes the screen lie: it still shows what the
 *  amplifier reported, and only a second click inside the slow case could toggle it back. */
const WAIT_MS = 5000

/** THE PRESS IN FLIGHT, per WINDOW, keyed by the amplifier — not component state. A component's
 *  state dies with it, so switching screens right after a click (or the box moving between the rail
 *  and a cockpit) would hand back a live button inside the window a second press would toggle the
 *  amplifier straight back in. Held here, every Amplifier box in this window drawn for the same
 *  amplifier reads the same hold, and a remount keeps it. It ends the same two ways: the amplifier
 *  reports a different state (or none), or `until` passes. Another window is another JS realm and
 *  holds its own; the poll thread's queue is the only thing they share. */
interface Hold {
  /** Which amplifier: its family and model id, the only identity the status carries. */
  amp: string
  /** The Operate state it had reported when the operator clicked. */
  from: boolean
  /** When the hold gives up, on this window's MONOTONIC clock (`performance.now()`), never the wall
   *  clock: a system clock stepping back must not keep the button locked for the size of the step. */
  until: number
}
let hold: Hold | null = null
const holdListeners = new Set<() => void>()
function setHold(next: Hold | null): void {
  hold = next
  for (const l of holdListeners) l()
}
/** Release `h` only if it is still the hold in force: a newer press keeps its own. */
function releaseHold(h: Hold): void {
  if (hold === h) setHold(null)
}
function subscribeHold(l: () => void): () => void {
  holdListeners.add(l)
  return () => holdListeners.delete(l)
}
/** Test seam: module state outlives a test's render, so each test starts with no press in flight. */
export function resetOperateHold(): void {
  setHold(null)
}
const ampKey = (amp: AmpStatus) => `${amp.family}/${amp.model}`

/** Wire tag → the sentence for it, each as a THUNK.
 *
 *  Thunks, not resolved strings: this is a module constant read during render, so calling `t`
 *  at import time would freeze whichever locale loaded first — the same reason the Connect
 *  pane registry resolves its titles through getters.
 *
 *  ⭐ EVERY LOOKUP IS TOTAL, AND THE FALLBACK IS THE FAULT. An alarm code a later firmware
 *  ships arrives with the tag `unknown` and must reach the operator as "an alarm we cannot
 *  name", never as silence — the failure direction of a status decoder in front of a kilowatt
 *  is toward reporting a fault. The backend flattens its enums to these flat tags precisely so
 *  this stays one lookup instead of a TS union that falls through on a shape it did not expect.
 */
const REASON: Record<string, () => string> = {
  // An EMPTY reason on an unlinked amplifier is the engine's not-yet-answered status, before the first
  // poll after launch or a radio switch: nothing has failed, so it is not "Not answering". Any other token
  // this page does not know still falls back to the fault below.
  '': () => t('amp.reason.waiting'),
  portBusy: () => t('amp.reason.portBusy'),
  noAnswer: () => t('amp.reason.noAnswer'),
  wrongModel: () => t('amp.reason.wrongModel'),
  malformed: () => t('amp.reason.malformed'),
}
const ALARM: Record<string, () => string> = {
  swrExceedingLimits: () => t('amp.alarm.swrExceedingLimits'),
  amplifierProtection: () => t('amp.alarm.amplifierProtection'),
  inputOverdriving: () => t('amp.alarm.inputOverdriving'),
  excessOverheating: () => t('amp.alarm.excessOverheating'),
  combinerFault: () => t('amp.alarm.combinerFault'),
  fault: () => t('amp.alarm.fault'),
}
const WARNING: Record<string, () => string> = {
  alarmAmplifier: () => t('amp.warning.alarmAmplifier'),
  noSelectedAntenna: () => t('amp.warning.noSelectedAntenna'),
  swrAntenna: () => t('amp.warning.swrAntenna'),
  noValidBand: () => t('amp.warning.noValidBand'),
  powerLimitExceeded: () => t('amp.warning.powerLimitExceeded'),
  overheating: () => t('amp.warning.overheating'),
  atuNotAvailable: () => t('amp.warning.atuNotAvailable'),
  tuningWithNoPower: () => t('amp.warning.tuningWithNoPower'),
  atuBypassed: () => t('amp.warning.atuBypassed'),
  powerSwitchHeldByRemote: () => t('amp.warning.powerSwitchHeldByRemote'),
  combinerOverheating: () => t('amp.warning.combinerOverheating'),
  combinerFault: () => t('amp.warning.combinerFault'),
}

/** Watts, rounded — never a raw float. */
const fmtWatts = (w: number) => `${Math.round(w)} W`
/** An SWR ratio, one decimal, in the form a rig's own meter prints it. */
const fmtSwr = (s: number) => `${s.toFixed(1)}:1`

/**
 * ⚠️ NEVER ROUTE A TEMPERATURE THROUGH `units.ts`. `fmtTempF` assumes FAHRENHEIT input and
 * would render an SPE reading of 33 as `1°C` on a metric install.
 *
 * The scale letter is licensed by `tempCelsius` and by nothing else. Elecraft's `^TM` is
 * documented Celsius, so the KPA gets `52 °C`. SPE's §5 says "Temp in °C or F" — the amplifier
 * reports whatever its own front panel is set to and the wire does not say which — so it gets
 * a bare `41°` with a degree sign and NO letter, plus the tooltip below. A guessed °C is a
 * false statement half the time.
 */
const fmtTemp = (deg: number, celsius: boolean) => (celsius ? `${deg} °C` : `${deg}°`)

/** One label/value cell. `null` value renders the em dash — absence stays absent. */
function Cell({
  k,
  v,
  title,
  tone,
}: {
  k: string
  v: string | null
  title?: string
  tone?: 'alarm' | 'warn'
}) {
  return (
    <div className="amp-cell" title={title}>
      <span className="amp-k">{k}</span>
      <span className={tone ? `amp-v amp-${tone}` : 'amp-v'}>{v ?? DASH}</span>
    </div>
  )
}

/** A big reading: its label, and the value as SVG text sized by the cell. `textLength` holds each
 *  character to 0.6 of the font size, whatever the platform's monospace face measures, so every
 *  reading of the same length draws the same width. */
function Big({ k, v, title }: { k: string; v: string | null; title?: string }) {
  const text = v ?? DASH
  return (
    <div className="amp-cell" title={title}>
      <span className="amp-k">{k}</span>
      <span className="amp-v amp-big">
        <svg
          viewBox={`0 0 ${Math.max(BIG_CHARS, text.length) * 60 + 30} 80`}
          preserveAspectRatio="xMinYMid meet"
          role="img"
          aria-label={text}
        >
          <text x="0" y="76" fontSize="100" textLength={text.length * 60} lengthAdjust="spacingAndGlyphs">
            {text}
          </text>
        </svg>
      </span>
    </div>
  )
}

/** A lit lamp: Operate, the amplifier in line. Drawn, not a glyph, so every platform shows the same shape. */
const LAMP = (
  <svg viewBox="0 0 20 20" aria-hidden="true">
    <circle cx="10" cy="10" r="5" fill="currentColor" />
    <circle cx="10" cy="10" r="8.5" fill="none" stroke="currentColor" strokeWidth="2" />
  </svg>
)
/** A pause mark: Standby, the amplifier out of line. */
const PAUSE = (
  <svg viewBox="0 0 20 20" aria-hidden="true">
    <rect x="4" y="3" width="4.5" height="14" rx="1" fill="currentColor" />
    <rect x="11.5" y="3" width="4.5" height="14" rx="1" fill="currentColor" />
  </svg>
)

/** THE PANE'S ONE CONTROL: Operate/Standby, big and coloured (see the file header for its rules). */
function OperateButton({
  amp,
  operate,
  control: { keyed, usable, refused, send },
}: {
  amp: AmpStatus
  /** `amp.operate`, already gated on the link (`val`): null = we cannot see the state. */
  operate: boolean | null
  control: ReturnType<typeof useAmpControl>
}) {
  const known = operate != null
  // THE PRESS IN FLIGHT (the window's `hold`, above), when it is this amplifier's and not yet given up.
  const current = useSyncExternalStore(subscribeHold, () => hold)
  const mine = current != null && current.amp === ampKey(amp) ? current : null
  const waiting = mine != null && performance.now() < mine.until
  // Ends when the amplifier reports a different state (or none)…
  useEffect(() => {
    if (mine != null && operate !== mine.from) releaseHold(mine)
  }, [operate, mine])
  // …or when it gives up: the time LEFT, so a remount mid-wait keeps the original deadline.
  useEffect(() => {
    if (mine == null) return
    const timer = setTimeout(() => releaseHold(mine), Math.max(0, mine.until - performance.now()))
    return () => clearTimeout(timer)
  }, [mine])
  const press = async () => {
    if (operate == null) return
    // Held from the click, not from the answer: the second click of a double click lands before it.
    const h: Hold = { amp: ampKey(amp), from: operate, until: performance.now() + WAIT_MS }
    setHold(h)
    const queued = await send('operate')
    if (!queued) releaseHold(h)
  }
  // Every title ends on the same warning, because this is the control an operator could reach
  // for in a hurry: Standby does NOT stop a transmission.
  const notStop = t('amp.op.notStop')
  const reason = !amp.linked
    ? (REASON[amp.reason] ?? REASON.noAnswer)()
    : !known
      ? t('amp.strip.statusUnknown')
      : null
  const title =
    reason != null
      ? `${reason} ${notStop}`
      : keyed
        ? `${t('amp.strip.keyed.title')} ${notStop}`
        : operate
          ? t('amp.strip.toStandby.title')
          : `${t('amp.strip.toOperate.title')} ${notStop}`
  const word = !known ? DASH : operate ? t('amp.operate') : t('amp.standby')
  // An unknown state says so on its face; WHY is the box's head line just above, the title and the
  // spoken name (the reason is not repeated on screen a second time).
  const sub = !known ? t('amp.strip.statusUnknown') : operate ? t('amp.op.active') : t('amp.op.notActive')
  const tone = !known ? '' : operate ? ' is-operate' : ' is-standby'
  // Spoken as one phrase ("Operate · Active"); an unknown state is named as unknown, then why
  // (once, when the reason IS that it is unknown).
  const name = known ? `${word}${SEP}${sub}` : amp.linked ? sub : `${sub}${SEP}${reason}`

  return (
    <div className="amp-op-row">
      <button
        type="button"
        className={`amp-opbtn${tone}`}
        disabled={!usable || !known || waiting}
        title={title}
        aria-label={name}
        // The amplifier answers on the next poll; nothing here moves until it does.
        onClick={() => void press()}
      >
        {known && <span className="amp-opbtn-icon">{operate ? LAMP : PAUSE}</span>}
        <span className="amp-opbtn-word">{word}</span>
        <span className="amp-opbtn-sub">{sub}</span>
      </button>
      {refused && (
        <span className="amp-strip-refused" role="status">
          <T k="amp.strip.refused" />
        </span>
      )}
      {waiting && (
        <span className="amp-op-waiting" role="status">
          <T k="amp.op.waiting" />
        </span>
      )}
    </div>
  )
}

export function AmpPane({
  amp,
  radioOnAir,
}: {
  amp: AmpStatus | null | undefined
  /** The RADIO's ON AIR answer (types.ts `isOnAir`), for an amplifier that reports no transmit
   *  flag of its own (Elecraft) or reports idle while the exciter is keyed. */
  radioOnAir: boolean
}) {
  // Called before the early return below: a hook must run on every render.
  const control = useAmpControl(amp, radioOnAir)
  // No amplifier configured on the active radio: the box's one line, and no readout.
  if (amp == null) return <p className="pane-basic">{t('connect.pane.amp.basic')}</p>

  const linked = amp.linked
  // ⭐ EVERY READING IS GATED ON `linked`, not merely on its own presence. The backend already
  // clears each one on the first failed poll — a stale wattage in front of a kilowatt is a
  // fabrication — and this is the second half of the same rule: nothing from a dead link
  // reaches the screen even if a field somehow survived it.
  const val = <T,>(x: T | null | undefined): T | null => (linked && x != null ? x : null)

  const watts = val(amp.outputWatts)
  const swr = val(amp.swr)
  const swrAtu = val(amp.swrAtu)
  const volts = val(amp.volts)
  const amps = val(amp.amps)
  const temp = val(amp.temp)
  const operate = val(amp.operate)

  // ⭐ COLOUR FROM THE AMPLIFIER'S OWN JUDGEMENT, NEVER FROM A THRESHOLD OF OURS. `alarmRaised`
  // counts an alarm letter no firmware we know ships — the decoder's failure direction is
  // toward reporting a fault, not toward silence — and a tag comparison here would undo that.
  // And the temperature is deliberately NOT thresholded or coloured: 60 °C and 60 °F are not
  // the same amplifier state, and we do not know which one an SPE is reporting.
  const alarm = linked && amp.alarmRaised
  const warning = linked && amp.warningRaised

  return (
    <div className="amp-pane">
      <div className="amp-head">
        <span className={linked ? 'amp-link' : 'amp-link amp-down'}>
          {linked ? t('amp.link.up') : (REASON[amp.reason] ?? REASON.noAnswer)()}
        </span>
        {/* The amplifier's own model id, raw. Empty for a KPA, which reports none. */}
        {amp.model !== '' && <span className="amp-model">{amp.model}</span>}
      </div>

      <OperateButton amp={amp} operate={operate} control={control} />

      <div className="amp-big-row">
        <Big k={t('amp.k.power')} v={watts == null ? null : fmtWatts(watts)} />
        <Big k={SWR} v={swr == null ? null : fmtSwr(swr)} title={t('amp.swr.title')} />
      </div>

      <div className="amp-grid">
        {/* Pre-ATU SWR is SPE-only; the cell is simply absent for a KPA rather than an
            eternal '—' that reads as a broken reading. */}
        {amp.swrAtu != null && (
          <Cell
            k={`${SWR} ${ATU}`}
            v={swrAtu == null ? null : fmtSwr(swrAtu)}
            title={t('amp.swrAtu.title')}
          />
        )}
        <Cell
          k={t('amp.k.temp')}
          v={temp == null ? null : fmtTemp(temp, amp.tempCelsius)}
          // The one place the missing unit is explained, and it is explained rather than
          // guessed at.
          title={amp.tempCelsius ? undefined : t('amp.temp.unknownScale')}
        />
        <Cell k={VDC} v={volts == null ? null : `${volts.toFixed(1)} V`} />
        <Cell k={t('amp.k.current')} v={amps == null ? null : `${amps.toFixed(1)} A`} />
      </div>

      {/* FAULTS ARE RENDERED AS FAULTS, including the `unknown` tag — an alarm letter a later
          firmware ships must reach the operator, not go quiet. */}
      {alarm && (
        <p className="amp-fault amp-alarm">
          {(ALARM[amp.alarm] ?? (() => t('amp.alarm.unknown')))()}
          {amp.kpaFault != null && amp.kpaFault !== 0 ? ` (${amp.kpaFault})` : ''}
        </p>
      )}
      {warning && (
        <p className="amp-fault amp-warn">
          {(WARNING[amp.warning] ?? (() => t('amp.warning.unknown')))()}
        </p>
      )}
    </div>
  )
}
