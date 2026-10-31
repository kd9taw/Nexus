// The hidden keep-alive cockpit's memo comparator.
import { describe, it, expect } from 'vitest'
import { createElement } from 'react'
import { keepAlivePropsEqual, operatePropsEqual, LIVE_FIELDS } from './keepAliveProps'

const snap = () => ({
  mycall: 'W9XYZ',
  radio: { dialMhz: 14.074, nextSlotMs: 9000, rxLevel: 0.2, smeterDb: null as number | null, clockAgeSecs: 4 },
  stations: [{ call: 'K1ABC', snr: -5 }, { call: 'JA1XYZ', snr: -12 }],
  recentDecodes: [{ id: 'a', text: 'CQ K1ABC FN42' }],
  qso: null as null | { dxcall: string },
})

describe('keepAlivePropsEqual', () => {
  const fn = () => {}
  const props = (active: boolean, s = snap()) => ({ active, snap: s, onSnap: fn, boxes: { stations: s.stations } })

  it('hidden: a fresh but equal snapshot, or one whose meter fields alone moved, is no change', () => {
    expect(keepAlivePropsEqual(props(false), props(false))).toBe(true)
    const live = snap()
    live.radio.nextSlotMs = 100
    live.radio.rxLevel = 0.9
    live.radio.smeterDb = -73
    live.radio.clockAgeSecs = 10
    expect(keepAlivePropsEqual(props(false), props(false, live))).toBe(true)
  })

  it('hidden: any other change is a change, at any depth, in elements too', () => {
    const dial = snap()
    dial.radio.dialMhz = 7.074
    expect(keepAlivePropsEqual(props(false), props(false, dial))).toBe(false)
    const roster = snap()
    roster.stations[1].snr = -1
    expect(keepAlivePropsEqual(props(false), props(false, roster))).toBe(false)

    const el = (radio: object) => ({ active: false, txStrip: createElement('div', { radio }) })
    const r1 = snap().radio
    expect(keepAlivePropsEqual(el(r1), el({ ...r1, nextSlotMs: 1 }))).toBe(true)
    expect(keepAlivePropsEqual(el(r1), el({ ...r1, dialMhz: 3.5 }))).toBe(false)
    expect(keepAlivePropsEqual(el(r1), { active: false, txStrip: createElement('span', { radio: r1 }) })).toBe(false)
  })

  it('hidden: the fields it ignores are exactly the four per-poll meter fields', () => {
    expect([...LIVE_FIELDS].sort()).toEqual(['clockAgeSecs', 'nextSlotMs', 'rxLevel', 'smeterDb'])
    // Every other radio field a poll can carry is a change, the slot and the dial among them.
    for (const k of ['slot', 'dialMhz', 'txEnabled', 'transmitting', 'rxOffsetHz']) {
      const a = { active: false, radio: { [k]: 1 } }
      expect(keepAlivePropsEqual(a, { active: false, radio: { [k]: 2 } }), k).toBe(false)
    }
  })

  it('hidden: past the depth it looks to, a new object counts as a change, never as equal', () => {
    const nest = (depth: number, leaf: unknown): unknown => (depth === 0 ? leaf : { n: nest(depth - 1, leaf) })
    // Equal in value but rebuilt, and deeper than the compare looks: render rather than guess.
    expect(keepAlivePropsEqual({ active: false, deep: nest(40, { v: 1 }) }, { active: false, deep: nest(40, { v: 1 }) })).toBe(false)
    // The same object at any depth is still no change.
    const same = nest(40, { v: 1 })
    expect(keepAlivePropsEqual({ active: false, deep: same }, { active: false, deep: same })).toBe(true)
    // Everything a cockpit is handed lies inside the depth: a message's `incomplete` pair under
    // boxes → otaBoard → snap → conversations → messages, rebuilt and equal, is no change.
    const boxes = () => ({ otaBoard: { snap: { conversations: [{ peer: 'K1ABC', messages: [{ text: 'hi', incomplete: [1, 3] }] }] } } })
    expect(keepAlivePropsEqual({ active: false, boxes: boxes() }, { active: false, boxes: boxes() })).toBe(true)
  })

  it('hidden: a handler is compared by identity, never by what it looks like', () => {
    expect(keepAlivePropsEqual({ active: false, onSnap: () => {} }, { active: false, onSnap: () => {} })).toBe(false)
  })

  it('on screen, or crossing on or off: React’s own shallow compare', () => {
    const p = props(true)
    expect(keepAlivePropsEqual(p, { ...p })).toBe(true)
    expect(keepAlivePropsEqual(p, props(true))).toBe(false) // equal content, new snapshot object
    const live = snap()
    live.radio.nextSlotMs = 1
    expect(keepAlivePropsEqual(props(true), props(true, live))).toBe(false)
    const h = props(false)
    expect(keepAlivePropsEqual(h, { ...h, active: true })).toBe(false)
    expect(keepAlivePropsEqual({ ...h, active: true }, h)).toBe(false)
  })
})

describe('the first poll after a cockpit is hidden', () => {
  for (const [name, part] of [['snap', 'snap'], ['APRS, given only radio', 'radio']] as const) {
    it(`${name}: renders that poll, keeps the flag through re-renders of App, then sits out again`, () => {
      const at = (active: boolean, value: object) => ({ active, [part]: value })
      const shown = snap()
      const hide = at(false, shown) // the hide render reuses the last poll's snapshot
      expect(keepAlivePropsEqual(at(true, shown), hide)).toBe(false)
      // App rendering again for another reason, same snapshot: no poll, nothing to render, the
      // flag stays for the poll still to come.
      expect(keepAlivePropsEqual(hide, at(false, shown))).toBe(true)
      // The first poll, unchanged in value: rendered, as every poll was.
      const first = at(false, snap())
      expect(keepAlivePropsEqual(hide, first)).toBe(false)
      // The one after it is sat out.
      expect(keepAlivePropsEqual(first, at(false, snap()))).toBe(true)
      // A poll that lands in the hide render itself: it is the hidden one that counts.
      const hideOnPoll = at(false, snap())
      expect(keepAlivePropsEqual(at(true, snap()), hideOnPoll)).toBe(false)
      expect(keepAlivePropsEqual(hideOnPoll, at(false, snap()))).toBe(false)
    })
  }

  it('a cockpit that was never on screen, or stays on screen, gets no such render', () => {
    const a = { active: false, snap: snap() }
    expect(keepAlivePropsEqual(a, { active: false, snap: snap() })).toBe(true)
    const on = { active: true, snap: snap() }
    const stays = { active: true, snap: snap() }
    expect(keepAlivePropsEqual(on, stays)).toBe(false)
    // Hidden later with a snapshot it held on screen: only the hide render's snapshot is flagged.
    expect(keepAlivePropsEqual({ active: false, snap: on.snap }, { active: false, snap: snap() })).toBe(true)
  })
})

describe('operatePropsEqual (the FT cockpit)', () => {
  type Ticks = { clearTick?: number; rxFreqClearTick?: number }
  const ft = (ticks: Ticks = {}) => ({ active: false, snap: { ...snap(), ...ticks } })

  for (const tick of ['clearTick', 'rxFreqClearTick'] as const) {
    it(`hidden: renders on the ${tick} poll and on the one poll after it, then sits out again`, () => {
      const before = ft({ [tick]: 0 })
      const clear = ft({ [tick]: 1 })
      const after = ft({ [tick]: 1 })
      expect(operatePropsEqual(before, clear)).toBe(false)
      // The poll after the Clear, unchanged in value: it refills the panes, so it renders.
      expect(operatePropsEqual(clear, after)).toBe(false)
      // The one after that is sat out, as any unchanged poll is.
      expect(operatePropsEqual(after, ft({ [tick]: 1 }))).toBe(true)
    })
  }

  it('hidden: no Clear, no extra render; the same snapshot again is no poll', () => {
    const a = ft({ clearTick: 3, rxFreqClearTick: 2 })
    const b = ft({ clearTick: 3, rxFreqClearTick: 2 })
    expect(operatePropsEqual(a, b)).toBe(true)
    expect(operatePropsEqual(b, ft({ clearTick: 3, rxFreqClearTick: 2 }))).toBe(true)
    // A re-render of App with the Clear's own snapshot is not the poll after it.
    const clear = ft({ clearTick: 4, rxFreqClearTick: 2 })
    expect(operatePropsEqual(b, clear)).toBe(false)
    expect(operatePropsEqual(clear, { ...clear })).toBe(true)
    // Everything else is `keepAlivePropsEqual`: a real change still renders.
    const dial = ft({ clearTick: 3, rxFreqClearTick: 2 })
    dial.snap.radio.dialMhz = 7.074
    expect(operatePropsEqual(a, dial)).toBe(false)
  })
})
