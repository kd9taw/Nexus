// Structural sharing of the snapshot, and the hidden keep-alive cockpit's memo comparator.
import { describe, it, expect } from 'vitest'
import { createElement } from 'react'
import { keepAlivePropsEqual, shareSnapshot } from './snapshotShare'

const snap = () => ({
  mycall: 'W9XYZ',
  radio: { dialMhz: 14.074, nextSlotMs: 9000, rxLevel: 0.2, smeterDb: null as number | null, clockAgeSecs: 4 },
  stations: [{ call: 'K1ABC', snr: -5 }, { call: 'JA1XYZ', snr: -12 }],
  recentDecodes: [{ id: 'a', text: 'CQ K1ABC FN42' }],
  qso: null as null | { dxcall: string },
})

describe('shareSnapshot', () => {
  it('hands back the old snapshot itself when the new one is equal', () => {
    const a = snap()
    expect(shareSnapshot(a, snap())).toBe(a)
  })

  it('keeps every unchanged part and makes new only the path to what changed', () => {
    const a = snap()
    const next = snap()
    next.stations[1].snr = -3
    const s = shareSnapshot(a, next)
    expect(s).not.toBe(a)
    expect(s.radio).toBe(a.radio)
    expect(s.recentDecodes).toBe(a.recentDecodes)
    expect(s.stations).not.toBe(a.stations)
    expect(s.stations[0]).toBe(a.stations[0])
    expect(s.stations[1]).not.toBe(a.stations[1])
    expect(s.stations[1].snr).toBe(-3)
    expect(s).toEqual(next)
    // The old snapshot is never edited.
    expect(a.stations[1].snr).toBe(-12)
  })

  it('follows a list that grows or shrinks, and a field that appears, leaves or changes type', () => {
    const a = snap()
    const grown = snap()
    grown.recentDecodes.push({ id: 'b', text: 'CQ DL1QQ JO62' })
    const g = shareSnapshot(a, grown)
    expect(g.recentDecodes).not.toBe(a.recentDecodes)
    expect(g.recentDecodes[0]).toBe(a.recentDecodes[0])
    expect(g.recentDecodes).toEqual(grown.recentDecodes)

    const shrunk = snap()
    shrunk.stations.pop()
    expect(shareSnapshot(a, shrunk).stations).toEqual([{ call: 'K1ABC', snr: -5 }])

    const added = { ...snap(), extra: 1 }
    expect(shareSnapshot(a, added)).toEqual(added)
    const { qso: _gone, ...left } = snap()
    expect(shareSnapshot(a, left)).toEqual(left)
    expect('qso' in shareSnapshot(a, left)).toBe(false)

    const typed = { ...snap(), qso: { dxcall: 'K1ABC' } }
    expect(shareSnapshot(a, typed).qso).toEqual({ dxcall: 'K1ABC' })
    expect(shareSnapshot(typed, snap()).qso).toBeNull()
  })

  it('takes the new value when there is no old one', () => {
    const n = snap()
    expect(shareSnapshot(null, n)).toBe(n)
    expect(shareSnapshot(n, null)).toBeNull()
  })
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
