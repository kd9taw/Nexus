// @vitest-environment jsdom
//
// THE FAST GRAPH IS A DARK DISPLAY IN BOTH THEMES (2026-09-26, "Light frame, dark displays").
//
// It painted its own ground by theme — `dark ? '#0b0f14' : '#f2f5f8'` — while its trace, its
// second ticks and its decode markers are drawn for a dark ground in every theme: in the light
// theme the white 1-second ticks were invisible and the green trace sat at ~1.6:1 on near-white.
// It now reads `--well-bg` (and `--well-grid` for its ticks and divider) off its own canvas, the
// MiniSpectrum pattern, and repaints when the theme changes even with no new samples arriving.
//
// A recording context stands in for the paint target only (jsdom has no 2D canvas; the graph
// reads nothing back). Tokens are set on <html> as sentinels no theme uses.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { FastGraph } from './FastGraph'

let batches: Array<Array<{ seq: number; unixMs: number; rms: number }>> = []
vi.mock('../api', () => ({
  getFastPower: () => Promise.resolve(batches.shift() ?? []),
}))

type Call = { op: string; args: unknown[]; fillStyle: string }
let calls: Call[] = []
/** Fires the ResizeObserver callback the component registers, as a real layout would. */
let resized: (() => void) | null = null

beforeEach(() => {
  calls = []
  batches = [[{ seq: 1, unixMs: Date.now(), rms: 0.2 }]]
  const state: Record<string, unknown> = { fillStyle: '#000000', strokeStyle: '#000000', lineWidth: 1, font: '' }
  const ctx = new Proxy(state, {
    get(target, prop) {
      if (typeof prop !== 'string') return undefined
      if (prop in target) return target[prop]
      return (...args: unknown[]) => calls.push({ op: prop, args, fillStyle: String(target.fillStyle) })
    },
    set(target, prop, value) {
      if (typeof prop === 'string') target[prop] = value
      return true
    },
  })
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(ctx as unknown as CanvasRenderingContext2D)
  globalThis.ResizeObserver = class {
    constructor(cb: ResizeObserverCallback) {
      resized = () => cb([] as unknown as ResizeObserverEntry[], this as unknown as ResizeObserver)
    }
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  document.documentElement.style.setProperty('--well-bg', '#010203')
  document.documentElement.style.setProperty('--well-grid', '#040506')
})

afterEach(() => {
  cleanup()
  resized = null
  document.documentElement.removeAttribute('style')
  vi.restoreAllMocks()
})

const settle = (ms = 60) => act(() => new Promise((r) => setTimeout(r, ms)))
/** Each draw opens by filling the whole ground: the fills at (0, 0). */
const grounds = () => calls.filter((c) => c.op === 'fillRect' && c.args[0] === 0 && c.args[1] === 0).map((c) => c.fillStyle)

describe('the Fast Graph paints a dark display in either theme', () => {
  it.each(['dark', 'light'] as const)('its ground is --well-bg in the %s theme', async (theme) => {
    render(<FastGraph periodS={15} theme={theme} />)
    await act(async () => resized?.())
    await settle()
    expect(grounds().length, 'nothing was drawn').toBeGreaterThan(0)
    expect([...new Set(grounds())], 'the ground').toEqual(['#010203'])
  })

  it('its second ticks and its divider are drawn in --well-grid', async () => {
    render(<FastGraph periodS={15} theme="light" />)
    await act(async () => resized?.())
    await settle()
    const thin = calls.filter((c) => c.op === 'fillRect' && !(c.args[0] === 0 && c.args[1] === 0) && c.fillStyle !== '#010203')
    const ticksAndDivider = thin.filter((c) => !/^rgba\(255,\s*200,\s*80/.test(c.fillStyle)) // not the amber decode marks
    expect(ticksAndDivider.length, 'no ticks or divider drawn').toBeGreaterThan(0)
    expect([...new Set(ticksAndDivider.map((c) => c.fillStyle))]).toEqual(['#040506'])
  })

  it('repaints from the tokens on a theme change, with no new samples arriving', async () => {
    const { rerender } = render(<FastGraph periodS={15} theme="dark" />)
    await act(async () => resized?.())
    await settle()
    batches = [] // the band is quiet: no poll will bring a new draw
    document.documentElement.style.setProperty('--well-bg', '#0a0b0c')
    const before = calls.length
    rerender(<FastGraph periodS={15} theme="light" />)
    await settle()
    const after = calls.slice(before).filter((c) => c.op === 'fillRect' && c.args[0] === 0 && c.args[1] === 0)
    expect(after.map((c) => c.fillStyle), 'no repaint, or a stale ground, after the theme changed').toContain('#0a0b0c')
  })
})
