// Enter Sends Message — the step table, by value. Every row of the Run and S&P tables, each
// refusal by its reason, the dupes, "call once", and the press that logs. Pure: no rig, no
// socket, no engine — what Enter WOULD do, which the cockpits carry out through their own send
// paths once ESM is wired.
//
// Each case names the value it expects in full (role, log, caret and the state after), so a
// step that sends the right message but logs, or moves the caret, where it should not is red.
import { describe, expect, it } from 'vitest'
import {
  esmEvent,
  esmInert,
  esmPress,
  esmStart,
  esmStep,
  esmTxRefusal,
  type EsmDecision,
  type EsmGuards,
  type EsmPressInput,
  type EsmState,
  type EsmStrip,
} from './esm'
import { CONTEST_LAYOUT_ROLES, CW_CONTEST_LAYOUT, VOICE_SLOT_ROLES, esmRoles } from './esmRoles'
import { resolveRttySet } from './rttyMacros'

const RUN: EsmState = { mode: 'run', exchTo: null, myCallTo: null }
const SP: EsmState = { mode: 'sp', exchTo: null, myCallTo: null }
const strip = (
  call: string,
  exchangeComplete = false,
  dupe: EsmStrip['dupe'] = 'none',
  fromHistory = false,
): EsmStrip => ({
  call,
  exchangeComplete,
  fromHistory,
  dupe,
})
/** A strip whose exchange a call-history fill completes: no box typed over. */
const filled = (call: string, exchangeComplete = true): EsmStrip => strip(call, exchangeComplete, 'none', true)
const CW = { cockpit: 'cw', callOnce: false } as const
const RTTY = { cockpit: 'rtty', callOnce: false } as const
const PHONE = { cockpit: 'phone', callOnce: false } as const
const ONCE = { cockpit: 'cw', callOnce: true } as const

describe('esmStep — Run', () => {
  it('R1: an empty call sends CQ, logs nothing, and puts the caret in Call', () => {
    expect(esmStep(RUN, strip(''), CW)).toEqual({ kind: 'send', role: 'cq', log: false, caret: 'call', next: RUN })
  })

  it('R2: a new call, exchange incomplete, sends his call and my exchange; the caret goes to the first box', () => {
    expect(esmStep(RUN, strip('K9AAA'), CW)).toEqual({
      kind: 'send',
      role: 'callExch',
      log: false,
      caret: 'ex0',
      next: { ...RUN, exchTo: 'K9AAA' },
    })
  })

  it('R3: the exchange already sent and his still incomplete sends AGN; the caret stays', () => {
    const sent = { ...RUN, exchTo: 'K9AAA' }
    expect(esmStep(sent, strip('K9AAA'), CW)).toEqual({ kind: 'send', role: 'again', log: false, caret: 'stay', next: sent })
  })

  it('R4: a complete exchange not yet sent sends his call and my exchange, and does NOT log', () => {
    expect(esmStep(RUN, strip('K9AAA', true), CW)).toEqual({
      kind: 'send',
      role: 'callExch',
      log: false,
      caret: 'stay',
      next: { ...RUN, exchTo: 'K9AAA' },
    })
  })

  it('R5: a complete exchange already sent sends TU and logs at the same press; the strip starts a new contact', () => {
    expect(esmStep({ ...RUN, exchTo: 'K9AAA' }, strip('K9AAA', true), CW)).toEqual({
      kind: 'send',
      role: 'tu',
      log: true,
      caret: 'call',
      next: RUN,
    })
  })

  it('R6: an own dupe sends nothing and logs nothing, whatever the exchange holds', () => {
    for (const complete of [false, true]) {
      expect(esmStep(RUN, strip('K9AAA', complete, 'own'), CW)).toEqual({ kind: 'refuse', refusal: { why: 'dupe' } })
      expect(esmStep({ ...RUN, exchTo: 'K9AAA' }, strip('K9AAA', complete, 'own'), CW)).toEqual({
        kind: 'refuse',
        refusal: { why: 'dupe' },
      })
    }
  })

  it('walks a whole contact: CQ, his call and my exchange, AGN, TU and the log', () => {
    let s = RUN
    const steps: string[] = []
    for (const st of [strip(''), strip('K9AAA'), strip('K9AAA'), strip('K9AAA', true)]) {
      const r = esmStep(s, st, CW)
      if (r.kind !== 'send') throw new Error(`refused: ${JSON.stringify(r)}`)
      steps.push(`${r.role}${r.log ? '+log' : ''}`)
      s = r.next
    }
    expect(steps).toEqual(['cq', 'callExch', 'again', 'tu+log'])
    expect(s).toEqual(RUN)
  })
})

describe('esmStep — S&P', () => {
  it('S1: an empty call sends my call; nothing logs and the caret stays', () => {
    expect(esmStep(SP, strip(''), CW)).toEqual({ kind: 'send', role: 'myCall', log: false, caret: 'stay', next: SP })
  })

  it('S2: a call typed, exchange incomplete, sends my call on every press and keeps the caret in Call', () => {
    const first = esmStep(SP, strip('K9AAA'), CW)
    // It remembers the call my call went to, "call once" off too (S5 reads it).
    expect(first).toEqual({ kind: 'send', role: 'myCall', log: false, caret: 'call', next: { ...SP, myCallTo: 'K9AAA' } })
    if (first.kind !== 'send') throw new Error('refused')
    expect(esmStep(first.next, strip('K9AAA'), CW)).toEqual(first)
  })

  it('S2 with "call once": the first press sends my call and moves the caret to the first box, later presses send AGN', () => {
    const first = esmStep(SP, strip('K9AAA'), ONCE)
    expect(first).toEqual({ kind: 'send', role: 'myCall', log: false, caret: 'ex0', next: { ...SP, myCallTo: 'K9AAA' } })
    if (first.kind !== 'send') throw new Error('refused')
    expect(esmStep(first.next, strip('K9AAA'), ONCE)).toEqual({
      kind: 'send',
      role: 'again',
      log: false,
      caret: 'stay',
      next: first.next,
    })
  })

  it('"call once" counts per call: a different call is a first press again', () => {
    expect(esmStep({ ...SP, myCallTo: 'K9AAA' }, strip('W1AW'), ONCE)).toEqual({
      kind: 'send',
      role: 'myCall',
      log: false,
      caret: 'ex0',
      next: { ...SP, myCallTo: 'W1AW' },
    })
  })

  it('S3: a complete exchange sends my exchange and logs at the same press; the strip starts a new contact', () => {
    const expected = { kind: 'send', role: 'exch', log: true, caret: 'call', next: SP }
    expect(esmStep(SP, strip('K9AAA', true), CW)).toEqual(expected)
    expect(esmStep({ ...SP, myCallTo: 'K9AAA' }, strip('K9AAA', true), ONCE)).toEqual(expected)
  })

  it('S4: an own dupe sends nothing and logs nothing', () => {
    expect(esmStep(SP, strip('K9AAA', false, 'own'), CW)).toEqual({ kind: 'refuse', refusal: { why: 'dupe' } })
    expect(esmStep(SP, strip('K9AAA', true, 'own'), ONCE)).toEqual({ kind: 'refuse', refusal: { why: 'dupe' } })
  })

  // Call history fills the boxes as the call is typed, so the exchange can be complete before he
  // has sent a thing. Then my call goes first, as N1MM's S&P Enter sends it (operator, 2026-10-09).
  it('S5: complete only by a call-history fill, the first press sends my call and logs nothing; the next sends my exchange and logs', () => {
    const first = esmStep(SP, filled('K9AAA'), CW)
    expect(first).toEqual({ kind: 'send', role: 'myCall', log: false, caret: 'call', next: { ...SP, myCallTo: 'K9AAA' } })
    if (first.kind !== 'send') throw new Error('refused')
    expect(esmStep(first.next, filled('K9AAA'), CW)).toEqual({ kind: 'send', role: 'exch', log: true, caret: 'call', next: SP })
  })

  it('S5 with "call once": the first press also moves the caret to the first box; the next sends my exchange and logs', () => {
    const first = esmStep(SP, filled('K9AAA'), ONCE)
    expect(first).toEqual({ kind: 'send', role: 'myCall', log: false, caret: 'ex0', next: { ...SP, myCallTo: 'K9AAA' } })
    if (first.kind !== 'send') throw new Error('refused')
    expect(esmStep(first.next, filled('K9AAA'), ONCE)).toEqual({ kind: 'send', role: 'exch', log: true, caret: 'call', next: SP })
  })

  it('S5: my call sent while his exchange was incomplete counts, so a fill that completes it with the rest typed sends my exchange', () => {
    // Field Day: the file fills his section as the call is typed; his class is typed once he sends it.
    for (const opts of [CW, ONCE]) {
      const first = esmStep(SP, filled('K9AAA', false), opts)
      expect(first).toMatchObject({ role: 'myCall', log: false, next: { ...SP, myCallTo: 'K9AAA' } })
      if (first.kind !== 'send') throw new Error('refused')
      expect(esmStep(first.next, filled('K9AAA'), opts)).toEqual({ kind: 'send', role: 'exch', log: true, caret: 'call', next: SP })
    }
  })

  it('S5 is per call: my call went to K9AAA, so K9ABC, filled, gets my call first', () => {
    expect(esmStep({ ...SP, myCallTo: 'K9AAA' }, filled('K9ABC'), CW)).toEqual({
      kind: 'send',
      role: 'myCall',
      log: false,
      caret: 'call',
      next: { ...SP, myCallTo: 'K9ABC' },
    })
  })

  it('S5: a stopped call counts as not sent, so the next press sends it again rather than logging', () => {
    const stopped = esmEvent({ ...SP, myCallTo: 'K9AAA' }, 'stopped')
    expect(esmStep(stopped, filled('K9AAA'), CW)).toEqual({
      kind: 'send',
      role: 'myCall',
      log: false,
      caret: 'call',
      next: { ...SP, myCallTo: 'K9AAA' },
    })
  })

  it('S5 asks only for a fill: an exchange typed or picked sends my exchange and logs at the first press (S3)', () => {
    expect(esmStep(SP, strip('K9AAA', true), CW)).toEqual({ kind: 'send', role: 'exch', log: true, caret: 'call', next: SP })
    expect(esmStep(SP, strip('K9AAA', true), ONCE)).toEqual({ kind: 'send', role: 'exch', log: true, caret: 'call', next: SP })
  })

  it('Run never asks it: a fill gives exactly the result a typed exchange gets', () => {
    for (const s of [RUN, { ...RUN, exchTo: 'K9AAA' }])
      for (const complete of [false, true])
        for (const opts of [CW, ONCE, PHONE])
          expect(esmStep(s, filled('K9AAA', complete), opts)).toEqual(esmStep(s, strip('K9AAA', complete), opts))
  })
})

describe('esmStep — a club dupe is a new call', () => {
  // Another position worked them: a warning on the strip, never a block (N3FJP's semantics).
  const states: EsmState[] = [RUN, { ...RUN, exchTo: 'K9AAA' }, SP, { ...SP, myCallTo: 'K9AAA' }]
  it('gives exactly the result the same strip gets with no dupe, in Run and S&P, call once or not', () => {
    for (const s of states)
      for (const complete of [false, true])
        for (const opts of [CW, ONCE, PHONE]) {
          const club = esmStep(s, strip('K9AAA', complete, 'club'), opts)
          expect(club.kind).not.toBe('refuse')
          expect(club).toEqual(esmStep(s, strip('K9AAA', complete, 'none'), opts))
        }
  })
})

describe('esmStep — Phone', () => {
  // A recording cannot say a callsign, so Run's "his call and my exchange" plays nothing: you
  // say it, and the press counts it as said.
  it('Run, a call typed, exchange incomplete: nothing plays, the caret goes to the first box, and the exchange counts as said', () => {
    expect(esmStep(RUN, strip('K9AAA'), PHONE)).toEqual({ kind: 'speak', caret: 'ex0', next: { ...RUN, exchTo: 'K9AAA' } })
  })

  it('Run, exchange complete but not yet said: nothing plays, and the next press is TU and the log', () => {
    const r = esmStep(RUN, strip('K9AAA', true), PHONE)
    expect(r).toEqual({ kind: 'speak', caret: 'stay', next: { ...RUN, exchTo: 'K9AAA' } })
    if (r.kind !== 'speak') throw new Error('not speak')
    expect(esmStep(r.next, strip('K9AAA', true), PHONE)).toEqual({ kind: 'send', role: 'tu', log: true, caret: 'call', next: RUN })
  })

  it('plays the CQ, AGN and S&P slots as the keyboard modes send them', () => {
    expect(esmStep(RUN, strip(''), PHONE)).toEqual(esmStep(RUN, strip(''), CW))
    expect(esmStep({ ...RUN, exchTo: 'K9AAA' }, strip('K9AAA'), PHONE)).toEqual(
      esmStep({ ...RUN, exchTo: 'K9AAA' }, strip('K9AAA'), CW),
    )
    for (const complete of [false, true])
      expect(esmStep(SP, strip('K9AAA', complete), PHONE)).toEqual(esmStep(SP, strip('K9AAA', complete), CW))
  })

  it('RTTY sends exactly what CW sends', () => {
    for (const s of [RUN, { ...RUN, exchTo: 'K9AAA' }, SP])
      for (const call of ['', 'K9AAA'])
        for (const complete of [false, true])
          expect(esmStep(s, strip(call, complete), RTTY)).toEqual(esmStep(s, strip(call, complete), CW))
  })
})

describe('esmStep — when ESM does not know, it re-sends rather than logs', () => {
  it('a remount starts in S&P with nothing sent, so a complete exchange in Run is sent, not logged', () => {
    expect(esmStart()).toEqual(SP)
    expect(esmStep(esmEvent(esmStart(), 'cq'), strip('K9AAA', true), CW)).toEqual({
      kind: 'send',
      role: 'callExch',
      log: false,
      caret: 'stay',
      next: { ...RUN, exchTo: 'K9AAA' },
    })
  })

  it('a call corrected after the exchange went out gets the exchange again', () => {
    const sent = { ...RUN, exchTo: 'K9AAA' }
    expect(esmStep(sent, strip('K9AAB', true), CW)).toEqual({
      kind: 'send',
      role: 'callExch',
      log: false,
      caret: 'stay',
      next: { ...RUN, exchTo: 'K9AAB' },
    })
    // …and the call it went out to still gets TU.
    expect(esmStep(sent, strip('K9AAA', true), CW)).toMatchObject({ role: 'tu', log: true })
  })

  it('reads the call as the strip holds it: case and stray spaces do not make a new call', () => {
    expect(esmStep({ ...RUN, exchTo: 'K9AAA' }, strip(' k9aaa ', true), CW)).toMatchObject({ role: 'tu', log: true })
  })

  it('a stopped message counts as not sent: the next press sends the exchange again instead of logging', () => {
    const stopped = esmEvent({ ...RUN, exchTo: 'K9AAA' }, 'stopped')
    expect(stopped).toEqual(RUN)
    expect(esmStep(stopped, strip('K9AAA', true), CW)).toMatchObject({ role: 'callExch', log: false })
    expect(esmEvent({ ...SP, myCallTo: 'K9AAA' }, 'stopped')).toEqual(SP)
  })

  it('a stop after the press that logged takes nothing back: the strip is already a new contact', () => {
    const r = esmStep({ ...RUN, exchTo: 'K9AAA' }, strip('K9AAA', true), CW)
    expect(r).toMatchObject({ role: 'tu', log: true })
    if (r.kind !== 'send') throw new Error('refused')
    expect(esmEvent(r.next, 'stopped')).toEqual(r.next)
  })
})

describe('esmEvent — Run and S&P', () => {
  it('starts in S&P; calling CQ switches to Run; a spot click switches to S&P', () => {
    expect(esmStart().mode).toBe('sp')
    expect(esmEvent(SP, 'cq')).toEqual(RUN)
    expect(esmEvent(RUN, 'cq')).toEqual(RUN)
    expect(esmEvent(RUN, 'spot')).toEqual(SP)
    expect(esmEvent(SP, 'spot')).toEqual(SP)
  })

  it('the plate toggles Run and S&P', () => {
    expect(esmEvent(SP, 'toggle')).toEqual(RUN)
    expect(esmEvent(RUN, 'toggle')).toEqual(SP)
  })

  it('switching keeps what went out to the call in the strip', () => {
    expect(esmEvent({ ...SP, exchTo: 'K9AAA', myCallTo: 'K9AAA' }, 'cq')).toEqual({
      mode: 'run',
      exchTo: 'K9AAA',
      myCallTo: 'K9AAA',
    })
  })

  it('a cleared strip is a new contact with nothing sent, in the same mode', () => {
    expect(esmEvent({ mode: 'run', exchTo: 'K9AAA', myCallTo: 'K9AAA' }, 'cleared')).toEqual(RUN)
    expect(esmEvent({ mode: 'sp', exchTo: 'K9AAA', myCallTo: 'K9AAA' }, 'cleared')).toEqual(SP)
  })
})

describe('esmTxRefusal — each refusal by its reason', () => {
  const tx = { txEnabled: true, txAllowed: true, clockRepair: false }
  const cw = (over: Partial<typeof tx> = {}): EsmGuards => ({ cockpit: 'cw', ...tx, ...over })
  const rtty = (over: object = {}): EsmGuards => ({ cockpit: 'rtty', ...tx, autoRunning: false, continuousTx: false, ...over })
  const phone = (over: object = {}): EsmGuards => ({
    cockpit: 'phone',
    ...tx,
    keyerShown: true,
    pttHeld: false,
    recording: false,
    radioHasMic: false,
    ...over,
  })

  it('refuses nothing when every guard is clear', () => {
    expect(esmTxRefusal(cw())).toBeNull()
    expect(esmTxRefusal(rtty())).toBeNull()
    expect(esmTxRefusal(phone())).toBeNull()
  })

  it('TX off: Enter never turns it on, in every cockpit', () => {
    for (const g of [cw({ txEnabled: false }), rtty({ txEnabled: false }), phone({ txEnabled: false })])
      expect(esmTxRefusal(g)).toEqual({ why: 'txOff' })
  })

  it('outside the licence privileges', () => {
    for (const g of [cw({ txAllowed: false }), rtty({ txAllowed: false }), phone({ txAllowed: false })])
      expect(esmTxRefusal(g)).toEqual({ why: 'txLocked' })
  })

  it('while a clock repair holds transmit', () => {
    for (const g of [cw({ clockRepair: true }), rtty({ clockRepair: true }), phone({ clockRepair: true })])
      expect(esmTxRefusal(g)).toEqual({ why: 'clockRepair' })
  })

  it('Phone: the keyer recording, PTT held, the radio holding the mic', () => {
    expect(esmTxRefusal(phone({ recording: true }))).toEqual({ why: 'recording' })
    expect(esmTxRefusal(phone({ pttHeld: true }))).toEqual({ why: 'pttHeld' })
    expect(esmTxRefusal(phone({ radioHasMic: true }))).toEqual({ why: 'radioHasMic' })
  })

  it('ESM steps aside (Enter logs as with ESM off) while the keyer is hidden, the RTTY auto sequence runs, or Continuous TX is latched', () => {
    expect(esmInert(cw())).toBeNull()
    expect(esmInert(rtty())).toBeNull()
    expect(esmInert(phone())).toBeNull()
    expect(esmInert(phone({ keyerShown: false }))).toBe('noKeyer')
    expect(esmInert(rtty({ autoRunning: true }))).toBe('auto')
    expect(esmInert(rtty({ continuousTx: true }))).toBe('continuousTx')
    // Stepping aside is not a refusal: the transmit guards say nothing about it.
    expect(esmTxRefusal(phone({ keyerShown: false }))).toBeNull()
    expect(esmTxRefusal(rtty({ autoRunning: true, continuousTx: true }))).toBeNull()
  })
})

describe('esmPress — the step, its message and the guards, in one answer', () => {
  const tx = { txEnabled: true, txAllowed: true, clockRepair: false }
  const cw: EsmGuards = { cockpit: 'cw', ...tx }
  const press = (over: Partial<EsmPressInput>): EsmDecision =>
    esmPress({
      state: RUN,
      strip: strip(''),
      callOnce: false,
      guards: cw,
      roles: CONTEST_LAYOUT_ROLES,
      slots: CW_CONTEST_LAYOUT,
      ...over,
    })
  const last = { state: { ...RUN, exchTo: 'K9AAA' }, strip: strip('K9AAA', true) }

  it('sends the key the step is on, and logs at the same press on the last step', () => {
    expect(press(last)).toEqual({
      kind: 'send',
      role: 'tu',
      keys: ['F3'],
      text: 'TU {MYCALL}',
      log: true,
      caret: 'call',
      next: RUN,
    })
    expect(press({ strip: strip('K9AAA', true) })).toEqual({
      kind: 'send',
      role: 'callExch',
      keys: ['F2'],
      text: '! {RST} {EXCH}',
      log: false,
      caret: 'stay',
      next: { ...RUN, exchTo: 'K9AAA' },
    })
    expect(press({ state: SP, strip: strip('K9AAA', true) })).toEqual({
      kind: 'send',
      role: 'exch',
      keys: ['F6'],
      text: 'TU {RST} {EXCH}',
      log: true,
      caret: 'call',
      next: SP,
    })
  })

  it('a refused press never logs: each transmit guard refuses the logging step outright', () => {
    expect(press({ ...last, guards: { ...cw, txEnabled: false } })).toEqual({ kind: 'refuse', refusal: { why: 'txOff' } })
    expect(press({ ...last, guards: { ...cw, txAllowed: false } })).toEqual({ kind: 'refuse', refusal: { why: 'txLocked' } })
    expect(press({ ...last, guards: { ...cw, clockRepair: true } })).toEqual({
      kind: 'refuse',
      refusal: { why: 'clockRepair' },
    })
  })

  it('an own dupe is refused as a dupe, before any guard is asked', () => {
    expect(press({ strip: strip('K9AAA', true, 'own'), guards: { ...cw, txEnabled: false } })).toEqual({
      kind: 'refuse',
      refusal: { why: 'dupe' },
    })
  })

  it('steps aside on a set with no step mapped', () => {
    expect(press({ roles: {} })).toEqual({ kind: 'inert', why: 'noRoles' })
    expect(press({ roles: esmRoles(null, undefined), ...last })).toEqual({ kind: 'inert', why: 'noRoles' })
  })

  it('refuses, by name, a step the set has no message for — and still sends the steps it has', () => {
    const noTu = esmRoles(null, { cq: ['F1'], callExch: ['F2'], myCall: ['F4'], exch: ['F6'], again: ['F7'] })
    expect(press({ roles: noTu, ...last })).toEqual({ kind: 'refuse', refusal: { why: 'unmapped', role: 'tu' } })
    expect(press({ roles: noTu, strip: strip('K9AAA') })).toMatchObject({ kind: 'send', role: 'callExch', keys: ['F2'] })
  })

  it('RTTY sends its contest set\'s keys, and steps aside while the auto sequence runs or Continuous TX is latched', () => {
    const rtty: EsmGuards = { cockpit: 'rtty', ...tx, autoRunning: false, continuousTx: false }
    const slots = resolveRttySet(undefined, 'contest', (k) => k)
    expect(press({ guards: rtty, slots, strip: strip('K9AAA') })).toEqual({
      kind: 'send',
      role: 'callExch',
      keys: ['F2'],
      text: '{CALL} 599 {EXCH} {EXCH}',
      log: false,
      caret: 'ex0',
      next: { ...RUN, exchTo: 'K9AAA' },
    })
    expect(press({ guards: { ...rtty, autoRunning: true }, slots })).toEqual({ kind: 'inert', why: 'auto' })
    expect(press({ guards: { ...rtty, continuousTx: true }, slots })).toEqual({ kind: 'inert', why: 'continuousTx' })
  })

  describe('Phone', () => {
    const phone: EsmGuards = {
      cockpit: 'phone',
      ...tx,
      keyerShown: true,
      pttHeld: false,
      recording: false,
      radioHasMic: false,
    }
    const slots = ['F1', 'F2', 'F3', 'F4', 'F5', 'F6'].map((key) => ({ key, text: `${key}.wav` }))
    const ph = (over: Partial<EsmPressInput>) => press({ guards: phone, roles: VOICE_SLOT_ROLES, slots, ...over })

    it('plays the slot the step is on, and logs at the press that plays TU or the S&P exchange', () => {
      expect(ph({})).toMatchObject({ kind: 'send', role: 'cq', keys: ['F1'], log: false })
      expect(ph(last)).toMatchObject({ kind: 'send', role: 'tu', keys: ['F3'], log: true })
      expect(ph({ state: SP, strip: strip('K9AAA', true) })).toMatchObject({ kind: 'send', role: 'exch', keys: ['F2'], log: true })
    })

    it('running, a call typed: nothing plays, so no transmit guard is asked', () => {
      expect(ph({ strip: strip('K9AAA'), guards: { ...phone, txEnabled: false } })).toEqual({
        kind: 'speak',
        caret: 'ex0',
        next: { ...RUN, exchTo: 'K9AAA' },
      })
    })

    it('names the slot to record, and refuses a step mapped to two recordings', () => {
      const noTu = slots.map((s) => (s.key === 'F3' ? { ...s, text: '' } : s))
      expect(ph({ ...last, slots: noTu })).toEqual({ kind: 'refuse', refusal: { why: 'empty', role: 'tu', key: 'F3' } })
      expect(ph({ roles: { ...VOICE_SLOT_ROLES, cq: ['F1', 'F6'] } })).toEqual({
        kind: 'refuse',
        refusal: { why: 'oneSlot', role: 'cq' },
      })
    })

    it('refuses while recording, while PTT is held, and while the radio has the mic', () => {
      expect(ph({ guards: { ...phone, recording: true } })).toEqual({ kind: 'refuse', refusal: { why: 'recording' } })
      expect(ph({ guards: { ...phone, pttHeld: true } })).toEqual({ kind: 'refuse', refusal: { why: 'pttHeld' } })
      expect(ph({ guards: { ...phone, radioHasMic: true } })).toEqual({ kind: 'refuse', refusal: { why: 'radioHasMic' } })
    })

    it('steps aside while the keyer is hidden, before anything else is asked', () => {
      expect(ph({ guards: { ...phone, keyerShown: false, txEnabled: false }, strip: strip('K9AAA', true, 'own') })).toEqual({
        kind: 'inert',
        why: 'noKeyer',
      })
    })
  })
})
