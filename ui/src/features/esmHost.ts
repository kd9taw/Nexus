// ENTER SENDS MESSAGE — what a cockpit hands its contest strip, and what its TX dock shows.
//
// The strip decides each Enter press (`esmPress`) and logs through its own one log path. The
// cockpit owns everything that is its own: the switch, the set in use (its roles and keys), its
// transmit guards as it reads them at the press, and its own send path, the one its F-keys use
// with every check they have. ESM's state (Run or S&P, and what went out to the call in the strip)
// lives here, with the cockpit, because the dock shows it: the Run/S&P plate, and the key or keys
// Enter sends next.
//
// ⛔ NOTHING HERE TRANSMITS OR TURNS TX ON. A message goes out only from an Enter press in the
// strip, through `send`, which each cockpit builds on its own send path; nothing is sent by a
// timer. A stop (`noteStop`) only makes what went out count as not sent: it keys nothing and waits
// for nothing, and the cockpit's own stop has already run.
import { useCallback, useEffect, useState } from 'react'
import {
  esmEvent,
  esmPress,
  esmStart,
  type EsmCockpit,
  type EsmDecision,
  type EsmEvent,
  type EsmGuards,
  type EsmRole,
  type EsmState,
  type EsmStrip,
} from './esm'
import type { EsmRoleMap, EsmSlot } from './esmRoles'
import type { MacroKey } from './macroSets'

/** One message as the strip hands it to the cockpit's send path. */
export interface EsmMessage {
  role: EsmRole
  keys: MacroKey[]
  /** The keys' texts joined: the template the cockpit sends, its tokens expanded by its own path. */
  text: string
  /** The call in the strip at the press, which is the call the message names (rule 7). */
  call: string
}

/** What a cockpit hands its contest strip. */
export interface EsmHost {
  cockpit: EsmCockpit
  /** The cockpit's switch is on, a contest runs, and this window is the station's own. */
  on: boolean
  /** "Call once" (S&P). */
  callOnce: boolean
  /** The set's roles: `esmRoles(builtIn, own)`. */
  roles: EsmRoleMap
  /** The set's keys as the dock shows them, the operator's own texts included. */
  slots: readonly EsmSlot[]
  /** The transmit guards as the cockpit reads them at the press. */
  guards: () => EsmGuards
  /** The cockpit's own send path: resolves null once it took the message, or with why it did not.
   *  It never turns TX on. */
  send: (message: EsmMessage) => Promise<string | null>
  state: EsmState
  dispatch: (next: EsmState | EsmEvent) => void
  /** Counts the stops the cockpit sees (Esc, Stop TX, the keyer's ■ Stop): a change is a stop. */
  stops: number
  /** What the strip holds, for the dock; null while there is no contest strip. */
  onStrip: (strip: EsmStrip | null) => void
}

/** What App hands a cockpit: its ESM switch as Settings holds it, "call once", and the dock
 *  switch's save (that one field, never a settings-form save). Absent on the hosted page. */
export interface EsmSetting {
  on: boolean
  callOnce: boolean
  onSwitch: (on: boolean) => void
  /** Phone only: the voice keyer's step mapping (`Settings.voiceEsmRoles`). */
  voiceRoles?: EsmRoleMap
}

/** The parts of a host the cockpit supplies; the rest is kept here. */
export type EsmHostInput = Pick<EsmHost, 'cockpit' | 'on' | 'callOnce' | 'roles' | 'slots' | 'guards' | 'send'>

const sameStrip = (a: EsmStrip | null, b: EsmStrip | null) =>
  a === b ||
  (a !== null &&
    b !== null &&
    a.call === b.call &&
    a.exchangeComplete === b.exchangeComplete &&
    a.dupe === b.dupe &&
    (a.fromHistory ?? null) === (b.fromHistory ?? null))

/** A cockpit's Enter Sends Message: the host for its strip, what the next Enter would do (for the
 *  dock's plate and highlight), and the cockpit's three events: a stop, the key its CQ is on, and
 *  the plate's toggle. */
export function useEsmHost(input: EsmHostInput): {
  host: EsmHost
  preview: EsmDecision | null
  noteStop: () => void
  noteKey: (key: string) => void
  toggle: () => void
} {
  const [state, setState] = useState<EsmState>(esmStart)
  const [strip, setStrip] = useState<EsmStrip | null>(null)
  const [stops, setStops] = useState(0)
  const dispatch = useCallback((next: EsmState | EsmEvent) => {
    setState((s) => (typeof next === 'string' ? esmEvent(s, next) : next))
  }, [])
  const onStrip = useCallback((next: EsmStrip | null) => {
    setStrip((prev) => (sameStrip(prev, next) ? prev : next))
  }, [])
  const noteStop = useCallback(() => setStops((n) => n + 1), [])
  // Switched off (or the contest ended), ESM forgets: switched back on, it starts in S&P with
  // nothing sent, as a remount does.
  useEffect(() => {
    if (!input.on) setState(esmStart())
  }, [input.on])
  const { roles } = input
  // The key the set's CQ step is on switches to Run (decision 5): F1 in every built-in set.
  const noteKey = (key: string) => {
    if (input.on && (roles.cq ?? []).includes(key as MacroKey)) dispatch('cq')
  }
  const toggle = () => dispatch('toggle')
  const host: EsmHost = { ...input, state, dispatch, stops, onStrip }
  const preview =
    input.on && strip
      ? esmPress({ state, strip, callOnce: input.callOnce, guards: input.guards(), roles, slots: input.slots })
      : null
  return { host, preview, noteStop, noteKey, toggle }
}

/** The keys the dock lights for the next Enter: the keys of the message it would send. */
export function esmLitKeys(preview: EsmDecision | null): readonly string[] {
  return preview?.kind === 'send' ? preview.keys : []
}
