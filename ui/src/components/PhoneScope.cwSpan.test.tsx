// @vitest-environment jsdom
//
// CW click-to-tune is exact at every span of the CW audio scope. A click on a received tone tunes
// the dial that puts it on the pitch, and that dial must not depend on how wide the scope is
// drawn: the Filter window round the pitch and the waterfalls' Std view (200–3000 Hz) map the
// same tone to the same dial. The real draw loop, pointer handling, signal detector and sideband
// math run; only the audio row is sampled.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render } from '@testing-library/react'
import { PhoneScope } from './PhoneScope'
import { cwAudioView } from '../waterfall'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

/** The received tones (audio Hz). The row honours the window it is asked for, as the engine does. */
let tones = [750]
let seq = 0
vi.mock('../api', () => ({
  getScopeFrame: vi.fn(async (lo: number, hi: number) => ({
    seq: ++seq, tMs: Date.now(), source: 'rx', loHz: lo, hiHz: hi,
    scale: { kind: 'dbfs', loDb: -120, hiDb: 0 },
    bins: Array.from({ length: 512 }, (_, i) => {
      const hz = lo + ((i + 0.5) / 512) * (hi - lo)
      return tones.some((t) => Math.abs(hz - t) <= Math.max(4, (hi - lo) / 512)) ? 0.9 : 0.1
    }),
  })),
}))

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'performance'] })
  vi.stubGlobal('requestAnimationFrame', (cb: FrameRequestCallback) => setTimeout(() => cb(performance.now()), 16))
  vi.stubGlobal('cancelAnimationFrame', clearTimeout)
  vi.stubGlobal('PointerEvent', class extends MouseEvent {
    pointerId: number
    constructor(type: string, init: PointerEventInit = {}) { super(type, init); this.pointerId = init.pointerId ?? 1 }
  })
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} })
  vi.stubGlobal('ImageData', class {
    data: Uint8ClampedArray; width: number; height: number
    // Both forms: (width, height), which the renderer's band takes, and (data, width, height).
    constructor(a: Uint8ClampedArray | number, b: number, c?: number) {
      if (typeof a === 'number') { this.width = a; this.height = b; this.data = new Uint8ClampedArray(a * b * 4) }
      else { this.data = a; this.width = b; this.height = c ?? a.length / 4 / b }
    }
  })
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  // A 2D paint target only: `webgl2` answers null, so the renderer draws on its canvas-2D path.
  const paint = {
    fillRect() {}, clearRect() {}, putImageData() {}, beginPath() {}, closePath() {}, moveTo() {}, lineTo() {}, fillText() {}, fill() {}, stroke() {}, setLineDash() {},
    save() {}, restore() {}, rect() {}, clip() {}, measureText: (t: string) => ({ width: t.length * 6 }),
    createLinearGradient: () => ({ addColorStop() {} }),
  }
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(((kind: string) =>
    kind === '2d' ? paint : null) as unknown as HTMLCanvasElement['getContext'])
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue({ x: 0, y: 0, left: 0, top: 0,
    width: 800, height: 200, right: 800, bottom: 200, toJSON() { return {} } })
  HTMLCanvasElement.prototype.setPointerCapture = vi.fn()
  localStorage.clear()
})
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); vi.useRealTimers() })
async function draw(ms = 150) { await act(async () => { await vi.advanceTimersByTimeAsync(ms) }) }

const DIAL = 14_050_000
const PITCH = 600
const W = 800

/** Click the scope at audio frequency `tone` (Hz), at a span; the dial the click tunes. */
async function clickTone(span: 'filter' | 'std', tone: number) {
  const { loHz, hiHz } = cwAudioView(span, PITCH, 500)
  const onTune = vi.fn()
  const ui = render(
    <PhoneScope cockpit="cw" sideband="CW" dialHz={DIAL} pitchHz={PITCH} cwPitchRefDial
      viewLoHz={loHz} viewHiHz={hiHz} markerHz={PITCH} transmitting={false} active interactive
      theme="dark" filterWidthHz={500} onTune={onTune} />,
  )
  await draw(300)
  const canvas = ui.container.querySelector<HTMLCanvasElement>('canvas.ph-scope-canvas')!
  const x = ((tone - loHz) / (hiHz - loHz)) * W
  fireEvent.pointerDown(canvas, { button: 0, clientX: x, clientY: 100 })
  fireEvent.pointerUp(canvas, { button: 0, clientX: x, clientY: 100 })
  cleanup()
  expect(onTune, `${span}: the click tuned nothing`).toHaveBeenCalledOnce()
  return onTune.mock.calls[0][0].dialHz as number
}

describe('CW click-to-tune at every span', () => {
  it.each([750, 480, 820])('a tone at %i Hz tunes the same dial at Filter and at Std', async (tone) => {
    tones = [tone]
    const atFilter = await clickTone('filter', tone)
    const atStd = await clickTone('std', tone)
    expect(atStd).toBe(atFilter)
    // The dial that puts the tone on the pitch (CW, upper side): up by the tone's distance above it.
    expect(atFilter).toBe(DIAL + (tone - PITCH))
  })

  it.each([700, 520, 880])('with no tone to snap to, a click at %i Hz tunes that frequency at Filter and at Std', async (hz) => {
    tones = []
    const atFilter = await clickTone('filter', hz)
    const atStd = await clickTone('std', hz)
    expect(atFilter).toBe(DIAL + (hz - PITCH))
    expect(atStd).toBe(DIAL + (hz - PITCH))
  })
})

// ── The toolbar's start slot ────────────────────────────────────────────────────────────────
// The CW span chips ride the scope's existing toolbar strip (`.ph-scope-smeter`) rather than a row
// of their own, which cost the picture its height at 1024x768. jsdom lays nothing out, so this pins
// the structure and the cascade that keeps them on the strip's line: the strip a wrapping ROW of
// flex items, and the chip group a content-width item, never a full-width line of its own. Values
// come from the real sheets through a cascade resolved here (jsdom's CSSOM gives matching only).
const RULES: { rule: CSSStyleRule; order: number }[] = []
function loadSheets() {
  if (RULES.length) return
  for (const sheet of ['styles.css', 'cockpit-panes.css']) {
    const style = document.createElement('style')
    style.textContent = readFileSync(resolve(process.cwd(), 'src', sheet), 'utf8')
    document.head.appendChild(style)
    const walk = (rules: CSSRuleList) => {
      for (let i = 0; i < rules.length; i++) {
        const r = rules[i]
        if ((r as CSSStyleRule).selectorText) RULES.push({ rule: r as CSSStyleRule, order: RULES.length })
        else if ((r as CSSGroupingRule).cssRules) walk((r as CSSGroupingRule).cssRules)
      }
    }
    walk(style.sheet!.cssRules)
  }
}
/** Specificity as one comparable number: (ids, classes+attrs+pseudo-classes, elements). */
function spec(sel: string): number {
  const s = sel.replace(/:where\([^)]*\)/g, '')
  const ids = (s.match(/#[\w-]+/g) ?? []).length
  const cls = (s.match(/\.[\w-]+|\[[^\]]+\]|:[a-z-]+(?:\([^)]*\))?/g) ?? []).filter((x) => !x.startsWith('::')).length
  const el = (s.match(/(?:^|[\s>+~(])([a-z][\w-]*)/g) ?? []).length
  return ids * 10000 + cls * 100 + el
}
/** The winning declaration of `prop` on a rendered element, by importance, specificity, then order.
 *  Selector lists are split so each selector is weighed on its own. */
function winner(el: Element, prop: string): string | null {
  let win: { value: string; important: boolean; spec: number; order: number } | null = null
  for (const { rule, order } of RULES) {
    const value = rule.style.getPropertyValue(prop)
    if (!value) continue
    for (const sel of rule.selectorText.split(',')) {
      let hit = false
      try { hit = el.matches(sel) } catch { continue }
      if (!hit) continue
      const important = rule.style.getPropertyPriority(prop) === 'important'
      const s = spec(sel)
      if (win === null || (important && !win.important) ||
        (important === win.important && (s > win.spec || (s === win.spec && order >= win.order)))) {
        win = { value: value.trim(), important, spec: s, order }
      }
    }
  }
  return win?.value ?? null
}

describe("the scope toolbar's start slot", () => {
  it('renders first in the toolbar strip, on its line, and a scope without it is unchanged', async () => {
    loadSheets()
    const ui = render(
      <PhoneScope cockpit="cw" transmitting={false} theme="dark" active
        toolbarStart={<div className="ph-span" role="group" aria-label="chips"><button type="button" className="theme-chip">Std</button></div>} />,
    )
    await draw()
    const strip = ui.container.querySelector('.ph-scope-smeter')!
    const chips = ui.getByRole('group', { name: 'chips' })
    expect(chips.parentElement).toBe(strip)
    expect(strip.firstElementChild).toBe(chips)
    // The S-meter is still there, after it.
    expect(chips.nextElementSibling?.className).toBe('ph-scope-smeter-label')
    // The strip is a wrapping ROW of flex items…
    expect(winner(strip, 'display')).toBe('flex')
    expect(winner(strip, 'flex-direction') ?? 'row').toBe('row')
    // …and the chip group is a content-width item in it, not a line of its own.
    expect(winner(chips, 'display')).toBe('flex')
    for (const p of ['width', 'flex-basis', 'flex', 'flex-grow']) {
      expect(winner(chips, p), `${p} on the chip group`).toBeNull()
    }
    cleanup()
    // Negative control: without the slot the strip opens on the S-meter, as before.
    const bare = render(<PhoneScope cockpit="cw" transmitting={false} theme="dark" active />)
    await draw()
    expect(bare.container.querySelector('.ph-scope-smeter')!.firstElementChild?.className).toBe('ph-scope-smeter-label')
  })
})
