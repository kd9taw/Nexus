// THE CW TURN CUE: what the CW cockpit draws from the station's turn read (`turn` in the CW decode
// read; the model is `tempo_core::cw_turn`). Pure, so every state is tested by value: which F-key a
// role names in the set the dock shows, the lamp's glyph and words, how a key wears the cue, the
// pace hint, and what a screen reader hears.
//
// ⛔ NOTHING HERE SENDS. A role names the KIND of message that fits; the cockpit lights the key that
// sends it, and only the operator presses one. Nothing is held back either: a key pressed while
// the lamp says the other station is sending still sends.
//
// The glyphs are not words and stay in the code; every word comes from the catalog (`cw.turn.*`).
// ▶ and ⏸ carry U+FE0E, the text-style selector, so Windows never draws them as colour emoji.

import { t } from '../i18n'
import type { CwTurn, CwTurnRole, CwTurnState } from '../types'
import type { EsmRole } from './esm'
import type { EsmRoleMap } from './esmRoles'

/** How a key wears the cue: lit (your turn, or they are signing), a dashed outline while their
 *  over ends (the pre-cue), a dotted one when it is only a suggestion ("not sure", no reply). */
export type CueGlow = 'now' | 'soon' | 'maybe'

/** One F-key as the dock shows it. */
export interface CueSlot {
  key: string
  text: string
}

/** The contest layout's step for each role (`features/esmRoles.ts`). Signing has none: a contest
 *  contact ends on TU. */
const CONTEST_STEP: Partial<Record<CwTurnRole, EsmRole>> = {
  cq: 'cq',
  answer: 'myCall',
  reply: 'callExch',
  exchange: 'exch',
  tu: 'tu',
  myCall: 'myCall',
  again: 'again',
}

/** The keys a role names in the set the dock shows. In a contest, the contest layout's step for
 *  it (`roles`: the set's own, with the profile's mapping over it). Outside one, the everyday keys
 *  by place, the roles every set keeps (F1 CQ, F2 the answer, F3 the reply, F4 73), and "my call"
 *  and AGN by what the key sends, so a set laid out differently lights the key that sends them, or
 *  none. "Send it again" is the key last sent. A key with nothing in it is never named. */
export function cueKeys(
  role: CwTurnRole | null,
  ctx: { slots: readonly CueSlot[]; contest: boolean; roles: EsmRoleMap; lastKey: string | null },
): string[] {
  if (!role) return []
  const sends = (key: string) => ctx.slots.some((s) => s.key === key && s.text.trim() !== '')
  const keep = (keys: readonly string[] | undefined) => (keys ?? []).filter(sends)
  if (role === 'repeat') return ctx.lastKey ? keep([ctx.lastKey]) : []
  if (ctx.contest) {
    const step = CONTEST_STEP[role]
    return step ? keep(ctx.roles[step]) : []
  }
  const first = (match: (text: string) => boolean) =>
    ctx.slots.filter((s) => match(s.text.trim().toUpperCase())).slice(0, 1).map((s) => s.key)
  switch (role) {
    case 'cq':
      return keep(['F1'])
    case 'answer':
      return keep(['F2'])
    case 'reply':
      return keep(['F3'])
    case 'close':
      return keep(['F4'])
    case 'myCall':
      return first((text) => text === '{MYCALL}')
    case 'again':
      return first((text) => /^(PSE )?AGN\b/.test(text))
    default:
      return []
  }
}

/** The lamp, as drawn. `speech` is what a screen reader hears, and at which verbosity. */
export interface Lamp {
  state: CwTurnState
  glyph: string
  label: string
  title: string
  /** The keys that wear the cue, and how. */
  keys: string[]
  glow: CueGlow | null
  speech: { text: string; at: 'needed' | 'all' } | null
}

const GLYPH: Record<CwTurnState, string> = {
  idle: '◌',
  theySend: '≋',
  finishing: '≋',
  yourTurn: '▶︎',
  hold: '⏸︎',
  unsure: '?',
  youSend: '●',
  listen: '◔',
  closing: '✓',
}

/** The lamp for a turn read. `keys` are the role's keys (`cueKeys`); `esm` is Enter Sends Message
 *  running in this cockpit, which knows WHAT the next message is and lights its own keys: the lamp
 *  then says only WHEN ("Your turn · Enter") and lights none. */
export function lamp(turn: CwTurn, keys: readonly string[], esm: boolean): Lamp {
  const lit = esm ? [] : [...keys]
  const key = esm ? (turn.state === 'yourTurn' ? t('cw.turn.key.enter') : '') : lit.join('+')
  const call = turn.call ?? ''
  const base = t('cw.turn.title')
  let label: string
  let title = base
  let glow: CueGlow | null = null
  let speech: Lamp['speech'] = null
  let glyph = GLYPH[turn.state]
  switch (turn.state) {
    case 'idle':
      if (turn.hint === 'noReply') {
        label = key ? t('cw.turn.noReply', { key }) : t('cw.turn.noReply.bare')
        glow = 'maybe'
        speech = { text: t('cw.turn.say.noReply'), at: 'all' }
      } else label = t('cw.turn.idle')
      break
    case 'theySend':
      label = call ? t('cw.turn.theySend', { call }) : t('cw.turn.theySend.bare')
      break
    case 'finishing':
      label = call ? t('cw.turn.finishing', { call }) : t('cw.turn.finishing.bare')
      glow = 'soon'
      break
    case 'yourTurn':
      glow = 'now'
      if (turn.hint === 'type') label = t('cw.turn.type')
      else if (turn.hint === 'fiveNnTu') {
        // A DX coming back in a pileup. No key fits, so the lamp says what to send: nobody hovers
        // in a pileup. The tooltip says why the Reply key does not fit.
        label = t('cw.turn.fiveNnTu')
        title = `${t('cw.turn.fiveNnTu.title')}\n${base}`
      } else if (turn.hint === 'partial') {
        label = key ? t('cw.turn.partial', { key }) : t('cw.turn.partial.bare')
      } else label = key ? t('cw.turn.yourTurn', { key }) : t('cw.turn.yourTurn.bare')
      speech = {
        text:
          turn.hint === 'fiveNnTu'
            ? t('cw.turn.say.fiveNnTu')
            : key
              ? t('cw.turn.say.yourTurn', { key })
              : t('cw.turn.say.yourTurn.bare'),
        at: 'needed',
      }
      break
    case 'hold':
      if (turn.hint === 'splitOff') {
        glyph = '⇡'
        label = t('cw.turn.splitOff')
        speech = { text: t('cw.turn.say.splitOff'), at: 'needed' }
      } else {
        label = call ? t('cw.turn.hold', { call }) : t('cw.turn.hold.bare')
        speech = { text: call ? t('cw.turn.say.hold', { call }) : t('cw.turn.say.hold.bare'), at: 'needed' }
      }
      break
    case 'unsure':
      label = key ? t('cw.turn.unsure', { key }) : t('cw.turn.unsure.bare')
      glow = 'maybe'
      speech = { text: t('cw.turn.say.unsure'), at: 'all' }
      break
    case 'youSend':
      label = t('cw.turn.youSend')
      break
    case 'listen':
      label = turn.hint === 'late' ? t('cw.turn.late') : t('cw.turn.listen')
      break
    case 'closing':
      label = key ? t('cw.turn.closing') : t('cw.turn.closing.bare')
      glow = 'now'
      speech = { text: t('cw.turn.say.closing'), at: 'needed' }
      break
  }
  // The tooltip leads with the whole line, which a narrow row may cut.
  return {
    state: turn.state,
    glyph,
    label,
    title: `${label}\n${title}`,
    keys: glow ? lit : [],
    glow: glow && lit.length ? glow : null,
    speech,
  }
}

/** The unit the speed readout prints, an invariant token, as the Decode head has always printed it. */
const WPM = 'WPM'

/** Their speed beside ours, with ⇣ when ours is 3 WPM or more faster: answer at the speed you are
 *  called at. `null` without a recent measurement of theirs. */
export function pace(them: number, you: number): { label: string; fast: boolean } | null {
  if (!(them > 0)) return null
  const fast = you - them >= 3
  return { label: `${t('cw.pace.label', { them, you })} ${WPM}${fast ? ' ⇣' : ''}`, fast }
}
