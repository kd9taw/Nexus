// @vitest-environment jsdom
//
// THE WATERFALL'S TX/RX MARKERS AND AXIS STRIP PAINT WITH THE THEME'S TOKENS (2026-09-26).
//
// They were literals — `rgba(255,70,70)` / `#ff5a5a` for TX, `#3ddc8c` for RX, and an axis strip
// picked by `theme === 'light' ? … : …` — so the scope could never agree with the ON AIR sign, and
// the strip went pale in the light theme while the display around it stayed dark. Now the overlay
// reads `--tx`, `--rx`, `--well-bg` and `--well-ink` off its own canvas (the MiniSpectrum
// pattern), so the waterfall's `.well` stage decides what they are, and it reads them again when
// the theme changes.
//
// WHY A FAKE CONTEXT IS FAITHFUL HERE. jsdom has no 2D canvas. The overlay only WRITES to its
// context and reads nothing back, so a recording stand-in is a stand-in for the paint target, not
// for the thing under test: the component's own effect, its own rAF loop and its own drawOverlay
// run as shipped (the PhoneScope.draw.test.tsx argument). The tokens are set on <html> with
// sentinel values no theme uses, which jsdom's computed style hands down to the canvas — so a
// sentinel in the recording can only have come from a token read.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { Waterfall } from './Waterfall'

vi.mock('../api', () => ({
  getSpectrumRow: () => new Promise(() => {}),
}))

type Call = { op: string; args: unknown[]; fillStyle: string; alpha: number }

/** A 2D context that records every call with the paint state in force at the time. */
function recordingCtx() {
  const calls: Call[] = []
  const state: Record<string, unknown> = {
    fillStyle: '#000000',
    strokeStyle: '#000000',
    globalAlpha: 1,
    font: '',
    lineWidth: 1,
    textAlign: 'left',
    textBaseline: 'alphabetic',
  }
  const ctx = new Proxy(state, {
    get(target, prop) {
      if (typeof prop !== 'string') return undefined
      if (prop in target) return target[prop]
      if (prop === 'measureText') return () => ({ width: 10 })
      if (prop === 'createLinearGradient') return () => ({ addColorStop() {} })
      return (...args: unknown[]) => {
        calls.push({ op: prop, args, fillStyle: String(target.fillStyle), alpha: Number(target.globalAlpha) })
      }
    },
    set(target, prop, value) {
      if (typeof prop === 'string') target[prop] = value
      return true
    },
  })
  return { ctx, calls }
}

const TX = '#a1b2c3'
const RX = '#c3b2a1'
const WELL_BG = '#010203'
const WELL_INK = '#fefdfc'
const TX2 = '#123456'
const RX2 = '#654321'

let overlay: ReturnType<typeof recordingCtx>
let realRaf: typeof requestAnimationFrame
let realCaf: typeof cancelAnimationFrame

function setTokens(tokens: Record<string, string>) {
  for (const [k, v] of Object.entries(tokens)) document.documentElement.style.setProperty(k, v)
}

beforeEach(() => {
  overlay = recordingCtx()
  const spectrum = recordingCtx()
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(function (this: HTMLCanvasElement) {
    return (this.classList.contains('waterfall-overlay') ? overlay.ctx : spectrum.ctx) as unknown as CanvasRenderingContext2D
  } as unknown as typeof HTMLCanvasElement.prototype.getContext)
  globalThis.ImageData = class {
    data: Uint8ClampedArray
    width: number
    height: number
    constructor(d: Uint8ClampedArray, w: number, h: number) {
      this.data = d
      this.width = w
      this.height = h
    }
  } as unknown as typeof ImageData
  window.matchMedia = ((q: string) =>
    ({ matches: false, media: q, addEventListener: () => {}, removeEventListener: () => {} }) as unknown as MediaQueryList) as typeof window.matchMedia
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  // jsdom stamps rAF from the window's time origin while the loop's clock is performance.now();
  // put the callback on the same clock, as a browser does (PhoneScope.draw.test.tsx).
  realRaf = globalThis.requestAnimationFrame
  realCaf = globalThis.cancelAnimationFrame
  globalThis.requestAnimationFrame = ((cb: FrameRequestCallback) =>
    setTimeout(() => cb(performance.now()), 16) as unknown as number) as typeof requestAnimationFrame
  globalThis.cancelAnimationFrame = ((id: number) => clearTimeout(id as unknown as NodeJS.Timeout)) as typeof cancelAnimationFrame
  setTokens({ '--tx': TX, '--rx': RX, '--well-bg': WELL_BG, '--well-ink': WELL_INK })
})

afterEach(() => {
  cleanup()
  globalThis.requestAnimationFrame = realRaf
  globalThis.cancelAnimationFrame = realCaf
  document.documentElement.removeAttribute('style')
  vi.restoreAllMocks()
})

const frames = (ms: number) => act(() => new Promise((r) => setTimeout(r, ms)))

/** The fills of the most recent overlay frame (each frame opens with a clearRect). */
function lastFrame(): Call[] {
  const at = overlay.calls.map((c) => c.op).lastIndexOf('clearRect')
  return overlay.calls.slice(at)
}
const markerLines = (f: Call[]) => f.filter((c) => c.op === 'fillRect' && c.args[2] === 2)
const label = (f: Call[], text: string) => f.find((c) => c.op === 'fillText' && c.args[0] === text)

describe('the TX/RX markers are the theme tokens, not literals', () => {
  it('paints the TX marker in --tx and the RX marker in --rx, read off the canvas', async () => {
    render(<Waterfall transmitting={false} rxOffsetHz={1500} txOffsetHz={1000} theme="dark" />)
    await frames(120)
    const f = lastFrame()
    expect(markerLines(f).map((c) => c.fillStyle), 'the marker lines').toEqual([TX, RX])
    expect(label(f, 'TX')?.fillStyle, 'the TX label').toBe(TX)
    expect(label(f, 'RX')?.fillStyle, 'the RX label').toBe(RX)
  })

  it('keeps the TX marker brighter while transmitting, by alpha — the hue is the token either way', async () => {
    const { rerender } = render(<Waterfall transmitting={false} rxOffsetHz={1500} txOffsetHz={1000} theme="dark" />)
    await frames(120)
    const idle = markerLines(lastFrame())[0]
    rerender(<Waterfall transmitting rxOffsetHz={1500} txOffsetHz={1000} theme="dark" />)
    await frames(120)
    const keyed = markerLines(lastFrame())[0]
    expect([idle.fillStyle, keyed.fillStyle]).toEqual([TX, TX])
    expect(keyed.alpha, 'keyed TX marker is not stronger than idle').toBeGreaterThan(idle.alpha)
  })

  it('reads the tokens again when the theme changes', async () => {
    const { rerender } = render(<Waterfall transmitting={false} rxOffsetHz={1500} txOffsetHz={1000} theme="dark" />)
    await frames(120)
    expect(markerLines(lastFrame()).map((c) => c.fillStyle)).toEqual([TX, RX])
    setTokens({ '--tx': TX2, '--rx': RX2 })
    rerender(<Waterfall transmitting={false} rxOffsetHz={1500} txOffsetHz={1000} theme="light" />)
    await frames(120)
    expect(markerLines(lastFrame()).map((c) => c.fillStyle), 'stale marker colours after a theme change').toEqual([TX2, RX2])
  })

  it('a named cursor given as var(--rx) paints the token (PSK RX, RTTY mark)', async () => {
    render(
      <Waterfall
        transmitting={false}
        rxOffsetHz={1500}
        txOffsetHz={0}
        theme="dark"
        cursors={[{ hz: 1500, color: 'var(--rx)', label: 'RX' }]}
      />,
    )
    await frames(120)
    const f = lastFrame()
    expect(markerLines(f).map((c) => c.fillStyle)).toEqual([RX])
    expect(label(f, 'RX')?.fillStyle).toBe(RX)
  })
})

describe('the axis strip is part of the dark display', () => {
  it('paints its ground in --well-bg and its scale in --well-ink, in either theme', async () => {
    for (const theme of ['dark', 'light'] as const) {
      overlay.calls.length = 0
      render(<Waterfall transmitting={false} rxOffsetHz={1500} txOffsetHz={1000} theme={theme} />)
      await frames(120)
      const f = lastFrame()
      const ground = f.find((c) => c.op === 'fillRect')
      expect(ground?.fillStyle, `${theme}: the strip's ground`).toBe(WELL_BG)
      const digits = f.filter((c) => c.op === 'fillText' && /^\d+$/.test(String(c.args[0])))
      expect(digits.length, `${theme}: no scale drawn`).toBeGreaterThan(0)
      expect([...new Set(digits.map((c) => c.fillStyle))], `${theme}: the scale's ink`).toEqual([WELL_INK])
      cleanup()
    }
  })
})

describe('no TX/RX marker literal survives in the canvas code', () => {
  // The literals the markers used to be. A copy of one anywhere in the UI source means a marker
  // (or a named cursor standing in for one) has gone back to ignoring the theme. Styles are not
  // swept: `var(--state-good, #3ddc8c)` there is a fallback behind a declared token, and the
  // hosted site's own `--rs-signal` is chrome, not a marker.
  const MARKER = /#ff5a5a|#3ddc8c|rgba\(\s*255\s*,\s*70\s*,\s*70|rgba\(\s*255\s*,\s*90\s*,\s*90|rgba\(\s*60\s*,\s*220\s*,\s*140/i
  const SRC = resolve(process.cwd(), 'src')
  const files: string[] = []
  const walk = (dir: string) => {
    for (const e of readdirSync(dir)) {
      const p = join(dir, e)
      if (statSync(p).isDirectory()) walk(p)
      else if (/\.tsx?$/.test(p) && !/\.test\.tsx?$/.test(p)) files.push(p)
    }
  }
  walk(SRC)

  it('finds the source it sweeps (the sweep cannot silently empty out)', () => {
    expect(files.length).toBeGreaterThan(300)
    expect(files.some((f) => f.endsWith('Waterfall.tsx'))).toBe(true)
  })

  it('no source file carries one', () => {
    const hits = files.flatMap((f) =>
      readFileSync(f, 'utf8')
        .split('\n')
        .flatMap((line, i) => (MARKER.test(line) ? [`${f.slice(SRC.length + 1)}:${i + 1}: ${line.trim()}`] : [])),
    )
    expect(hits, `marker literals:\n${hits.join('\n')}`).toEqual([])
  })
})
