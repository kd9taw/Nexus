// ENTER SENDS MESSAGE — the step table. What Enter in a contest strip would send, what it would
// log, and what it refuses, as N1MM Logger+'s ESM does it. Pure: no rig, no engine, no DOM.
//
// ⚠️ NOTHING HERE TRANSMITS. ESM makes Enter a new way to start a transmission (its rules are
// signed, operator 2026-10-09), so the contest strip gathers its state and the cockpit's transmit
// guards, asks this file, and carries the answer out through the cockpit's OWN send path, which
// keeps every check it has (`features/esmHost.ts`). Nothing here weakens one: ESM's checks come
// first, so it can refuse a step before it logs anything, and the send path checks again.
//
// ⭐ THE CONTACT LOGS AT THE PRESS, AS N1MM'S DOES (operator, 2026-10-08). The press that sends
// TU (Run) or my exchange (S&P) logs the contact in the same breath; a Stop that cuts the message
// afterwards takes nothing back. What makes that safe is the other half: a message ESM sent
// EARLIER in the contact and the operator stopped counts as not sent, so ESM sends it again
// rather than logging on top of it.
//
// The state is session state and never persisted: a remount starts in S&P with nothing sent.
// When ESM does not know whether the exchange went out, it sends it again rather than logs — at
// worst the runner hears the exchange twice.
import type { ContestDupeVerdict } from './contestDupe'
import { resolveEsmRole, type EsmRoleMap, type EsmSlot } from './esmRoles'
import type { MacroKey } from './macroSets'

/** Running (calling CQ) or Search and Pounce (answering someone else's CQ). */
export type EsmMode = 'run' | 'sp'

/** The messages ESM sends, by what they do in the contact — never by key. Which key holds each
 *  is the set's role table (`esmRoles.ts`).
 *
 *  `cq` CQ · `callExch` his call and my exchange · `tu` TU, the end of a Run contact ·
 *  `myCall` my call · `exch` my exchange in S&P · `again` a request for a repeat.
 *
 *  There is no QSO B4: on a dupe of the operator's own log ESM sends nothing (operator,
 *  2026-10-08), so no step would ever send one. */
export type EsmRole = 'cq' | 'callExch' | 'tu' | 'myCall' | 'exch' | 'again'

/** Every role, in the order a contact meets them. */
export const ESM_ROLES: readonly EsmRole[] = ['cq', 'callExch', 'tu', 'myCall', 'exch', 'again']

/** The three cockpits ESM lives in. FT and every other mode never have it. */
export type EsmCockpit = 'cw' | 'rtty' | 'phone'

/** What ESM remembers about the contact in the strip. */
export interface EsmState {
  mode: EsmMode
  /** The call this contact's exchange went out to (Run), or null. The exchange counts as sent
   *  only while the strip still holds THAT call, so a busted call corrected after the exchange
   *  went out gets the exchange again. */
  exchTo: string | null
  /** The call my call went out to (S&P with "call once"), or null. Per call, as above. */
  myCallTo: string | null
}

/** What the strip holds at the press. */
export interface EsmStrip {
  /** The strip's call. */
  call: string
  /** Every exchange box passes the check the log itself uses, judged after Enter commits the box
   *  it was pressed in, the way Space does — so a county typed by name counts as its code. */
  exchangeComplete: boolean
  /** `contestDupe`'s verdict on the call. Only `own` stops ESM: a club dupe is a warning. */
  dupe: ContestDupeVerdict
}

/** Where the caret goes after the press: the Call box, the first exchange box, or nowhere.
 *  Only ever between the strip's own boxes, and only on a press made inside the strip. */
export type EsmCaret = 'call' | 'ex0' | 'stay'

/** Why Enter sent nothing and logged nothing. The strip says it in words. */
export type EsmRefusal =
  /** An own dupe. The strip's own-dupe sentence is already on screen. */
  | { why: 'dupe' }
  /** TX is off. Enter never turns it on: the operator does, by a deliberate act. */
  | { why: 'txOff' }
  /** Outside the licence privileges, or a connection that refuses TX. */
  | { why: 'txLocked' }
  /** A clock repair holds transmit. */
  | { why: 'clockRepair' }
  /** Phone: the keyer is recording. */
  | { why: 'recording' }
  /** Phone: the operator is holding PTT. */
  | { why: 'pttHeld' }
  /** Phone: the radio has the mic (Nexus's own Flex audio), so a recording would not go out. */
  | { why: 'radioHasMic' }
  /** The set has no message for this step: no key is mapped to it. */
  | { why: 'unmapped'; role: EsmRole }
  /** The key this step is on sends nothing (an empty macro, a voice slot with no recording). */
  | { why: 'empty'; role: EsmRole; key: MacroKey }
  /** Phone: this step is mapped to two recordings, and the keyer plays one per press. */
  | { why: 'oneSlot'; role: EsmRole }

/** One Enter press, decided.
 *
 *  `send` — send `role`'s message; when `log`, the contact logs at this same press. `next` is
 *  ESM's state once the message is on its way: kept only if the cockpit's send path took it.
 *  `speak` — Phone, Run: nothing plays. A recording cannot say a callsign, so the operator says
 *  his call and the exchange, and the press counts them as said.
 *  `refuse` — an own dupe: nothing is sent and nothing logs. */
export type EsmStep =
  | { kind: 'send'; role: EsmRole; log: boolean; caret: EsmCaret; next: EsmState }
  | { kind: 'speak'; caret: EsmCaret; next: EsmState }
  | { kind: 'refuse'; refusal: { why: 'dupe' } }

/** What ESM follows besides Enter.
 *
 *  `cq` — the operator called CQ by hand (the key the set's CQ is on): Run.
 *  `spot` — a spot click handed a station to the strip: S&P.
 *  `toggle` — the Run/S&P plate.
 *  `stopped` — Esc, Stop TX or the watchdog cut a message: what went out counts as not sent.
 *  `cleared` — the strip emptied (logged by hand, wiped): a new contact, nothing sent. */
export type EsmEvent = 'cq' | 'spot' | 'toggle' | 'stopped' | 'cleared'

/** A fresh strip: S&P, nothing sent (operator, 2026-10-08). Every remount starts here. */
export function esmStart(): EsmState {
  return { mode: 'sp', exchTo: null, myCallTo: null }
}

/** `state` after `event`. Switching between Run and S&P keeps what went out to the call in the
 *  strip; only a stop or a new contact forgets it. */
export function esmEvent(state: EsmState, event: EsmEvent): EsmState {
  switch (event) {
    case 'cq':
      return { ...state, mode: 'run' }
    case 'spot':
      return { ...state, mode: 'sp' }
    case 'toggle':
      return { ...state, mode: state.mode === 'run' ? 'sp' : 'run' }
    case 'stopped':
    case 'cleared':
      return { mode: state.mode, exchTo: null, myCallTo: null }
  }
}

/** The step table: what this Enter press sends and logs, before any transmit guard is asked.
 *
 *  Run: R1 no call → CQ · R2 a new call, exchange incomplete → his call and my exchange, caret to
 *  the first box · R3 incomplete, exchange already sent → AGN · R4 complete, not sent → his call
 *  and my exchange, no log · R5 complete and sent → TU, and the contact logs · R6 own dupe →
 *  nothing. S&P: S1 no call → my call · S2 incomplete → my call (with "call once": the first
 *  press also moves the caret to the first box, later presses send AGN) · S3 complete → my
 *  exchange, and the contact logs · S4 own dupe → nothing.
 *
 *  In Phone, R2 and R4 play nothing: the operator says his call and the exchange (operator,
 *  2026-10-08), as N1MM advises for a live exchange. */
export function esmStep(
  state: EsmState,
  strip: EsmStrip,
  opts: { cockpit: EsmCockpit; callOnce: boolean },
): EsmStep {
  const call = strip.call.trim().toUpperCase()
  if (strip.dupe === 'own') return { kind: 'refuse', refusal: { why: 'dupe' } }
  const send = (role: EsmRole, log: boolean, caret: EsmCaret, next: EsmState): EsmStep => ({
    kind: 'send',
    role,
    log,
    caret,
    next,
  })
  // The contact logs at this press, so the strip is a new contact from here on.
  const logged: EsmState = { mode: state.mode, exchTo: null, myCallTo: null }
  if (state.mode === 'run') {
    if (!call) return send('cq', false, 'call', state)
    const sent = state.exchTo === call
    if (sent && strip.exchangeComplete) return send('tu', true, 'call', logged)
    if (sent) return send('again', false, 'stay', state)
    const caret: EsmCaret = strip.exchangeComplete ? 'stay' : 'ex0'
    const said: EsmState = { ...state, exchTo: call }
    if (opts.cockpit === 'phone') return { kind: 'speak', caret, next: said }
    return send('callExch', false, caret, said)
  }
  if (!call) return send('myCall', false, 'stay', state)
  if (strip.exchangeComplete) return send('exch', true, 'call', logged)
  if (!opts.callOnce) return send('myCall', false, 'call', state)
  if (state.myCallTo === call) return send('again', false, 'stay', state)
  return send('myCall', false, 'ex0', { ...state, myCallTo: call })
}

interface TxGuards {
  /** The TX latch. ESM never turns it on. */
  txEnabled: boolean
  /** The dial is inside the licence privileges and the connection allows TX
   *  (`snap.radio.txAllowed`). */
  txAllowed: boolean
  /** A clock repair holds transmit. */
  clockRepair: boolean
}

/** The transmit guards as the cockpit reads them at the press, per cockpit. Every field is
 *  required: a guard the caller forgot must not read as a clear one. */
export type EsmGuards =
  | ({ cockpit: 'cw' } & TxGuards)
  | ({
      cockpit: 'rtty'
      /** The auto sequence is running: it keys on its own steps. */
      autoRunning: boolean
      /** Continuous TX is latched: a macro types into the live stream there. */
      continuousTx: boolean
    } & TxGuards)
  | ({
      cockpit: 'phone'
      /** The voice keyer pane is on screen: the only player of the recordings. */
      keyerShown: boolean
      pttHeld: boolean
      recording: boolean
      radioHasMic: boolean
    } & TxGuards)

/** Why ESM steps aside: Enter logs exactly as it does with ESM off, sends nothing, and the ESM
 *  plate says why. `noRoles` — the set in use has no step mapped at all (today's CW sets, RTTY's
 *  Everyday, a set of the operator's own nobody has mapped). */
export type EsmInert = 'noKeyer' | 'auto' | 'continuousTx' | 'noRoles'

/** Why ESM cannot work in this cockpit right now, or null. (`noRoles` is the set's, not the
 *  cockpit's: `esmPress` asks it.) */
export function esmInert(guards: EsmGuards): EsmInert | null {
  if (guards.cockpit === 'phone' && !guards.keyerShown) return 'noKeyer'
  if (guards.cockpit === 'rtty' && guards.autoRunning) return 'auto'
  if (guards.cockpit === 'rtty' && guards.continuousTx) return 'continuousTx'
  return null
}

/** Why a message cannot start right now, or null — checked before anything logs. */
export function esmTxRefusal(guards: EsmGuards): EsmRefusal | null {
  if (!guards.txEnabled) return { why: 'txOff' }
  if (!guards.txAllowed) return { why: 'txLocked' }
  if (guards.clockRepair) return { why: 'clockRepair' }
  if (guards.cockpit === 'phone') {
    if (guards.recording) return { why: 'recording' }
    if (guards.pttHeld) return { why: 'pttHeld' }
    if (guards.radioHasMic) return { why: 'radioHasMic' }
  }
  return null
}

/** Everything one Enter press is decided on. */
export interface EsmPressInput {
  state: EsmState
  strip: EsmStrip
  /** "Call once" (S&P): my call goes once per call, then Enter asks for a repeat. */
  callOnce: boolean
  guards: EsmGuards
  /** The set's roles: `esmRoles(builtIn, own)`. */
  roles: EsmRoleMap
  /** The set's keys as the dock shows them, the operator's own texts included. */
  slots: readonly EsmSlot[]
}

/** One Enter press, decided.
 *
 *  `inert` — ESM steps aside: Enter logs exactly as it does with ESM off; the plate says why.
 *  `refuse` — nothing is sent and nothing logs; the strip says why.
 *  `speak` — Phone, Run: nothing plays; the operator says his call and the exchange.
 *  `send` — send `text` (the keys' messages joined) as `role`, through the cockpit's own send
 *  path; when `log`, the contact logs at this press as soon as that path takes the message — and
 *  stays logged if the message is then stopped. `next` is kept only if the message was taken. */
export type EsmDecision =
  | { kind: 'inert'; why: EsmInert }
  | { kind: 'refuse'; refusal: EsmRefusal }
  | { kind: 'speak'; caret: EsmCaret; next: EsmState }
  | {
      kind: 'send'
      role: EsmRole
      keys: MacroKey[]
      text: string
      log: boolean
      caret: EsmCaret
      next: EsmState
    }

/** One Enter press: the step, its message, then the transmit guards — in that order, so the
 *  strip names the most useful reason (a dupe before an empty key, an empty key before TX off),
 *  and every refusal comes before anything could log. */
export function esmPress(input: EsmPressInput): EsmDecision {
  const { guards } = input
  const aside = esmInert(guards) ?? (esmHasSteps(input.roles) ? null : 'noRoles')
  if (aside) return { kind: 'inert', why: aside }
  const step = esmStep(input.state, input.strip, { cockpit: guards.cockpit, callOnce: input.callOnce })
  if (step.kind !== 'send') return step
  const message = esmMessage(input.roles, step.role, input.slots, guards.cockpit)
  if ('why' in message) return { kind: 'refuse', refusal: message }
  const refusal = esmTxRefusal(guards)
  if (refusal) return { kind: 'refuse', refusal }
  return { kind: 'send', ...message, role: step.role, log: step.log, caret: step.caret, next: step.next }
}

/** Whether a set's roles map any step at all. A set that maps none makes ESM step aside. */
export function esmHasSteps(roles: EsmRoleMap): boolean {
  return ESM_ROLES.some((role) => (roles[role]?.length ?? 0) > 0)
}

/** `role`'s message as a press sends it: its keys and their texts joined (`resolveEsmRole`), or
 *  why there is none, by name. In Phone a step mapped to two recordings has none, because the
 *  keyer plays one recording per press. The role picker shows exactly this for each step. */
export function esmMessage(
  roles: EsmRoleMap,
  role: EsmRole,
  slots: readonly EsmSlot[],
  cockpit: EsmCockpit,
): { keys: MacroKey[]; text: string } | Extract<EsmRefusal, { why: 'unmapped' | 'empty' | 'oneSlot' }> {
  const message = resolveEsmRole(roles, role, slots)
  if ('why' in message) return message
  if (cockpit === 'phone' && message.keys.length > 1) return { why: 'oneSlot', role }
  return message
}
