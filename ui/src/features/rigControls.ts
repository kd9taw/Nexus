// THE RIG-CONTROL REGISTRY — one row per control the Phone cockpit can offer, and one place
// that decides whether it may be offered right now and what to say when it may not.
//
// ⭐ WHY A REGISTRY AND NOT TWENTY TERNARIES. The 2026-09-20 ruling (a control the radio
// cannot drive stays on screen, disabled, WITH THE REASON) turns every control into a small
// state machine: which wire token it rides, which snapshot field says the radio has it, which
// of several causes is the one to name, and whether the reason belongs on the row, in a
// pane-level banner, or in one collapsed line at the pane's foot. Written inline that is
// twenty copies of the same decision, each free to drift — and drifting wordings are the
// exact defect the ⊘ mark exists to prevent.
//
// Pure data and pure functions, no JSX: the same discipline `panelHost` keeps, and for the
// same reason — the decisions stay trivially testable and cannot mutate anything.
import type { RadioStatus } from '../types'

/** What a control's value IS, which decides how it is drawn and what a reason may say about
 *  it. `dbSteps` exists for ATT/PRE, whose Hamlib caps carry a step list. */
export type ControlKind = 'fraction' | 'hz' | 'enum' | 'toggle' | 'dbSteps'

/**
 * WHY a control cannot be driven. ONE ENTRY PER CAUSE — the catalogue key is chosen from
 * this and the control interpolates its own plate and token, so two controls failing the
 * same way cannot describe it two different ways.
 *
 * ⚠️ FOUR OF THESE CANNOT FIRE YET, and they are here rather than omitted so the shape is
 * settled before the data lands (see `RigCaps`). `causeFor` never returns them today, and
 * `rigControls.test.ts` pins that — an unreachable cause that quietly became reachable with
 * no wording behind it is worse than a missing one.
 */
export type UnavailableCause =
  /** No CAT link at all. The PANE says this once as a banner; rows never repeat it. */
  | 'noCat'
  /** The control does not apply on the mode in use — BW on FM, whose passband is fixed. */
  | 'notOnMode'
  /** This radio family does not have it. Collapses to ONE line at the pane's foot. */
  | 'absent'
  /** The daemon path does not carry the token yet (Hamlib does). NOT YET DETECTABLE. */
  | 'nativeCiv'
  /** The rig's Hamlib backend exposes no such token. NOT YET DETECTABLE (needs the model). */
  | 'backend'
  /** The backend has it as a raw position, not a frequency. NOT YET DETECTABLE. */
  | 'backendRawOnly'
  /** The radio is not answering reads on this token. NOT YET DETECTABLE (needs an age). */
  | 'silent'

/** The catalogue key each cause prints. One key per cause; the control supplies `{plate}`,
 *  `{token}`, `{mode}` or `{model}`. Kept beside the cause union so adding a cause without a
 *  wording does not typecheck. */
export const CAUSE_KEY: Record<UnavailableCause, string> = {
  noCat: 'phone.unavail.noCat',
  notOnMode: 'phone.unavail.notOnMode',
  absent: 'phone.unavail.absent',
  nativeCiv: 'phone.unavail.nativeCiv',
  backend: 'phone.unavail.backend',
  backendRawOnly: 'phone.unavail.backendRawOnly',
  silent: 'phone.unavail.silent',
}

export interface RigControl {
  /** Stable id — the row's `data-chain` anchor and the key everything else joins on. */
  id: string
  /** The rig's own plate, as printed on a front panel. INVARIANT in every locale. */
  plate: string
  /** The wire token per path, for the reasons that name it. A reason that says "your backend
   *  does not expose it" is only useful if it says WHAT, in the words the backend uses. */
  token: { hamlib: string; civ?: string }
  kind: ControlKind
  /** The `RadioStatus` field whose non-null value means this radio drives it. */
  field: keyof RadioStatus
  chain: 'rx' | 'tx'
  /**
   * ⛔ HAS NEXUS BUILT A PATH FOR THIS AT ALL? `false` = the row is OMITTED everywhere — not
   * drawn, and NOT named in the pane-foot line either, because "not on this radio" would be
   * false about a control no radio can reach.
   *
   * This is the first of the ruling's three exceptions and it is a density rule with teeth:
   * a row greyed out on every radio in the fleet reads as "broken" to a thousand operators,
   * not as "not built yet". The entry still lives here so the field slots in the day the
   * path lands, and the row appears then.
   */
  built: boolean
}

/**
 * The receive chain, in signal order, then the transmit chain. ⚠️ ORDER IS THE CONTRACT —
 * the panes render in this order and `PhoneCockpit.chain.test.tsx` pins it off the DOM.
 */
export const RIG_CONTROLS: readonly RigControl[] = [
  { id: 'BW', plate: 'BW', token: { hamlib: 'PASSBAND' }, kind: 'hz', field: 'filterWidthHz', chain: 'rx', built: true },
  // ⛔ NOT BUILT: a sibling is adding the attenuator, preamp and TX monitor now. They have
  // four states each (a caps bit with a step list → chips; the bit with an empty list → a dB
  // stepper; the bit clear → `backend`; a daemon without the token → `nativeCiv`), which is
  // why they are `dbSteps` and why the slots are declared before the fields exist.
  { id: 'ATT', plate: 'ATT', token: { hamlib: 'ATT' }, kind: 'dbSteps', field: 'rfGain', chain: 'rx', built: false },
  { id: 'PRE', plate: 'PRE', token: { hamlib: 'PREAMP' }, kind: 'dbSteps', field: 'rfGain', chain: 'rx', built: false },
  { id: 'RF', plate: 'RF', token: { hamlib: 'RF' }, kind: 'fraction', field: 'rfGain', chain: 'rx', built: true },
  { id: 'NB', plate: 'NB', token: { hamlib: 'NB' }, kind: 'toggle', field: 'nb', chain: 'rx', built: true },
  { id: 'NR', plate: 'NR', token: { hamlib: 'NR' }, kind: 'toggle', field: 'nr', chain: 'rx', built: true },
  { id: 'NRLVL', plate: 'NR', token: { hamlib: 'NR' }, kind: 'fraction', field: 'nrLevel', chain: 'rx', built: true },
  { id: 'ANF', plate: 'Auto notch', token: { hamlib: 'ANF' }, kind: 'toggle', field: 'notch', chain: 'rx', built: true },
  { id: 'MN', plate: 'Manual notch', token: { hamlib: 'MN' }, kind: 'toggle', field: 'manualNotch', chain: 'rx', built: true },
  { id: 'NOTCHF', plate: 'NOTCH', token: { hamlib: 'NOTCHF' }, kind: 'hz', field: 'notchFreqHz', chain: 'rx', built: true },
  { id: 'AGC', plate: 'AGC', token: { hamlib: 'AGC' }, kind: 'enum', field: 'agc', chain: 'rx', built: true },
  { id: 'AF', plate: 'AF', token: { hamlib: 'AF' }, kind: 'fraction', field: 'afGain', chain: 'rx', built: true },
  { id: 'SQL', plate: 'SQL', token: { hamlib: 'SQL' }, kind: 'fraction', field: 'squelch', chain: 'rx', built: true },
  { id: 'MIC', plate: 'Mic', token: { hamlib: 'MICGAIN' }, kind: 'fraction', field: 'micGain', chain: 'tx', built: true },
  { id: 'COMP', plate: 'COMP', token: { hamlib: 'COMP' }, kind: 'toggle', field: 'comp', chain: 'tx', built: true },
  { id: 'COMPLVL', plate: 'COMP', token: { hamlib: 'COMP' }, kind: 'fraction', field: 'compLevel', chain: 'tx', built: true },
  { id: 'VOX', plate: 'VOX', token: { hamlib: 'VOX' }, kind: 'toggle', field: 'vox', chain: 'tx', built: true },
  { id: 'MON', plate: 'MON', token: { hamlib: 'MONITOR_GAIN' }, kind: 'fraction', field: 'micGain', chain: 'tx', built: false },
]

/**
 * ⚠️ THE CAPABILITY MODEL DOES NOT EXIST YET. Parsing Hamlib's `\dump_state` into
 * `RadioStatus.caps` is a BACKEND job and step 1 of the programme; nothing on the snapshot
 * carries it today. This is the shape this file assumes, declared here so the backend can
 * match it rather than the other way round:
 *
 *   caps.lacks — control ids the MODEL TABLE says this radio family does not have. That is
 *                the only sound discriminator between "this rig cannot" and "we have not
 *                managed to read it yet", and it is what the pane-foot collapse wants.
 *   caps.steps — the step list for a `dbSteps` control. Present and non-empty ⇒ chips;
 *                present and EMPTY ⇒ a plain dB stepper; absent ⇒ the bit is clear.
 *
 * Until it arrives `capsLacks` answers from the snapshot alone: a field this radio has never
 * reported. That conflates a family-wide absence with a probe that has not landed, which is
 * why the cockpit holds a per-radio "has ever reported" memory in front of it — a momentary
 * null must never reach this.
 */
export interface RigCaps {
  lacks?: readonly string[]
  steps?: Readonly<Record<string, readonly number[]>>
}

/** What the resolver needs to know. `reported` is the cockpit's STICKY answer — reported now
 *  or at some point on THIS radio — never the raw `!= null`, which a QSY blanks. */
export interface ControlState {
  catOk: boolean
  /** Has this radio ever reported this control's field, on this radio. */
  reported: (c: RigControl) => boolean
  /** The mode the next over goes out in, for `notOnMode`. */
  mode: string
  caps?: RigCaps
}

/**
 * THE ONE DECISION: why this control cannot be driven, or `null` when it can.
 *
 * Order is the argument. A dead CAT link explains every control at once, so it wins and the
 * pane says it once. Mode-inapplicability is next because it is true of a radio that is
 * otherwise perfectly reachable. Absence is last: it is the weakest inference here, resting
 * on "never reported" until the caps model lands.
 */
export function causeFor(c: RigControl, s: ControlState): UnavailableCause | null {
  if (!c.built) return 'absent'
  if (!s.catOk) return 'noCat'
  // BW is the only mode-inapplicable control today: FM's passband is fixed, so there is no
  // width to set — a fact about the MODE, not a fault in the radio, and the row keeps its
  // slot rather than disappearing into the foot line with the things the rig cannot do.
  if (c.id === 'BW' && s.mode === 'FM') return 'notOnMode'
  if (s.caps?.lacks?.includes(c.id)) return 'absent'
  return s.reported(c) ? null : 'absent'
}

/**
 * ⛔ DOES THIS CONTROL GET A ROW AT ALL?
 *
 * The second and third of the ruling's exceptions, which are what stop disabled-with-reason
 * turning a cockpit into a wall of grey:
 *   · `built: false` — no path on any radio. No row, and no mention anywhere (see the field).
 *   · `absent` — this radio family does not have it. No row; its PLATE goes in one line at
 *     the pane's foot instead, so nothing vanishes silently and an IC-7300 does not open to
 *     four grey rows.
 * Everything else keeps its row, disabled, with its reason — which is the ruling proper.
 */
export function rendersRow(c: RigControl, s: ControlState): boolean {
  return causeFor(c, s) !== 'absent'
}

/** The plates for the pane-foot line — "Not on this radio: Contour · SHIFT · MON". Only
 *  controls Nexus HAS built: naming an unbuilt one would blame the radio for Nexus. */
export function absentPlates(chain: 'rx' | 'tx', s: ControlState): string[] {
  return RIG_CONTROLS.filter((c) => c.chain === chain && c.built && causeFor(c, s) === 'absent').map(
    (c) => c.plate,
  )
}

/** The controls that draw a row, in registry order. */
export function chainControls(chain: 'rx' | 'tx', s: ControlState): RigControl[] {
  return RIG_CONTROLS.filter((c) => c.chain === chain && rendersRow(c, s))
}

/**
 * THE A11Y HALF OF "DISABLED AND SAID", and the two kinds are not the same shape.
 *
 * ⚠️ A `disabled` BUTTON IS OUT OF THE TAB ORDER, so a keyboard or screen-reader operator
 * never lands on it and never hears the reason — the whole point of saying it. Buttons
 * therefore use `aria-disabled` and stay focusable, and the caller MUST guard the handler
 * (see `guard`): aria-disabled does not prevent activation on its own.
 *
 * ⚠️ A RANGE INPUT CANNOT DO THAT. `aria-disabled` leaves it draggable, and a slider that
 * moves the thumb while commanding nothing is a worse lie than an inert one — so inputs keep
 * the real `disabled` attribute and their reason is reached through the pane's banner or its
 * foot line, both of which are ordinary text in the pane.
 */
export function deadControlProps(
  kind: 'button' | 'input',
  describedBy?: string,
): { disabled?: true; 'aria-disabled'?: true; 'aria-describedby'?: string } {
  return kind === 'button'
    ? { 'aria-disabled': true, 'aria-describedby': describedBy }
    : { disabled: true, 'aria-describedby': describedBy }
}

/** Swallow a handler on an `aria-disabled` control, which is still clickable without it. */
export function guard<T extends unknown[]>(dead: boolean, fn: (...a: T) => void) {
  return dead ? () => {} : fn
}
