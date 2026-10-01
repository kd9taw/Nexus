// @vitest-environment jsdom
//
// THE SPECTRUM'S SOURCE BADGE READS IN EVERY LIGHT THEME, IN BOTH ITS HOSTS (operator, 2026-09-30: "Ink, accent as the
// mark", then the same for Settings ▸ Audio).
//
// MiniSpectrum letters its source (AUDIO, FLEX RF, CI-V RF) in the accent, which was tuned as a mark: as lettering on the
// light page it read 4.25:1, under the 4.5:1 floor (Chrome, Connect's scope pane). In the light themes the badge takes the
// theme's ink, with the accent kept as its underline, in both of the component's hosts: Connect's scope pane and the audio
// spectrum in Settings ▸ Audio. Dark is untouched.
//
// This renders both hosts in their chains and resolves, with the app's own resolver (cssCascade.ts), what paints the badge
// and what it sits on: in every light theme (the four light modes, bare and on each light theme, and the standard light
// theme under every accent preset) the word must clear 4.5:1 and its underline, the accent, 3:1; in every dark theme it
// still letters in the accent, with no underline.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { act, cleanup, render } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import type { ReactNode } from 'react'
import { MiniSpectrum } from './MiniSpectrum'
import { PaneFrame } from './connect/PaneFrame'
import type { PaneContext } from './connect/paneContext'
import { PALETTE_ROLES } from '../features/paletteRoles'
import { SKINS } from '../features/skins'
import {
  BASE_MODES,
  SENTINEL_MODES,
  baseTheme,
  chainOf,
  contrast,
  expandWith,
  parseRules,
  rgbHex as hex,
  toRgb,
  tokensAt,
  winnerAt,
  withRoles,
  type El,
  type Mode,
  type Rgb,
  type Rule,
} from '../cssCascade'

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  getSpectrumRow: vi.fn(async () => null),
}))

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

const noop = () => {}
/** The two hosts, each in its own chain (App wraps Settings in `main.layout.single`; the pane is Connect's). */
const HOSTS: Array<[string, () => ReactNode]> = [
  ['Connect scope pane', () => (
    <div className="app">
      <main className="layout single">
        <div className="connect-shell">
          <div className="connect" data-rails="both">
            <div className="connect-rail" data-side="left">
              <PaneFrame slotId="left1" paneId="scope" ctx={{ theme: 'dark', intent: 'dx' } as unknown as PaneContext} onAssign={noop} onHide={noop} />
            </div>
          </div>
        </div>
      </main>
    </div>
  )],
  ['Settings ▸ Audio', () => (
    <div className="app">
      <main className="layout single">
        <section className="panel settings-panel">
          <form className="settings-form">
            <div className="settings-scroll">
              <fieldset className="settings-section" id="settings-audio">
                <div className="settings-grid">
                  <div className="settings-field settings-audio-scope">
                    <MiniSpectrum height={84} idleHint="idle" />
                  </div>
                </div>
              </fieldset>
            </div>
          </form>
        </section>
      </main>
    </div>
  )],
]

interface Badge {
  host: string
  chain: El[]
}
const BODY: El = { tag: 'body', classes: [], attrs: {} }
async function renderBadges(): Promise<Badge[]> {
  const out: Badge[] = []
  for (const [host, make] of HOSTS) {
    let r!: ReturnType<typeof render>
    await act(async () => {
      r = render(<>{make()}</>)
    })
    for (const el of r.container.querySelectorAll('.mini-spectrum-src')) out.push({ host, chain: [BODY, ...chainOf(el)] })
    cleanup()
  }
  return out
}

const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES = parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css'))
/** The badge as it shipped: the light-theme rule on it removed. */
const SHIPPED = RULES.filter((r) => !(r.selector.startsWith("[data-theme='light'] ") && /\.mini-spectrum-src\b/.test(r.selector)))

const skinsOf = (theme: 'light' | 'dark') => ['', ...SKINS.filter((s) => s.base === theme).map((s) => s.id)]
const accent = PALETTE_ROLES.find((r) => r.id === 'accent')!
/** Every built-in light theme, and the standard light theme under each accent preset (the accent is the badge's colour). */
const LIGHT: Mode[] = [
  ...BASE_MODES.filter((b) => baseTheme(b) === 'light').flatMap((b) => skinsOf('light').map((skin) => (skin ? withRoles(b, { skin }) : b))),
  ...accent.presets.slice(1).map((p) => withRoles('light', { accent: p.id })),
]
const DARK: Mode[] = [...BASE_MODES.filter((b) => baseTheme(b) === 'dark'), ...SENTINEL_MODES.filter((m) => m.startsWith('dark skin='))]

const BLANK = /^(inherit|transparent|none|initial|unset)$/i
const colourOf = (rules: Rule[], mode: Mode, at: El[], value: string, under: Rgb): Rgb => {
  const c = toRgb(expandWith(tokensAt(rules, mode, at), value), under)
  if (!c) throw new Error(`not a colour: "${value}"`)
  return c
}
/** What the badge sits on: every background from it outward, down to the first opaque one, composited. */
function surfaceOf(rules: Rule[], mode: Mode, chain: El[]): Rgb {
  const layers: Array<{ value: string; at: El[] }> = []
  for (let i = chain.length; i > 0; i--) {
    const at = chain.slice(0, i)
    const w = winnerAt(rules, mode, at, 'background', 'background-color')
    if (!w || BLANK.test(w.value.trim())) continue
    layers.push({ value: w.value, at })
    if (hex(colourOf(rules, mode, at, w.value, [0, 0, 0])) === hex(colourOf(rules, mode, at, w.value, [255, 255, 255]))) break
  }
  return layers.reduceRight((under, l) => colourOf(rules, mode, l.at, l.value, under), colourOf(rules, mode, chain.slice(0, 1), 'var(--bg)', [0, 0, 0]))
}
/** The declaration that paints the word: the badge's own, else the nearest ancestor's (it inherits). */
function inkOf(rules: Rule[], mode: Mode, chain: El[]) {
  for (let i = chain.length; i > 0; i--) {
    const w = winnerAt(rules, mode, chain.slice(0, i), 'color')
    if (w && !BLANK.test(w.value.trim())) return { value: w.value, at: chain.slice(0, i) }
  }
  throw new Error('nothing paints the badge')
}
function wordOf(rules: Rule[], mode: Mode, b: Badge) {
  const bg = surfaceOf(rules, mode, b.chain)
  const ink = inkOf(rules, mode, b.chain)
  const fg = colourOf(rules, mode, ink.at, ink.value, bg)
  return { fg, bg, ratio: contrast(fg, bg), ink }
}
const accentAt = (rules: Rule[], mode: Mode, at: El[]) => hex(colourOf(rules, mode, at, 'var(--accent)', [0, 0, 0]))

describe("the spectrum's source badge reads in every light theme, in both its hosts", () => {
  let badges: Badge[] = []
  beforeAll(async () => {
    badges = await renderBadges()
  }, 60_000)

  it('renders the badge in both hosts (the census cannot silently empty out)', () => {
    expect(badges.map((b) => b.host)).toEqual(HOSTS.map(([h]) => h))
  })

  it('in every light theme and under every accent the badge reads 4.5:1 in the ink, underlined in the accent at 3:1', () => {
    const low: string[] = []
    for (const b of badges)
      for (const mode of LIGHT) {
        const { fg, bg, ratio } = wordOf(RULES, mode, b)
        if (ratio < 4.5) low.push(`${b.host} ${mode}: ${hex(fg)} on ${hex(bg)} = ${ratio.toFixed(2)}:1`)
        const line = winnerAt(RULES, mode, b.chain, 'text-decoration-line')?.value
        const mark = line === 'underline' ? winnerAt(RULES, mode, b.chain, 'text-decoration-color')?.value : null
        if (mark !== 'var(--accent)') {
          low.push(`${b.host} ${mode}: the accent is not its underline (${line} ${mark})`)
          continue
        }
        const c = colourOf(RULES, mode, b.chain, mark, bg)
        if (contrast(c, bg) < 3) low.push(`${b.host} ${mode}: the underline ${hex(c)} on ${hex(bg)} = ${contrast(c, bg).toFixed(2)}:1`)
      }
    expect(low).toEqual([])
  }, 60_000)

  it('in every dark theme the badge still letters in the accent, with no underline', () => {
    const moved: string[] = []
    for (const b of badges)
      for (const mode of DARK) {
        const { fg, ink } = wordOf(RULES, mode, b)
        if (hex(fg) !== accentAt(RULES, mode, ink.at)) moved.push(`${b.host} ${mode}: lettered in ${hex(fg)}, not the accent`)
        const line = winnerAt(RULES, mode, b.chain, 'text-decoration-line', 'text-decoration')?.value
        if (line && line !== 'none') moved.push(`${b.host} ${mode}: underlined in dark`)
      }
    expect(moved).toEqual([])
  }, 60_000)

  it('FIRES: the badge as it shipped letters in the accent in the light theme, at the ratio Chrome measured on Connect', () => {
    const shipped = badges.map((b) => {
      const { fg, bg, ratio } = wordOf(SHIPPED, 'light', b)
      return `${b.host}: ${hex(fg)} on ${hex(bg)} = ${ratio.toFixed(2)}:1`
    })
    expect(shipped[0]).toBe('Connect scope pane: #0174ab on #e5eaf0 = 4.25:1')
    // In Settings it sits on the panel, where the accent read 5.00:1 in Chrome: under the rule for its likeness, not its ratio.
    expect(shipped[1]).toBe('Settings ▸ Audio: #0174ab on #fbfcfe = 5.00:1')
  }, 60_000)
})
