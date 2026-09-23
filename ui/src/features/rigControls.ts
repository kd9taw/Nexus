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
import type { RadioStatus, ReceiversStatus } from '../types'

/** What a control's value IS, which decides how it is drawn and what a reason may say about
 *  it. `dbSteps` exists for ATT/PRE, whose Hamlib caps carry a step list. */
export type ControlKind = 'fraction' | 'hz' | 'enum' | 'toggle' | 'dbSteps'

/** The receive stage a row belongs to — `dualrx::RxStage`'s three, in the snapshot's words:
 *  ATT·PRE·RF·AGC the front end; BW·NB·NR·NRLVL·ANF·MN·NOTCHF the DSP; AF·SQL the audio. */
export type RxStage = 'frontEnd' | 'dsp' | 'audio'

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
  /**
   * ATT/PRE ONLY, and it is the third state of the step list rather than a fault.
   *
   * ⛔ THESE TWO ARE COMMANDED FROM A LIST, NOT SET TO A NUMBER. `set_att_db` rejects any dB
   * that is not one of `radio.attStepsDb`, because a pad the rig does not hold is NAKed or
   * silently rounded to a neighbour and the front end then moves by an amount nobody chose
   * (`observe_rig_db_steps`, service.rs: "`None` = unknown, and the cockpit must then offer
   * nothing rather than a guessed ladder"). So with no list there is no value to offer and
   * the control cannot be drawn live — which is a fact about what NEXUS could read, not
   * about what the radio has.
   *
   * ⚠️ WHICH IS WHY IT IS NOT `absent`. Collapsing it there would print "Not on this radio:
   * ATT" at the pane's foot over a rig that may well have a 20 dB pad and merely answered no
   * `\dump_state` — the confident wrong answer the three-state model exists to stop. It
   * keeps its row, dead, and says which of the two it is.
   */
  | 'noSteps'

/** The catalogue key each cause prints. One key per cause; the control supplies `{plate}`,
 *  `{token}`, `{mode}` or `{model}`. Kept beside the cause union so adding a cause without a
 *  wording does not typecheck. */
export const CAUSE_KEY: Record<UnavailableCause, string> = {
  noCat: 'phone.unavail.noCat',
  notOnMode: 'phone.unavail.notOnMode',
  // `absent` is printed COLLAPSED, as the pane-foot line over a list of plates — see
  // `absentPlates`. It is the one cause with no per-control wording, by design.
  absent: 'phone.chain.absent',
  nativeCiv: 'phone.unavail.nativeCiv',
  backend: 'phone.unavail.backend',
  backendRawOnly: 'phone.unavail.backendRawOnly',
  silent: 'phone.unavail.silent',
  noSteps: 'phone.unavail.noSteps',
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
  /** The `RadioStatus` field whose non-null value means this radio drives it. ABSENT on an
   *  unbuilt control, because there is no field yet — pointing one at a neighbour's field to
   *  satisfy the type is how a slot quietly starts answering for the wrong thing. */
  field?: keyof RadioStatus
  chain: 'rx' | 'tx'
  /**
   * RECEIVE ROWS ONLY: the receive stage this control belongs to — the key a receiver's own
   * capability joins on (dual-receiver ruling D7: capabilities are PER RECEIVER). The mapping is
   * `dualrx::RxStage`'s table in the Rust crate, and a test reads that table and holds this one
   * to it row for row. A transmit row has none: the radio has one transmitter.
   */
  stage?: RxStage
  /**
   * HAS NEXUS BUILT A PATH FOR THIS CONTROL ON THE **SUB** RECEIVER? The per-receiver twin of
   * `built` below, with the same teeth: `false` (or absent) = no Sub row and no mention, because
   * "not confirmed for the sub receiver" would be a claim about the radio made on behalf of a
   * path Nexus never built. Only the rows whose Sub command reaches the Sub carry it.
   */
  subBuilt?: boolean
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
  // ⚠️ BW HAS NO CAPABILITY FIELD, and that is not an omission. `filterWidthHz` null means
  // "unknown or the rig's default" — the stepper still commands the rig perfectly well from
  // its 2.4 kHz base and only the READOUT is blank, so treating a missing read-back as "this
  // radio has no filter" would collapse a control that works into the foot line.
  { id: 'BW', plate: 'BW', token: { hamlib: 'PASSBAND' }, kind: 'hz', chain: 'rx', stage: 'dsp', built: true },
  // ⭐ BUILT 2026-09-22, and their capability answer is the STEP LIST, never the read-back.
  // `attStepsDb` / `preampStepsDb` are what this radio declared in `\dump_state`, so the
  // three states fall straight out of it: a list with entries → chips; an EMPTY list → the
  // rig positively has no pad fitted, which is `absent`; no list at all → `noSteps`.
  // `field` is still declared, because the READING is what the active chip is drawn from.
  { id: 'ATT', plate: 'ATT', token: { hamlib: 'ATT' }, kind: 'dbSteps', field: 'attDb', chain: 'rx', stage: 'frontEnd', built: true },
  { id: 'PRE', plate: 'PRE', token: { hamlib: 'PREAMP' }, kind: 'dbSteps', field: 'preampDb', chain: 'rx', stage: 'frontEnd', built: true },
  { id: 'RF', plate: 'RF', token: { hamlib: 'RF' }, kind: 'fraction', field: 'rfGain', chain: 'rx', stage: 'frontEnd', built: true, subBuilt: true },
  { id: 'NB', plate: 'NB', token: { hamlib: 'NB' }, kind: 'toggle', field: 'nb', chain: 'rx', stage: 'dsp', built: true },
  { id: 'NR', plate: 'NR', token: { hamlib: 'NR' }, kind: 'toggle', field: 'nr', chain: 'rx', stage: 'dsp', built: true },
  { id: 'NRLVL', plate: 'NR', token: { hamlib: 'NR' }, kind: 'fraction', field: 'nrLevel', chain: 'rx', stage: 'dsp', built: true },
  { id: 'ANF', plate: 'Auto notch', token: { hamlib: 'ANF' }, kind: 'toggle', field: 'notch', chain: 'rx', stage: 'dsp', built: true },
  { id: 'MN', plate: 'Manual notch', token: { hamlib: 'MN' }, kind: 'toggle', field: 'manualNotch', chain: 'rx', stage: 'dsp', built: true },
  { id: 'NOTCHF', plate: 'NOTCH', token: { hamlib: 'NOTCHF' }, kind: 'hz', field: 'notchFreqHz', chain: 'rx', stage: 'dsp', built: true },
  { id: 'AGC', plate: 'AGC', token: { hamlib: 'AGC' }, kind: 'enum', field: 'agc', chain: 'rx', stage: 'frontEnd', built: true },
  { id: 'AF', plate: 'AF', token: { hamlib: 'AF' }, kind: 'fraction', field: 'afGain', chain: 'rx', stage: 'audio', built: true, subBuilt: true },
  { id: 'SQL', plate: 'SQL', token: { hamlib: 'SQL' }, kind: 'fraction', field: 'squelch', chain: 'rx', stage: 'audio', built: true, subBuilt: true },
  { id: 'MIC', plate: 'Mic', token: { hamlib: 'MICGAIN' }, kind: 'fraction', field: 'micGain', chain: 'tx', built: true },
  { id: 'COMP', plate: 'COMP', token: { hamlib: 'COMP' }, kind: 'toggle', field: 'comp', chain: 'tx', built: true },
  { id: 'COMPLVL', plate: 'COMP', token: { hamlib: 'COMP' }, kind: 'fraction', field: 'compLevel', chain: 'tx', built: true },
  { id: 'VOX', plate: 'VOX', token: { hamlib: 'VOX' }, kind: 'toggle', field: 'vox', chain: 'tx', built: true },
  // ⚠️ MON IS A TRANSMIT CONTROL AND STAYS ONE. It is the rig playing YOUR OWN audio back
  // while you talk (`setMonitorGain`, api.ts), heard only while transmitting — so it shapes
  // what the over sounds like, not what the receiver hears, and it belongs beside MIC/COMP.
  { id: 'MON', plate: 'MON', token: { hamlib: 'MONITOR_GAIN' }, kind: 'fraction', field: 'monitorGain', chain: 'tx', built: true },
]

/**
 * ⚠️ THE CAPABILITY MODEL DOES NOT EXIST YET. Parsing Hamlib's `\dump_state` into
 * `RadioStatus.caps` is a BACKEND job and step 1 of the programme; nothing on the snapshot
 * carries it today. This is the shape the DTO should carry, declared here so the backend can
 * match it rather than the other way round.
 *
 * ⭐ POSITIVE MASKS, NEVER A "LACKS" LIST, and the reason is the ruling's first exception
 * wearing a different hat. A negative list requires the backend to know the full universe of
 * control ids in order to state an absence — so a control added next month would be missing
 * from every rig's list, and every radio in the fleet would report "your radio does not have
 * this" about something Nexus had only just built. What `\dump_state` actually gives is what
 * the rig HAS, so that is what crosses the wire.
 *
 * ⛔ AND THE THREE STATES MUST SURVIVE THE CROSSING, because a bare mask cannot express them
 * and the reasons depend on the difference:
 *   PRESENT — the token is in the mask. Drivable.
 *   ABSENT  — the mask EXISTS and the token is not in it. The backend genuinely does not
 *             offer it, which is a fact worth printing.
 *   UNKNOWN — no caps at all (no `\dump_state` yet), or that particular mask is missing.
 *             Nothing may be claimed; the cockpit falls back to "has this radio ever
 *             reported it", which is what it has always done.
 * Collapsing UNKNOWN into ABSENT is the same class of error as resolving an unread repeater
 * shift to simplex: a default that manufactures a confident wrong answer.
 */
export interface RigCapsDto {
  /** Hamlib func tokens the rig reports as readable / settable (`NB`, `ANF`, `VOX`…). */
  funcGet?: readonly string[]
  funcSet?: readonly string[]
  /** Hamlib level tokens, likewise (`RF`, `AF`, `NOTCHF`, `AGC`…). */
  levelGet?: readonly string[]
  levelSet?: readonly string[]
  /**
   * The attenuator and preamp STEP LISTS in dB, straight from `\dump_state` — the live
   * `RadioStatus.attStepsDb` / `preampStepsDb`, which is where the engine puts them
   * (`observe_rig_db_steps`). Ascending, and WITHOUT the implicit 0 (off) every rig has.
   *
   * ⚠️ AN EMPTY LIST MEANS NO PAD FITTED — corrected 2026-09-22 against the shipped
   * backend, which this comment had wrong in the other direction ("the rig has the stage but
   * no step list, so a plain dB stepper"). Three sources say otherwise and none says that:
   * `Engine::observe_rig_db_steps` ("Empty is the different, positive answer: no pad
   * fitted"), `RadioStatus.attStepsDb` in types.ts (the same sentence), and the CI-V broker,
   * which returns None for both the read and the write of PREAMP when the model's list is
   * empty (broker.rs) — an IC-905 has no preamp at all. There is no path that produces an
   * empty list for a rig that HAS the stage, so a bare dB stepper would have had nothing to
   * step and no value the rig would accept.
   *
   *   non-empty ⇒ PRESENT, and these are the only values that may be commanded
   *   EMPTY     ⇒ ABSENT — the rig declared it has none
   *   absent    ⇒ UNKNOWN — nothing was declared, so nothing may be claimed (`noSteps`)
   */
  attDb?: readonly number[]
  preampDb?: readonly number[]
}

export type CapState = 'present' | 'absent' | 'unknown'

/**
 * What the caps masks say about ONE control — the three-state derivation, kept here rather
 * than asked of the DTO.
 *
 * It reads the SET mask, because the question this answers is "may the operator drive it".
 * The GET mask is a different question (whether a value can be read BACK, which decides
 * `phone.prov.rig` versus `phone.prov.cmd`) and is deliberately not folded in here.
 */
export function capStateFor(c: RigControl, caps?: RigCapsDto): CapState {
  if (!caps || !c.built) return 'unknown'
  // ⭐ ATT/PRE ANSWER FROM THEIR STEP LIST, not from a level mask, and that IS this
  // function's question: the list is the set of values the operator may drive this control
  // to. A rig can be in the `ATT` level mask and still be undrivable here — a mask says the
  // token exists, a list says which pads do — so the list is the stricter and the right one.
  if (c.kind === 'dbSteps') {
    const steps = stepsFor(c, caps)
    if (!steps) return 'unknown'
    return steps.length > 0 ? 'present' : 'absent'
  }
  const mask = c.kind === 'toggle' ? caps.funcSet : caps.levelSet
  // The mask itself missing is UNKNOWN, not an empty set of capabilities.
  if (!mask) return 'unknown'
  return mask.includes(c.token.hamlib) ? 'present' : 'absent'
}

/** The dB step list for a stepped control (ATT/PRE), or `undefined` when the radio declared
 *  nothing. EMPTY is the different, positive answer — no pad fitted — and the two must stay
 *  distinguishable here, because `capStateFor` reads one as ABSENT and the other as UNKNOWN. */
export function stepsFor(c: RigControl, caps?: RigCapsDto): readonly number[] | undefined {
  if (!caps) return undefined
  return c.id === 'ATT' ? caps.attDb : c.id === 'PRE' ? caps.preampDb : undefined
}

/** What the resolver needs to know. `reported` is the cockpit's STICKY answer — reported now
 *  or at some point on THIS radio — never the raw `!= null`, which a QSY blanks. */
export interface ControlState {
  catOk: boolean
  /** Has this radio ever reported this control's field, on this radio. */
  reported: (c: RigControl) => boolean
  /** The mode the next over goes out in, for `notOnMode`. */
  mode: string
  /** The rig's own capability masks, when the backend supplies them. Absent ⇒ every control
   *  is UNKNOWN and the sticky "has ever reported" answer stands, which is today's world. */
  caps?: RigCapsDto
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
  // THE MASKS FIRST, because they are the sounder source: they say what the rig HAS rather
  // than what a poll has happened to bring back. `present` settles it even if no value has
  // arrived yet; `absent` settles it without waiting for a probe that will never answer.
  const cap = capStateFor(c, s.caps)
  if (cap === 'present') return null
  if (cap === 'absent') return 'absent'
  // UNKNOWN — no caps, or no such mask. Fall back to what the cockpit has observed, which
  // is all this has ever had. ⚠️ Never treat unknown as absent: that is the default that
  // manufactures a confident wrong answer.
  //
  // ⛔ AND FOR ATT/PRE THE OBSERVED FALLBACK CANNOT RESCUE IT. Everything below this line
  // reasons "the rig has answered about this before, so offer it" — sound for a control you
  // set to a NUMBER, and wrong for one you pick from a LIST. A rig can report `attDb: 0`
  // (pad off) all day and still have told us no step list, and there is then no value the
  // control could command: `set_att_db` rejects anything not in `attStepsDb`. So it keeps
  // its row and says WHICH unknown this is, rather than drawing a live control with no
  // chips in it or claiming the radio has no pad.
  if (c.kind === 'dbSteps') return 'noSteps'
  // A BUILT control with no capability field has no per-radio signal at all — BW is the one
  // today. Nothing left to disqualify it, so it is available.
  if (!c.field) return null
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

// ── THE RECEIVER AXIS — the same table, drawn for the SUB ──────────────────────────────────
//
// ⭐ MAIN'S ANSWERS ARE `causeFor`'s AND NOTHING HERE TOUCHES THEM. A radio with one receiver
// is drawn exactly as it always was; everything below answers only "may this row be drawn for
// the Sub, and if not, why".
//
// The Sub's answer rests on four facts, and each has its own cause so no two failures read
// alike: is a Sub OFFERED at all (the snapshot's `receivers.sub`), has Nexus BUILT a Sub path
// for this row (`subBuilt`), may the Sub be CREDITED with this row's stage (D7 — the snapshot's
// per-receiver `stages`), and can the CAT path serving the radio NAME the Sub (the loop's
// `subCommandable`). Then the ordinary one: is CAT up.

/** Why a row is not drawn live for the Sub. */
export type SubCause =
  /** No Sub in the snapshot — UNKNOWN, ABSENT or a second receiver this build does not offer
   *  alike (D3). Nothing Sub is drawn. */
  | 'noSub'
  /** Nexus has no Sub path for this row, or it is a transmit row. No row, and NO mention. */
  | 'notBuilt'
  /** D7: no vendor statement credits the Sub with this row's stage. Not offered — and named
   *  as NOT CONFIRMED, never as absent: UNKNOWN is never a "no". */
  | 'stageUnknown'
  /** The stage is ONE physical stage serving both receivers. A Sub control here would move
   *  Main's, so it is not the Sub's to offer. (No offered radio reaches this in v1.) */
  | 'stageShared'
  /** The CAT path serving this radio cannot name the Sub (Hamlib, OmniRig). */
  | 'noRoute'
  /** The radio loop has not said yet whether it can. Offers nothing until it has. */
  | 'routeUnknown'
  /** No CAT link. The row stays, dead; the pane already says why once. */
  | 'noCat'

/** What the Sub's answer needs. `receivers` absent = a station that predates the field. */
export interface SubState {
  catOk: boolean
  receivers?: ReceiversStatus | null
}

/** THE ONE DECISION for a Sub row: why it cannot be driven, or `null` when it can. */
export function subCauseFor(c: RigControl, s: SubState): SubCause | null {
  const sub = s.receivers?.sub
  if (!sub) return 'noSub'
  if (c.chain !== 'rx' || !c.stage || !c.subBuilt) return 'notBuilt'
  const owner = sub.stages[c.stage]
  // ⛔ ANYTHING BUT A POSITIVE `own` IS NOT AN OFFER — and an unrecognised word is treated as
  // the unknown it is, never as ownership.
  if (owner === 'sharedWithMain') return 'stageShared'
  if (owner !== 'own') return 'stageUnknown'
  if (!s.catOk) return 'noCat'
  if (s.receivers?.subCommandable === false) return 'noRoute'
  if (s.receivers?.subCommandable !== true) return 'routeUnknown'
  return null
}

/** Does this row get drawn for the Sub — live, or dead behind the pane's no-CAT banner. */
export function subRendersRow(c: RigControl, s: SubState): boolean {
  const cause = subCauseFor(c, s)
  return cause === null || cause === 'noCat'
}

/** The rows the Sub strip draws, in registry (signal) order. */
export function subChainControls(s: SubState): RigControl[] {
  return RIG_CONTROLS.filter((c) => c.chain === 'rx' && subRendersRow(c, s))
}

/** The plates of rows Nexus built for the Sub whose stage no vendor statement credits to it —
 *  the Sub strip's "not confirmed" line. Never mixed into "not on this radio". */
export function subUnconfirmedPlates(s: SubState): string[] {
  return RIG_CONTROLS.filter((c) => subCauseFor(c, s) === 'stageUnknown').map((c) => c.plate)
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
