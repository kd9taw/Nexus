// @vitest-environment jsdom
//
// The JS8 lists' view choices (features/js8View): JS8Call 3.0.3's defaults, a stored record read
// back value by value with anything unknown defaulted, and what Clear leaves.
import { describe, expect, it } from 'vitest'
import {
  JS8_LISTS_KEY,
  JS8_VIEW_KEY,
  clearJs8Lists,
  defaultJs8ViewPrefs,
  emptyJs8ListMarks,
  js8StartupMarks,
  loadJs8ListMarks,
  loadJs8ViewPrefs,
  parseJs8ListMarks,
  parseJs8ViewPrefs,
  saveJs8ViewPrefs,
} from './js8View'

describe('the view choices', () => {
  it('default to JS8Call’s: Time Delta, Speed, Grid, Distance and Azimuth hidden, the rest shown, labels and heartbeats on', () => {
    const d = defaultJs8ViewPrefs()
    expect(d.band.columns).toEqual({ offset: true, timestamp: true, snr: true, tdrift: false, submode: false })
    expect(d.call.columns).toEqual({
      callsign: true,
      timestamp: true,
      snr: true,
      offset: true,
      tdrift: false,
      submode: false,
      grid: false,
      distance: false,
      azimuth: false,
      log: true,
      logName: true,
      logComment: true,
    })
    expect([d.band.sort, d.call.sort, d.labels, d.showHb]).toEqual(['offset', 'callsign', true, true])
  })

  it('read a stored record value by value, and default what is missing, unknown or of the wrong kind', () => {
    const p = parseJs8ViewPrefs(
      JSON.stringify({ band: { columns: { tdrift: true, offset: 'yes' }, sort: '-snr' }, call: { sort: 'sideways' }, labels: false, showHb: 1 }),
    )
    expect(p.band.columns.tdrift).toBe(true)
    expect(p.band.columns.offset, 'not a boolean').toBe(true)
    expect(p.band.sort).toBe('-snr')
    expect(p.call.sort, 'not a sort JS8Call has').toBe('callsign')
    expect([p.labels, p.showHb]).toEqual([false, true])
    for (const raw of [null, '', '{', '[]', '"x"']) expect(parseJs8ViewPrefs(raw)).toEqual(defaultJs8ViewPrefs())
  })

  it('keep JS8Call’s Minimal Column Labels, off by default, and read it back only as a boolean', () => {
    expect(defaultJs8ViewPrefs().minimal).toBe(false)
    expect(parseJs8ViewPrefs(JSON.stringify({ minimal: true })).minimal).toBe(true)
    expect(parseJs8ViewPrefs(JSON.stringify({ minimal: 'yes' })).minimal, 'not a boolean').toBe(false)
  })

  it('round-trip through this browser’s storage, and hold when storage refuses', () => {
    localStorage.clear()
    const p = { ...defaultJs8ViewPrefs(), showHb: false }
    saveJs8ViewPrefs(p)
    expect(JSON.parse(localStorage.getItem(JS8_VIEW_KEY)!).showHb).toBe(false)
    expect(loadJs8ViewPrefs()).toEqual(p)
    localStorage.setItem(JS8_LISTS_KEY, '{oops')
    expect(loadJs8ListMarks()).toEqual(emptyJs8ListMarks())
  })
})

describe('what was cleared, removed and added', () => {
  it('reads back only real times, the newest 500 of each kind', () => {
    const many = Object.fromEntries(Array.from({ length: 600 }, (_, i) => [`K${i}AA`, i + 1]))
    const m = parseJs8ListMarks(JSON.stringify({ cleared: { rx: 5, band: -1, call: 'x' }, removedCalls: many, added: { W1AW: 0, K1ABC: 7 } }))
    expect(m.cleared).toEqual({ rx: 5, band: 0, call: 0 })
    expect(Object.keys(m.removedCalls)).toHaveLength(500)
    expect(m.removedCalls.K599AA).toBe(600)
    expect(m.removedCalls.K0AA).toBeUndefined()
    expect(m.added).toEqual({ K1ABC: 7 })
  })

  it('clears one list or all of them, the call list’s removals and additions going with it', () => {
    const m = { ...emptyJs8ListMarks(), removedOffsets: { '900': 1 }, removedCalls: { W1AW: 1 }, added: { K1NEW: 1 } }
    expect(clearJs8Lists(m, 'rx', 9)).toEqual({ ...m, cleared: { rx: 9, band: 0, call: 0 } })
    expect(clearJs8Lists(m, 'band', 9)).toEqual({ ...m, cleared: { rx: 0, band: 9, call: 0 }, removedOffsets: {} })
    expect(clearJs8Lists(m, 'call', 9)).toEqual({ ...m, cleared: { rx: 0, band: 0, call: 9 }, removedCalls: {}, added: {} })
    expect(clearJs8Lists(m, 'all', 9)).toEqual({ cleared: { rx: 9, band: 9, call: 9 }, removedOffsets: {}, removedCalls: {}, added: {}, startup: 0 })
  })
})

// JS8Call's "Reset the Band Activity, Call Activity, and RX history at startup" (mainwindow.cpp:505-508,
// :607-609, :1281-1285): Clear All Lists as of the run's start, decided once a run.
describe('the startup reset', () => {
  const m = { ...emptyJs8ListMarks(), cleared: { rx: 5, band: 5, call: 5 }, removedOffsets: { '900': 1 }, removedCalls: { W1AW: 1 }, added: { K1NEW: 1 } }

  it('on, clears the three lists as of the run’s start, their removals and additions with them, and records the run', () => {
    expect(js8StartupMarks(m, true, 1000)).toEqual({
      cleared: { rx: 1000, band: 1000, call: 1000 },
      removedOffsets: {},
      removedCalls: {},
      added: {},
      startup: 1000,
    })
  })

  it('off, changes nothing but the record of the run', () => {
    expect(js8StartupMarks(m, false, 1000)).toEqual({ ...m, startup: 1000 })
  })

  it('is decided once a run: a later answer for the same run changes nothing', () => {
    const off = js8StartupMarks(m, false, 1000)
    expect(js8StartupMarks(off, true, 1000)).toBe(off)
    const on = js8StartupMarks(m, true, 1000)
    const later = { ...on, removedCalls: { N0NEW: 2000 } }
    expect(js8StartupMarks(later, true, 1000), 'a removal made since stays').toBe(later)
    expect(js8StartupMarks(off, true, 3000).cleared.rx, 'the next run decides again').toBe(3000)
  })

  it('never moves a later clear back', () => {
    expect(js8StartupMarks({ ...m, cleared: { rx: 2000, band: 5, call: 5 } }, true, 1000).cleared).toEqual({ rx: 2000, band: 1000, call: 1000 })
  })

  it('reads the record of the run back, and Clear keeps it', () => {
    expect(parseJs8ListMarks(JSON.stringify({ startup: 1000 })).startup).toBe(1000)
    expect(parseJs8ListMarks(JSON.stringify({ startup: 'x' })).startup, 'not a time').toBe(0)
    expect(clearJs8Lists({ ...m, startup: 1000 }, 'all', 9).startup).toBe(1000)
  })
})
