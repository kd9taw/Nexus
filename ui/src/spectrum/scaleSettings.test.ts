// @vitest-environment jsdom
import { act, renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { AVERAGE_STEPS_MS } from './scaleAverage'
import {
  loadScaleSettings,
  SCALE_KEYS,
  saveScaleSettings,
  scaleDefaults,
  useScaleSettings,
  type ScaleSettings,
  type ScopeCockpit,
} from './scaleSettings'

const COCKPITS = Object.keys(SCALE_KEYS) as ScopeCockpit[]
const DIGITAL: ScopeCockpit[] = ['operate', 'js8', 'rtty', 'psk', 'sstv', 'tempo']

/** mulberry32: the same settings on every run. */
function prng(seed: number): () => number {
  let a = seed >>> 0
  return () => {
    a = (a + 0x6d2b79f5) >>> 0
    let t = a
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

const store = (cockpit: ScopeCockpit, raw: string) => window.localStorage.setItem(SCALE_KEYS[cockpit], raw)

beforeEach(() => {
  window.localStorage.clear()
  window.history.replaceState({}, '', '/')
})

afterEach(() => {
  vi.restoreAllMocks()
})

describe('a cockpit’s scale settings', () => {
  it('start from the specification’s defaults, with CW’s averaging off', () => {
    for (const c of COCKPITS) {
      expect(loadScaleSettings(c), c).toEqual({
        averageMs: c === 'cw' ? 0 : 250,
        detector: 'peak',
        gain: 0,
        zero: 0,
        window: 'balanced',
      })
    }
  })

  it('round-trip: whatever is saved is what loads, for every cockpit', () => {
    const r = prng(118)
    const pick = <T,>(xs: readonly T[]) => xs[Math.floor(r() * xs.length)]
    for (let k = 0; k < 200; k++) {
      const c = pick(COCKPITS)
      const s: ScaleSettings = {
        averageMs: pick(AVERAGE_STEPS_MS),
        detector: pick(['peak', 'average'] as const),
        // `+ 0`: a slider step rounds to −0 below zero, which no slider hands a host.
        gain: k % 7 === 0 ? pick([-1, 1]) : Math.round((r() * 2 - 1) * 20) / 20 + 0,
        zero: k % 5 === 0 ? pick([-1, 1]) : r() * 2 - 1,
        window: pick(['fast', 'balanced', 'sharp'] as const),
      }
      saveScaleSettings(c, s)
      expect(loadScaleSettings(c), `${c} ${JSON.stringify(s)}`).toEqual(s)
    }
  })

  it('belong to their own cockpit', () => {
    saveScaleSettings('phone', { ...scaleDefaults('phone'), averageMs: 2000, gain: 0.5 })
    expect(loadScaleSettings('cw')).toEqual(scaleDefaults('cw'))
    expect(loadScaleSettings('operate')).toEqual(scaleDefaults('operate'))
  })

  it('are shared by every window showing the cockpit (a torn-off waterfall shares the docked one’s)', () => {
    saveScaleSettings('operate', { ...scaleDefaults('operate'), zero: 0.4 })
    window.history.replaceState({}, '', '/?panel=waterfall')
    expect(loadScaleSettings('operate').zero).toBe(0.4)
    saveScaleSettings('operate', { ...scaleDefaults('operate'), zero: -0.2 })
    window.history.replaceState({}, '', '/')
    expect(loadScaleSettings('operate').zero).toBe(-0.2)
  })
})

describe('clamped on load', () => {
  it('pulls an out-of-range value into range, and the averaging time onto a step the control offers', () => {
    const cases: [string, Partial<ScaleSettings>][] = [
      ['{"averageMs":99999}', { averageMs: 2000 }],
      ['{"averageMs":-40}', { averageMs: 0 }],
      ['{"averageMs":300}', { averageMs: 250 }],
      ['{"averageMs":760}', { averageMs: 1000 }],
      ['{"averageMs":1e999}', {}],
      ['{"gain":7,"zero":-3}', { gain: 1, zero: -1 }],
      ['{"gain":-1.0001,"zero":0.25}', { gain: -1, zero: 0.25 }],
    ]
    for (const [raw, want] of cases) {
      store('phone', raw)
      expect(loadScaleSettings('phone'), raw).toEqual({ ...scaleDefaults('phone'), ...want })
    }
  })

  it('reads what is not a value as never stored: NaN, strings, foreign shapes, broken JSON', () => {
    for (const raw of [
      '{"averageMs":NaN}',
      '{"averageMs":"250","gain":"0.5","zero":null,"detector":"rms","window":"ultra"}',
      '{"detector":7,"window":{"id":"fast"}}',
      '[250,"average"]',
      '"average"',
      'null',
      '42',
      '',
      '{',
    ]) {
      store('rtty', raw)
      expect(loadScaleSettings('rtty'), JSON.stringify(raw)).toEqual(scaleDefaults('rtty'))
    }
  })

  it('reads an old record field by field: what it has is kept, what it lacks is the default', () => {
    store('cw', '{"detector":"average"}')
    expect(loadScaleSettings('cw')).toEqual({ ...scaleDefaults('cw'), detector: 'average' })
    store('cw', '{"detector":"average","averageMs":"fast","window":"sharp"}')
    expect(loadScaleSettings('cw')).toEqual({ ...scaleDefaults('cw'), detector: 'average', window: 'sharp' })
  })

  it('takes what the controls stored before the record existed, clamped the same way', () => {
    // The rig scope's window (Phone and CW share it today) and the digital waterfall's app-wide G and Z.
    window.localStorage.setItem('nexus.phonescope.win', 'sharp')
    window.localStorage.setItem('nexus.waterfall.gain', '0.4')
    window.localStorage.setItem('nexus.waterfall.zero', '-2')
    expect(loadScaleSettings('phone').window).toBe('sharp')
    expect(loadScaleSettings('cw').window).toBe('sharp')
    for (const c of DIGITAL) {
      expect(loadScaleSettings(c), c).toEqual({ ...scaleDefaults(c), gain: 0.4, zero: -1 })
    }
    // Never crossed: the rig scope's G/Z were never stored, the waterfall had no window.
    expect(loadScaleSettings('phone').gain).toBe(0)
    expect(loadScaleSettings('operate').window).toBe('balanced')
    // Garbage in the old keys is the default, as the old controls read it.
    window.localStorage.setItem('nexus.phonescope.win', 'ultra')
    window.localStorage.setItem('nexus.waterfall.gain', 'abc')
    expect(loadScaleSettings('phone').window).toBe('balanced')
    expect(loadScaleSettings('psk').gain).toBe(0)
  })

  it('starts the RF scope pane from the defaults: the audio waterfall’s old G and Z are another axis', () => {
    window.localStorage.setItem('nexus.phonescope.win', 'sharp')
    window.localStorage.setItem('nexus.waterfall.gain', '0.4')
    window.localStorage.setItem('nexus.waterfall.zero', '0.3')
    expect(loadScaleSettings('rfpan')).toEqual(scaleDefaults('rfpan'))
    // Control: the same stored keys DO reach a digital waterfall's record and Phone's.
    expect(loadScaleSettings('operate').gain).toBe(0.4)
    expect(loadScaleSettings('phone').window).toBe('sharp')
  })

  it('stops reading the old keys once the record has the field, and never writes them', () => {
    window.localStorage.setItem('nexus.phonescope.win', 'sharp')
    window.localStorage.setItem('nexus.waterfall.zero', '0.5')
    saveScaleSettings('phone', { ...loadScaleSettings('phone'), window: 'fast' })
    saveScaleSettings('js8', { ...loadScaleSettings('js8'), zero: -0.25 })
    expect(loadScaleSettings('phone').window).toBe('fast')
    expect(loadScaleSettings('js8').zero).toBe(-0.25)
    expect(window.localStorage.getItem('nexus.phonescope.win')).toBe('sharp')
    expect(window.localStorage.getItem('nexus.waterfall.zero')).toBe('0.5')
  })

  it('keeps the fields a newer build wrote that this one does not know', () => {
    store('sstv', '{"averageMs":500,"spectrumTilt":3,"window":"fast"}')
    saveScaleSettings('sstv', { ...loadScaleSettings('sstv'), averageMs: 1000 })
    expect(JSON.parse(window.localStorage.getItem(SCALE_KEYS.sstv)!)).toEqual({
      averageMs: 1000,
      spectrumTilt: 3,
      window: 'fast',
      detector: 'peak',
      gain: 0,
      zero: 0,
    })
  })

  it('treats blocked storage as nothing stored, and a failed write as session-only', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('full')
    })
    expect(loadScaleSettings('phone')).toEqual(scaleDefaults('phone'))
    expect(() => saveScaleSettings('phone', scaleDefaults('phone'))).not.toThrow()
  })
})

describe('useScaleSettings', () => {
  it('loads, clamps every change, stores it, and re-renders', () => {
    store('phone', '{"averageMs":500}')
    const { result } = renderHook(() => useScaleSettings('phone'))
    expect(result.current[0].averageMs).toBe(500)
    act(() => result.current[1]({ averageMs: 9999 }))
    act(() => result.current[1]({ detector: 'average', gain: -4 }))
    expect(result.current[0]).toEqual({ ...scaleDefaults('phone'), averageMs: 2000, detector: 'average', gain: -1 })
    expect(loadScaleSettings('phone')).toEqual(result.current[0])
  })

  it("shows a new cockpit's record when the host's cockpit changes, and writes there, not to the old one", () => {
    store('cw', '{"window":"sharp"}')
    const { result, rerender } = renderHook(({ c }) => useScaleSettings(c), {
      initialProps: { c: 'phone' as ScopeCockpit },
    })
    act(() => result.current[1]({ zero: 0.5 }))
    rerender({ c: 'cw' })
    expect(result.current[0]).toEqual({ ...scaleDefaults('cw'), window: 'sharp' })
    act(() => result.current[1]({ gain: 0.25 }))
    expect(loadScaleSettings('cw')).toEqual({ ...scaleDefaults('cw'), window: 'sharp', gain: 0.25 })
    expect(loadScaleSettings('phone')).toEqual({ ...scaleDefaults('phone'), zero: 0.5 })
  })
})
