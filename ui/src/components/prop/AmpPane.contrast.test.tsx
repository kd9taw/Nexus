// @vitest-environment jsdom
//
// THE AMPLIFIER BOX'S OPERATE/STANDBY BUTTON READS IN EVERY THEME, and its colours are the operator's:
// RED for Operate (the TX red, `--tx`), GREEN for Standby (the good-state green, `--state-good`).
//
// The house rule for a coloured state button is the header strip's (OperateButton.contrast.test.tsx):
// the word in the theme's text colour, the state's colour on the border and as a tint of the face. A
// status colour is held to 3:1, which a border needs and lettering does not meet; lettering is held to
// 4.5:1 on its own face. This sweeps every base mode (the standard ones and the built-in themes' worst
// cases) under every OK preset — Standby's green is the OK role's — in the two places the box stands
// (Conditions and the dashboard rail), on every surface a box can sit on, locked or not: the button locks
// while keyed, which is exactly when the amplifier's state matters most, and shows it with a dashed edge
// rather than a dimming (a dimmed green face read under 4.5:1 at Night).
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { AmpPane } from './AmpPane'
import { PALETTE_ROLES } from '../../features/paletteRoles'
import {
  BASE_MODES,
  SENTINEL_MODES,
  baseOf,
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
} from '../../cssCascade'
import type { AmpStatus } from '../../types'

// Twenty base modes under three OK presets, two hosts, five states: the house 15 s budget, and more for
// the sweep itself below.
vi.setConfig({ testTimeout: 15_000 })

vi.mock('../../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../api')>()),
  ampCommand: vi.fn(async () => true),
}))

afterEach(cleanup)

const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
/** A button's `:disabled` IS its `disabled` attribute, which the rendered chain carries. */
const withDisabled = (rules: Rule[]) =>
  rules.map((r) => (r.selector.includes(':disabled') ? { ...r, selector: r.selector.replace(/:disabled/g, '[disabled]') } : r))
const RULES = withDisabled(parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css')))

const TEXT_MIN = 4.5
const STATE_MIN = 3
const BACKDROPS = ['--bg', '--panel', '--bg-elev'] as const
const OK = PALETTE_ROLES.find((r) => r.id === 'ok')!
const MODES: Mode[] = [...BASE_MODES, ...SENTINEL_MODES].flatMap((b) => OK.presets.map((p, i) => (i === 0 ? b : withRoles(b, { ok: p.id }))))

const live = (over: Partial<AmpStatus> = {}): AmpStatus => ({
  family: 'spe', model: '15K', linked: true, reason: '', operate: true, transmitting: false, outputWatts: 0, swr: 1.1,
  swrAtu: 1.0, volts: 48, amps: 0, temp: 30, tempCelsius: false, alarm: 'none', alarmRaised: false, warning: 'none',
  warningRaised: false, kpaFault: null, ...over,
})

interface Button {
  label: string
  chain: El[]
  state: 'operate' | 'standby' | 'unknown'
}

/** The box's button in every state, in each host the box stands in. */
function buttons(): Button[] {
  const out: Button[] = []
  const hosts: Array<[string, string]> = [
    ['Conditions', 'connect'],
    ['dashboard rail', 'dash-boxes'],
  ]
  const states: Array<[AmpStatus, boolean]> = [
    [live({ operate: true }), false],
    [live({ operate: false }), false],
    [live({ operate: true }), true],
    [live({ operate: false }), true],
    [live({ linked: false, reason: 'noAnswer' }), false],
  ]
  for (const [host, cls] of hosts) {
    for (const [amp, onAir] of states) {
      const r = render(
        <div className={cls}>
          <div className="pane-body">
            <AmpPane amp={amp} radioOnAir={onAir} />
          </div>
        </div>,
      )
      const b = r.container.querySelector<HTMLButtonElement>('.amp-opbtn')!
      const state = b.classList.contains('is-operate') ? 'operate' : b.classList.contains('is-standby') ? 'standby' : 'unknown'
      out.push({ label: `${host} ${state}${b.disabled ? ' (disabled)' : ''}`, chain: chainOf(b), state })
      cleanup()
    }
  }
  return out
}

const tokensFor = (mode: Mode, chain: El[]) => tokensAt(RULES, mode, chain)
const win = (mode: Mode, chain: El[], ...props: string[]) => winnerAt(RULES, baseOf(mode), chain, ...props)
const colour = (tokens: Map<string, string>, value: string, backdrop: Rgb): Rgb => {
  const c = toRgb(expandWith(tokens, value), backdrop)
  if (!c) throw new Error(`not a colour: "${value}" → "${expandWith(tokens, value)}"`)
  return c
}
const through = (c: Rgb, under: Rgb, opacity: number): Rgb =>
  [0, 1, 2].map((i) => Math.round(c[i] * opacity + under[i] * (1 - opacity))) as unknown as Rgb
const FACE = ['background', 'background-color'] as const
const BORDER = ['border', 'border-color', 'border-top-color'] as const

describe('the Amplifier box’s Operate/Standby button reads, and is red or green, in every theme', () => {
  let all: Button[] = []
  beforeAll(() => {
    all = buttons()
  })

  it('renders in every state in both hosts (the sweep cannot silently empty out)', () => {
    expect(all.map((b) => b.label)).toEqual(
      expect.arrayContaining([
        'Conditions operate', 'Conditions standby', 'Conditions operate (disabled)', 'Conditions standby (disabled)',
        'Conditions unknown (disabled)', 'dashboard rail operate', 'dashboard rail standby',
      ]),
    )
  })

  it('Operate is bordered and tinted in the TX red, Standby in the good-state green — never the other way', () => {
    for (const b of all) {
      for (const mode of BASE_MODES) {
        const border = win(mode, b.chain, ...BORDER)?.value ?? ''
        const face = win(mode, b.chain, ...FACE)?.value ?? ''
        if (b.state === 'operate') {
          expect(border, `${b.label} ${mode}`).toContain('var(--tx)')
          expect(face, `${b.label} ${mode}`).toContain('var(--tx)')
        } else if (b.state === 'standby') {
          expect(border, `${b.label} ${mode}`).toContain('var(--state-good)')
          expect(face, `${b.label} ${mode}`).toContain('var(--state-good)')
        } else {
          // A state we cannot see takes neither colour.
          expect(`${border} ${face}`, `${b.label} ${mode}`).not.toMatch(/--tx|--state-good/)
        }
      }
    }
  })

  it('its own face, ink and border — none left to the browser', () => {
    const left: string[] = []
    for (const b of all) {
      for (const mode of MODES) {
        for (const [what, props] of [['face', FACE], ['ink', ['color']], ['border', BORDER]] as const) {
          if (!win(mode, b.chain, ...props)) left.push(`${b.label}: the browser draws its ${what}`)
        }
      }
    }
    expect([...new Set(left)]).toEqual([])
  })

  it('the word letters at 4.5:1 on its own face, locked or not, in every mode, under every OK preset', () => {
    const out: string[] = []
    for (const b of all) {
      for (const mode of MODES) {
        const tokens = tokensFor(mode, b.chain)
        const face = win(mode, b.chain, ...FACE)!
        const ink = win(mode, b.chain, 'color')!
        const opacity = Number(win(mode, b.chain, 'opacity')?.value ?? 1)
        for (const s of BACKDROPS) {
          const under = colour(tokens, `var(${s})`, [0, 0, 0])
          const faceRgb = colour(tokens, face.value, under)
          const bg = through(faceRgb, under, opacity)
          const fg = through(colour(tokens, ink.value, faceRgb), under, opacity)
          const r = contrast(fg, bg)
          if (r < TEXT_MIN) out.push(`${b.label} ${mode} on ${s}: ${hex(fg)} on ${hex(bg)} = ${r.toFixed(2)}:1`)
        }
      }
    }
    expect(out).toEqual([])
  }, 60_000)

  it('the state colour on the border holds 3:1 from every surface the box sits on', () => {
    const out: string[] = []
    for (const b of all.filter((x) => x.state !== 'unknown')) {
      const token = b.state === 'operate' ? 'var(--tx)' : 'var(--state-good)'
      for (const mode of MODES) {
        const tokens = tokensFor(mode, b.chain)
        const c = colour(tokens, token, [0, 0, 0])
        for (const s of BACKDROPS) {
          const r = contrast(c, colour(tokens, `var(${s})`, [0, 0, 0]))
          if (r < STATE_MIN) out.push(`${b.label} ${mode}: ${token} on ${s} = ${r.toFixed(2)}:1`)
        }
      }
    }
    expect(out).toEqual([])
  }, 60_000)

  it('a locked button looks locked — a dashed edge — and an open one does not', () => {
    for (const b of all) {
      const locked = b.label.endsWith('(disabled)')
      for (const mode of BASE_MODES) {
        expect(win(mode, b.chain, 'border-style')?.value === 'dashed', `${b.label} ${mode}`).toBe(locked)
      }
    }
  })
})
