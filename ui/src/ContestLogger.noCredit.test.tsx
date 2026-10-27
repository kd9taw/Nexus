// @vitest-environment jsdom
//
// THE LOGGER WINDOW'S "FT8 · NO CREDIT" CHIP (2026-10-12). Benching the Illinois QSO Party on FT8, the
// operator found the warning over the large log line "very verbose and yellow and distracting": a
// full-width yellow band above its top row, in this window and on the main window's Contest screen.
// The pick: "A small outlined chip beside CONTEST LOG / PH that reads 'FT8 · no credit'; hover it for
// the full sentence. No yellow band."
//
// In the real `?panel=contestlog` window, mocked at the bridge as ContestLogger.rstFollowsMode.test.tsx
// is, and by value:
//   · on FT8 or FT4 at a contest that does not credit it, the top row reads, right after the mode chip,
//     "FT8 · no credit" (or "FT4 · no credit"), with the whole sentence as its title and its accessible
//     description; there is no band, and the contact still logs, under the digital class;
//   · on a mode the contest credits there is none, and the chip follows the station as the band did:
//     phone, then FT8, then phone again, without the window mounting anew;
//   · its look, computed from the real sheets on the chain this window renders: an outline in the
//     de-emphasised state ink, no fill, and 4.5:1 or better on its face in every theme and mode. The
//     main window's Contest screen is held to the same in App.contestScreenEntry.test.tsx;
//   · the Contest banner, here and in the contest screen window, says it with the same chip where it
//     drew an amber line, and it looks the same there.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, cleanup, act, fireEvent, screen, within } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { DetachedPanel } from './DetachedPanel'
import { contestName } from './fdEvent'
import { t } from './i18n'
import {
  BASE_MODES,
  MODES,
  chainOf,
  contrast,
  expandWith,
  parseRules,
  rgbHex as hex,
  toRgb,
  tokensAt,
  winnerAt,
  type El,
  type Mode,
  type Rgb,
  type Rule,
} from './cssCascade'
import defaultSettings from './components/__fixtures__/defaultSettings.json'

// The window polls in real time, and the look is swept over every theme and mode: about 2 s alone,
// and the full suite's own parallelism took that past the default 5 s once.
vi.setConfig({ testTimeout: 60_000 })

const T0 = 1_792_342_800

/** The station's operating section and the FT tier it runs, as its snapshots say. */
let section = 'phone'
let tier = 'Ft8'
/** The contest's ruleset, as `get_fd_ruleset` serves it: the Illinois QSO Party credits no FT8 or FT4. */
const ILQP_RULES = {
  event: 'ilqp',
  rulesYear: 2026,
  bannedModes: ['FT8', 'FT4'],
  spottingAllowed: true,
  clusterAllowed: true,
  enforcement: 'warn',
  role: 'in_state',
  exchange: [],
  problem: '',
  eventStartUnix: T0 - 3600,
  eventEndUnix: T0 + 3600,
}
let rules: Record<string, unknown> = ILQP_RULES
/** Every contact the window asked the engine to log. */
let logged: { call: string; fields: [string, string][]; mode: string }[] = []
/** What the window's poll reads: the station as it stood at the last `poll()`. */
let published: unknown = null

/** An Illinois QSO Party session sending from McHenry, as the engine serialises it. */
const snapshot = () => ({
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  mode: 'FieldDay',
  radio: { dialMhz: 7.074, band: '40m', sideband: 'USB', operatingMode: section, txEnabled: false, txAllowed: true },
  link: { tier },
  stations: [],
  conversations: [],
  activePeer: null,
  hunt: null,
  fieldDay: {
    running: false,
    state: 'Idle',
    qsoCount: logged.length,
    sections: 0,
    points: 0,
    multCount: 0,
    event: 'ilqp',
    role: 'in_state',
    log: [],
    receives: [
      { key: 'RST', kind: 'rst', required: true },
      { key: 'QTH', kind: 'oneOf', required: true, domains: ['il_counties', 'il_mults'] },
    ],
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'QTH', raw: 'MCHE', domain: 'il_counties' },
    ],
    composingText: '599 MCHE',
    composingTextPhone: '59 MCHE',
    assistanceOn: [],
    upload: { enabled: false, destinations: [], available: [], hint: '' },
    eventStartUnix: T0 - 3600,
    eventEndUnix: T0 + 3600,
  },
  contestEntry: { rev: 1, call: '', fields: {}, marks: null },
})

function respond(cmd: string, args: Record<string, unknown> | undefined): unknown {
  if (cmd === 'get_snapshot') return published
  if (cmd === 'get_settings') return { ...defaultSettings, fdActive: true, fdEvent: 'ilqp' }
  if (cmd === 'get_fd_ruleset') return rules
  if (cmd === 'contest_entry_put') return 2
  if (cmd === 'contest_log_manual') {
    logged.push({ call: args!.call as string, fields: args!.fields as [string, string][], mode: args!.mode as string })
    return snapshot()
  }
  if (cmd === 'contest_removed' || cmd === 'contest_unmerged') return []
  if (/^(get_band_plan|get_need_alerts|get_scp_calls)$/.test(cmd)) return []
  if (/^(get_propagation|resolve_entity|contest_place|get_call_history)$/.test(cmd)) return null
  return {}
}

beforeEach(() => {
  localStorage.clear()
  section = 'phone'
  tier = 'Ft8'
  rules = ILQP_RULES
  logged = []
  published = snapshot()
  window.__TAURI_INTERNALS__ = {
    invoke: async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => respond(cmd, args) as T,
  }
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(() => {
  cleanup()
  delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
})

/** The station as it stands now reaches the window at its next poll (every 300 ms). */
const poll = () => {
  published = snapshot()
  return act(async () => new Promise((r) => setTimeout(r, 400)))
}
const settle = () => act(async () => new Promise((r) => setTimeout(r, 0)))
const entry = () => screen.getByRole('region', { name: t('contestLogger.entry.aria') })
/** What the entry's top row shows right after its mode chip. */
const afterMode = () => entry().querySelector('.le-fd-header > .le-fd-mode')!.nextElementSibling!
const banned = (mode: string) => t('fieldDay.advisory.banned', { mode, event: contestName('ilqp'), year: 2026 })
const HINT = t('logEntry.contest.hint', { band: '40m' })

describe('the logger window: a mode the contest does not credit', () => {
  it('on FT8 the top row says "FT8 · no credit" right after the mode, the sentence on hover; no band; the contact still logs', async () => {
    section = 'digital'
    render(<DetachedPanel panel="contestlog" />)
    await poll()
    expect(entry().querySelector('.le-fd-mode')!.textContent).toBe('DIG')
    const chip = afterMode()
    expect(chip.textContent).toBe('FT8 · no credit')
    expect(chip.getAttribute('title')).toBe(banned('FT8'))
    expect(within(entry()).getByRole('status', { name: 'FT8 · no credit', description: banned('FT8') })).toBe(chip)
    // No band: the sentence is not drawn anywhere in the entry.
    expect(entry().querySelector('.fd-advisory')).toBeNull()
    expect(entry().textContent).not.toContain(banned('FT8'))
    // Logging stays allowed, under the digital class.
    fireEvent.change(within(entry()).getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' }), { target: { value: 'K9ABC' } })
    fireEvent.change(within(entry()).getByLabelText('County / State'), { target: { value: 'COOK' } })
    fireEvent.keyDown(within(entry()).getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' }), { key: 'Enter' })
    await settle()
    await settle()
    expect(logged).toEqual([{ call: 'K9ABC', fields: [['RST', '599'], ['QTH', 'COOK']], mode: 'DIG' }])
  })

  it('on FT4 the chip names FT4, the mode the station is on', async () => {
    section = 'digital'
    tier = 'Ft4'
    render(<DetachedPanel panel="contestlog" />)
    await poll()
    expect(afterMode().textContent).toBe('FT4 · no credit')
    expect(afterMode().getAttribute('title')).toBe(banned('FT4'))
  })

  it('follows the station as the band did: phone, then FT8, then phone again', async () => {
    render(<DetachedPanel panel="contestlog" />)
    await poll()
    expect(afterMode().textContent, 'phone').toBe(HINT)
    section = 'digital'
    await poll()
    expect(afterMode().textContent, 'moved to FT8').toBe('FT8 · no credit')
    section = 'phone'
    await poll()
    expect(afterMode().textContent, 'back on phone').toBe(HINT)
    expect(entry().querySelector('.le-fd-header')!.textContent).not.toContain('no credit')
  })

  it('on FT8 at a contest that credits it there is no chip', async () => {
    section = 'digital'
    rules = { ...ILQP_RULES, bannedModes: [] }
    render(<DetachedPanel panel="contestlog" />)
    await poll()
    expect(entry().querySelector('.le-fd-mode')!.textContent).toBe('DIG')
    expect(afterMode().textContent).toBe(HINT)
    expect(entry().querySelector('.le-fd-header')!.textContent).not.toContain('no credit')
  })
})

// ⭐ THE CONTEST BANNER SAYS IT THE SAME WAY (2026-10-12): it drew the same sentence as an amber line,
// and the operator picked "Same small chip with the full sentence on hover, in the Contest banner and
// the FT screen header". The banner is in this window's copy of the Contest screen, and in the
// contest screen window that the main window pops out.

/** What a window's Contest banner shows right after the contest's window: where its advisories stand. */
const afterWindow = () => document.querySelector('.fd-event-banner > .fd-event-subtitle')!.nextElementSibling!

describe('the Contest banner in the logger window and the contest screen window', () => {
  it.each(['contestlog', 'contestscreen'])('?panel=%s on FT8: "FT8 · no credit", the sentence on hover; the amber line is gone', async (panel) => {
    section = 'digital'
    render(<DetachedPanel panel={panel} />)
    await poll()
    expect(afterWindow().textContent).toBe('FT8 · no credit')
    expect(afterWindow().getAttribute('title')).toBe(banned('FT8'))
    const banner = document.querySelector<HTMLElement>('.fd-event-banner')!
    expect(within(banner).getByRole('status', { name: 'FT8 · no credit', description: banned('FT8') })).toBe(afterWindow())
    expect(banner.querySelector('.fd-advisory')).toBeNull()
    expect(banner.textContent).not.toContain(banned('FT8'))
  })

  it.each(['contestlog', 'contestscreen'])('?panel=%s follows the station: phone, then FT4, then phone again', async (panel) => {
    render(<DetachedPanel panel={panel} />)
    await poll()
    const said = () => document.querySelector('.fd-event-banner')!.querySelector('.le-fd-nocredit')?.textContent ?? null
    expect(document.querySelector('.fd-event-banner'), 'control: the banner is on show').not.toBeNull()
    expect(said(), 'phone').toBeNull()
    section = 'digital'
    tier = 'Ft4'
    await poll()
    expect(said(), 'moved to FT4').toBe('FT4 · no credit')
    section = 'phone'
    await poll()
    expect(said(), 'back on phone').toBeNull()
    expect(document.querySelector('.fd-advisory')).toBeNull()
  })
})

// ── the chip's look ─────────────────────────────────────────────────────────────────────────

const read = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const LOOK: Rule[] = parseRules(read('styles.css') + '\n' + read('cockpit-panes.css'))
/** FIRES: the chip lettered in the amber the band was, planted here and never shipped. */
const AMBER: Rule[] = [...LOOK, ...parseRules('.le-fd-nocredit { color: var(--state-warn); }', { n: LOOK.length + 1 })]

const memo = new Map<Rule[], Map<string, unknown>>()
function once<T>(rules: Rule[], key: string, make: () => T): T {
  let byKey = memo.get(rules)
  if (!byKey) memo.set(rules, (byKey = new Map()))
  if (!byKey.has(key)) byKey.set(key, make())
  return byKey.get(key) as T
}
const chainKey = (chain: El[]) =>
  JSON.stringify(chain.map((e) => [e.tag, e.classes, Object.entries(e.attrs).filter(([k]) => !/^(id|title|aria-\w+)$/.test(k))]))
/** A colour preset declares custom properties only, so the mode without its presets decides a winner. */
const winnerMode = (mode: Mode): Mode => mode.split(' ').filter((p, i) => i === 0 || p.startsWith('skin=')).join(' ') as Mode
/** The winning declaration of `props` on the chain's last element, or on the nearest ancestor that has one. */
function nearest(rules: Rule[], mode: Mode, chain: El[], ...props: string[]): { value: string; at: El[] } | null {
  for (let i = chain.length; i > 0; i--) {
    const at = chain.slice(0, i)
    const w = once(rules, `w|${winnerMode(mode)}|${chainKey(at)}|${props.join()}`, () => winnerAt(rules, winnerMode(mode), at, ...props))
    if (w && !/^(inherit|transparent|none)$/.test(w.value)) return { value: w.value, at }
  }
  return null
}
const colourOf = (rules: Rule[], mode: Mode, at: El[], value: string, under: Rgb): Rgb => {
  const c = toRgb(expandWith(once(rules, `t|${mode}|${chainKey(at)}`, () => tokensAt(rules, mode, at)), value), under)
  if (!c) throw new Error(`not a colour: "${value}"`)
  return c
}
/** The face the chain's last element sits on: the nearest painted face, laid over the faces beneath
 *  it, down to the page. */
function faceOf(rules: Rule[], mode: Mode, chain: El[]): Rgb {
  const face = nearest(rules, mode, chain, 'background', 'background-color')
  const page = () => colourOf(rules, mode, chain.slice(0, 1), 'var(--bg)', [0, 0, 0])
  if (!face) return page()
  return colourOf(rules, mode, face.at, face.value, face.at.length > 1 ? faceOf(rules, mode, face.at.slice(0, -1)) : page())
}
/** Every theme and mode in which the chain's last element letters under 4.5:1 on its face. */
function unreadable(rules: Rule[], chain: El[]): string[] {
  const out: string[] = []
  for (const mode of MODES) {
    const ink = nearest(rules, mode, chain, 'color')
    if (!ink) {
      out.push(`${mode}: no ink from the sheet`)
      continue
    }
    const bg = faceOf(rules, mode, chain)
    const fg = colourOf(rules, mode, ink.at, ink.value, bg)
    if (contrast(fg, bg) < 4.5) out.push(`${mode}: ${hex(fg)} on ${hex(bg)} = ${contrast(fg, bg).toFixed(2)}:1`)
  }
  return out
}

describe('the logger window: the no-credit chip’s look, computed from the real sheets', () => {
  /** The chip on FT8, read off this window. */
  async function chip(): Promise<Element> {
    section = 'digital'
    render(<DetachedPanel panel="contestlog" />)
    await poll()
    const c = afterMode()
    expect(c.textContent, 'control: the chip is on show').toBe('FT8 · no credit')
    return c
  }

  it('an outline in the de-emphasised state ink, no fill, in the mode chip’s own face', async () => {
    const c = await chip()
    const chain = chainOf(c)
    const mode = chainOf(entry().querySelector('.le-fd-header > .le-fd-mode')!)
    for (const m of BASE_MODES) {
      expect(winnerAt(LOOK, m, chain, 'color')?.value, m).toBe('var(--state-pending)')
      expect(winnerAt(LOOK, m, chain, 'border', 'border-color')?.value, m).toBe('1px solid currentColor')
      expect(winnerAt(LOOK, m, chain, 'background', 'background-color'), m).toBeNull()
      for (const prop of ['font-family', 'font-size', 'font-weight']) {
        expect(winnerAt(LOOK, m, chain, prop)?.value, `${m} ${prop}`).toBe(winnerAt(LOOK, m, mode, prop)?.value)
      }
    }
  })

  it('reads 4.5:1 or better on its face in every theme and mode, where the band’s amber would not', async () => {
    const chain = chainOf(await chip())
    expect(unreadable(LOOK, chain)).toEqual([])
    const fires = unreadable(AMBER, chain)
    expect(fires.find((f) => f.startsWith('light: ')), 'FIRES: the amber on the light panel').toBeDefined()
  })

  it('in this window’s Contest banner, the same outline and ink, and 4.5:1 or better on the banner’s own face', async () => {
    section = 'digital'
    render(<DetachedPanel panel="contestlog" />)
    await poll()
    const c = afterWindow()
    expect(c.textContent, 'control: the chip is on show').toBe('FT8 · no credit')
    const chain = chainOf(c)
    for (const m of BASE_MODES) {
      expect(winnerAt(LOOK, m, chain, 'color')?.value, m).toBe('var(--state-pending)')
      expect(winnerAt(LOOK, m, chain, 'border', 'border-color')?.value, m).toBe('1px solid currentColor')
      expect(winnerAt(LOOK, m, chain, 'background', 'background-color'), m).toBeNull()
      expect(winnerAt(LOOK, m, chain, 'font-size')?.value, m).toBe('calc(11px * var(--text-scale))')
    }
    expect(unreadable(LOOK, chain)).toEqual([])
    const fires = unreadable(AMBER, chain)
    expect(fires.find((f) => f.startsWith('light: ')), 'FIRES: the amber on the banner’s light face').toBeDefined()
  })
})
