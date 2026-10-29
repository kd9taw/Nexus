// @vitest-environment jsdom
//
// THE RF PANADAPTER'S SPANS, by value: the band window per band (the reported 30 m, plus 20 m, 6 m and
// 2 m), what each source is asked to sweep for it, when a sweep already fits, and that the ask goes
// out once — never before a sweep is known, never while transmitting, never for a source Nexus does
// not set.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { renderHook } from '@testing-library/react'
import {
  bandWindow,
  loadRfSpan,
  onlyPart,
  saveRfSpan,
  spanAsk,
  sweepFits,
  useSpanAsk,
  type SpanAsk,
} from './rfSpan'

describe('bandWindow', () => {
  it('30 m: the band with 5 kHz either side (the reported case)', () => {
    expect(bandWindow(10_120_000)).toEqual({ band: '30m', loHz: 10_095_000, hiHz: 10_155_000 })
  })
  it('20 m: 14.000-14.350 with the 25 kHz cap', () => {
    expect(bandWindow(14_074_000)).toEqual({ band: '20m', loHz: 13_975_000, hiHz: 14_375_000 })
  })
  it('6 m and 2 m: 4 MHz bands, 25 kHz margins', () => {
    expect(bandWindow(50_313_000)).toEqual({ band: '6m', loHz: 49_975_000, hiHz: 54_025_000 })
    expect(bandWindow(144_174_000)).toEqual({ band: '2m', loHz: 143_975_000, hiHz: 148_025_000 })
  })
  it('off every band, or with no dial: none', () => {
    expect(bandWindow(9_000_000)).toBeNull()
    expect(bandWindow(null)).toBeNull()
    expect(bandWindow(0)).toBeNull()
  })
})

describe('spanAsk', () => {
  const b30 = { loHz: 10_095_000, hiHz: 10_155_000 }
  it('Icom: the narrowest rung that reaches the farther band edge from the dial', () => {
    // 10.120: 35 kHz to the top edge → ±50 kHz.
    expect(spanAsk('civ', 10_120_000, b30)).toEqual({ kind: 'civ', halfHz: 50_000 })
    // 20 m from 14.074: 301 kHz to the top → ±500 kHz.
    expect(spanAsk('civ', 14_074_000, { loHz: 13_975_000, hiHz: 14_375_000 })).toEqual({ kind: 'civ', halfHz: 500_000 })
    // ±25k: exactly the rung.
    expect(spanAsk('civ', 14_200_000, { loHz: 14_175_000, hiHz: 14_225_000 })).toEqual({ kind: 'civ', halfHz: 25_000 })
  })
  it('Icom on 2 m: more than the widest rung → the widest', () => {
    expect(spanAsk('civ', 144_174_000, { loHz: 143_975_000, hiHz: 148_025_000 })).toEqual({ kind: 'civ', halfHz: 500_000 })
  })
  it('Flex: twice the reach, to the next kHz', () => {
    expect(spanAsk('flex', 10_120_000, b30)).toEqual({ kind: 'flex', hz: 70_000 })
    expect(spanAsk('flex', 14_074_000, { loHz: 13_975_000, hiHz: 14_375_000 })).toEqual({ kind: 'flex', hz: 602_000 })
  })
  it('TCI, SDRconnect and the FT-710 are not asked; nor is anything without a dial or a window', () => {
    for (const s of ['tci', 'sdrconnect', 'yaesu', 'audio']) expect(spanAsk(s, 10_120_000, b30)).toBeNull()
    expect(spanAsk('civ', null, b30)).toBeNull()
    expect(spanAsk('civ', 10_120_000, null)).toBeNull()
  })
})

describe('sweepFits and onlyPart', () => {
  const want = { loHz: 10_095_000, hiHz: 10_155_000 }
  const ask: SpanAsk = { kind: 'civ', halfHz: 50_000 }
  it('a sweep that holds the band and is at most one rung wider fits', () => {
    expect(sweepFits({ loHz: 10_070_000, hiHz: 10_170_000 }, want, ask)).toBe(true)
    expect(sweepFits({ loHz: 10_020_000, hiHz: 10_220_000 }, want, ask)).toBe(true)
  })
  it('a sweep that misses an edge, or is far wider (the 2 MHz report), does not', () => {
    expect(sweepFits({ loHz: 10_095_000, hiHz: 10_145_000 }, want, ask)).toBe(false)
    expect(sweepFits({ loHz: 9_120_000, hiHz: 11_120_000 }, want, ask)).toBe(false)
  })
  it('onlyPart: said when what is drawn falls short of the band', () => {
    expect(onlyPart({ loHz: 10_095_000, hiHz: 10_130_000 }, want)).toBe(true)
    expect(onlyPart(want, want)).toBe(false)
    expect(onlyPart(null, want)).toBe(false)
  })
})

describe('useSpanAsk', () => {
  const want = { loHz: 10_095_000, hiHz: 10_155_000 }
  const wide = { loHz: 9_620_000, hiHz: 10_620_000 }
  const base = { source: 'civ', dialHz: 10_120_000, want, choice: 'band', sweep: wide, transmitting: false }
  it('asks once for a sweep that does not fit, and not again on the next render', () => {
    const send = vi.fn()
    const { rerender } = renderHook((p) => useSpanAsk({ ...p, send }), { initialProps: base })
    expect(send).toHaveBeenCalledTimes(1)
    expect(send).toHaveBeenCalledWith({ kind: 'civ', halfHz: 50_000 })
    rerender({ ...base, sweep: { ...wide } })
    expect(send, 'a radio that refused is told once').toHaveBeenCalledTimes(1)
  })
  it('the Flex pan is asked for its bandwidth', () => {
    const send = vi.fn()
    renderHook(() => useSpanAsk({ ...base, source: 'flex', send }))
    expect(send).toHaveBeenCalledWith({ kind: 'flex', hz: 70_000 })
  })
  it('nothing before a sweep is known, while transmitting, when it already fits, or for an SDR', () => {
    const send = vi.fn()
    renderHook(() => useSpanAsk({ ...base, sweep: null, send }))
    renderHook(() => useSpanAsk({ ...base, transmitting: true, send }))
    renderHook(() => useSpanAsk({ ...base, sweep: { loHz: 10_070_000, hiHz: 10_170_000 }, send }))
    renderHook(() => useSpanAsk({ ...base, source: 'sdrconnect', send }))
    renderHook(() => useSpanAsk({ ...base, source: null, send }))
    expect(send).not.toHaveBeenCalled()
  })
  it('held while keyed, sent once the over ends', () => {
    const send = vi.fn()
    const { rerender } = renderHook((p) => useSpanAsk({ ...p, send }), { initialProps: { ...base, transmitting: true } })
    expect(send).not.toHaveBeenCalled()
    rerender({ ...base, transmitting: false })
    expect(send).toHaveBeenCalledTimes(1)
  })
  it('a new band is a new ask', () => {
    const send = vi.fn()
    const { rerender } = renderHook((p) => useSpanAsk({ ...p, send }), { initialProps: base })
    rerender({ ...base, dialHz: 14_200_000, want: { loHz: 13_975_000, hiHz: 14_375_000 }, sweep: wide })
    expect(send.mock.calls).toEqual([[{ kind: 'civ', halfHz: 50_000 }], [{ kind: 'civ', halfHz: 250_000 }]])
  })
  it('a new chip is a new ask, even for the same rung', () => {
    const send = vi.fn()
    const { rerender } = renderHook((p) => useSpanAsk({ ...p, send }), { initialProps: base })
    rerender({ ...base, choice: '±25k' })
    expect(send).toHaveBeenCalledTimes(2)
  })
  it('a new rung is a new ask; tuning back across it does not ask again', () => {
    const send = vi.fn()
    // The radio sweeps centred on the dial.
    const at = (dial: number, half: number) => ({ loHz: dial - half, hiHz: dial + half })
    const { rerender } = renderHook((p) => useSpanAsk({ ...p, send }), {
      initialProps: { ...base, sweep: at(10_120_000, 50_000) },
    })
    expect(send, 'already there: nothing asked').not.toHaveBeenCalled()
    // 10.100: 55 kHz to the top edge, past ±50k → ±100k.
    rerender({ ...base, dialHz: 10_100_000, sweep: at(10_100_000, 50_000) })
    expect(send.mock.calls).toEqual([[{ kind: 'civ', halfHz: 100_000 }]])
    rerender({ ...base, dialHz: 10_100_000, sweep: at(10_100_000, 100_000) })
    rerender({ ...base, sweep: at(10_120_000, 100_000) })
    rerender({ ...base, dialHz: 10_100_000, sweep: at(10_100_000, 100_000) })
    rerender({ ...base, sweep: at(10_120_000, 100_000) })
    expect(send).toHaveBeenCalledTimes(1)
  })
  it("after the ask lands, the operator's own span stands (radio knob, Rig Scope chip, Flex pan chip)", () => {
    const send = vi.fn()
    const fit = { loHz: 10_070_000, hiHz: 10_170_000 }
    const { rerender } = renderHook((p) => useSpanAsk({ ...p, send }), { initialProps: base })
    rerender({ ...base, sweep: fit }) // the ask landed
    rerender({ ...base, sweep: { loHz: 10_110_000, hiHz: 10_130_000 } }) // the operator: ±10k
    rerender({ ...base, sweep: fit })
    rerender({ ...base, sweep: { loHz: 9_870_000, hiHz: 10_370_000 } }) // the operator: ±250k
    rerender({ ...base, sweep: { loHz: 9_870_000, hiHz: 10_370_000 }, transmitting: true })
    rerender({ ...base, sweep: { loHz: 9_870_000, hiHz: 10_370_000 } })
    expect(send.mock.calls).toEqual([[{ kind: 'civ', halfHz: 50_000 }]])
  })
  it('the same for a sweep that already fitted Band when it was first seen', () => {
    const send = vi.fn()
    const fit = { loHz: 10_070_000, hiHz: 10_170_000 }
    const { rerender } = renderHook((p) => useSpanAsk({ ...p, send }), { initialProps: { ...base, sweep: fit } })
    rerender({ ...base, sweep: { loHz: 10_110_000, hiHz: 10_130_000 } })
    expect(send).not.toHaveBeenCalled()
  })
  it('a radio that refused is told once, however many overs follow', () => {
    const send = vi.fn()
    const { rerender } = renderHook((p) => useSpanAsk({ ...p, send }), { initialProps: base })
    for (let i = 0; i < 3; i++) {
      rerender({ ...base, sweep: { ...wide }, transmitting: true })
      rerender({ ...base, sweep: { ...wide }, transmitting: false })
    }
    expect(send).toHaveBeenCalledTimes(1)
  })
  it('two scopes asking different spans of one radio ask once each, never in turn', () => {
    const a = vi.fn()
    const b = vi.fn()
    const fitA = { loHz: 10_070_000, hiHz: 10_170_000 }
    const fitB = { loHz: 10_110_000, hiHz: 10_130_000 }
    const wantB = { loHz: 10_115_000, hiHz: 10_125_000 }
    const bBase = { ...base, want: wantB, choice: '±5k' }
    const ha = renderHook((p) => useSpanAsk({ ...p, send: a }), { initialProps: base })
    const hb = renderHook((p) => useSpanAsk({ ...p, send: b }), { initialProps: bBase })
    for (let i = 0; i < 5; i++) {
      ha.rerender({ ...base, sweep: fitA })
      hb.rerender({ ...bBase, sweep: fitA })
      ha.rerender({ ...base, sweep: fitB })
      hb.rerender({ ...bBase, sweep: fitB })
    }
    expect(a.mock.calls).toEqual([[{ kind: 'civ', halfHz: 50_000 }]])
    expect(b.mock.calls).toEqual([[{ kind: 'civ', halfHz: 5_000 }]])
  })
})

describe('the stored choice, per window', () => {
  afterEach(() => {
    localStorage.clear()
    window.history.replaceState({}, '', '/')
  })
  const ids = ['band', 'full', '±25k'] as const
  it('defaults to Band, and a stored choice wins', () => {
    expect(loadRfSpan('phone', ids, 'band')).toBe('band')
    saveRfSpan('phone', 'full')
    expect(loadRfSpan('phone', ids, 'band')).toBe('full')
    expect(loadRfSpan('cw', ids, 'band'), 'each cockpit its own').toBe('band')
  })
  it('a choice kept under the old station-wide key is still read', () => {
    localStorage.setItem('nexus.cw.rfSpan', '±25k')
    expect(loadRfSpan('cw', ids, 'band')).toBe('±25k')
    window.history.replaceState({}, '', '/?panel=cw&instance=w2')
    expect(loadRfSpan('cw', ids, 'band'), 'a pop-out with none of its own reads it too').toBe('±25k')
  })
  it("a pop-out's choice is its own and leaves the main window's alone", () => {
    saveRfSpan('phone', 'band')
    window.history.replaceState({}, '', '/?panel=phone&instance=w2')
    saveRfSpan('phone', '±25k')
    expect(loadRfSpan('phone', ids, 'band')).toBe('±25k')
    expect(localStorage.getItem('nexus.phone.rfSpan.phone.w2')).toBe('±25k')
    window.history.replaceState({}, '', '/')
    expect(loadRfSpan('phone', ids, 'band'), 'the main window kept Band').toBe('band')
  })
  it('a docked cockpit on Band and a torn-off one on ±5k, one radio: each asks once, never in turn', () => {
    const spans = ['band', 'full', '±5k'] as const
    saveRfSpan('phone', 'band')
    window.history.replaceState({}, '', '/?panel=phone&instance=w2')
    saveRfSpan('phone', '±5k')
    const torn = loadRfSpan('phone', spans, 'band')
    window.history.replaceState({}, '', '/')
    const docked = loadRfSpan('phone', spans, 'band')
    expect([docked, torn], 'premise: two windows, two stored spans').toEqual(['band', '±5k'])
    const dial = 10_120_000
    const wantOf = (id: string) => (id === 'band' ? bandWindow(dial) : { loHz: dial - 5_000, hiHz: dial + 5_000 })
    const props = (id: string, sweep: { loHz: number; hiHz: number }) =>
      ({ source: 'civ', dialHz: dial, want: wantOf(id), choice: id, sweep, transmitting: false })
    // The one radio's sweep, centred on the dial, as each window's ask lands and the other's replaces it.
    const at = (half: number) => ({ loHz: dial - half, hiHz: dial + half })
    const a = vi.fn()
    const b = vi.fn()
    const ha = renderHook((p) => useSpanAsk({ ...p, send: a }), { initialProps: props(docked, at(500_000)) })
    const hb = renderHook((p) => useSpanAsk({ ...p, send: b }), { initialProps: props(torn, at(500_000)) })
    for (let i = 0; i < 10; i++) {
      for (const half of [50_000, 5_000]) {
        ha.rerender(props(docked, at(half)))
        hb.rerender(props(torn, at(half)))
      }
    }
    expect(a.mock.calls).toEqual([[{ kind: 'civ', halfHz: 50_000 }]])
    expect(b.mock.calls).toEqual([[{ kind: 'civ', halfHz: 5_000 }]])
  })
  it('a stored value that is no span reads as the default', () => {
    localStorage.setItem('nexus.cw.rfSpan', '±3M')
    expect(loadRfSpan('cw', ids, 'band')).toBe('band')
  })
})
