// @vitest-environment jsdom
//
// THE CONTROLS THE NATIVE-CONTROL CENSUS FOUND PAINTED BY THE BROWSER NOW PAINT THEMSELVES
// (2026-09-27, "Put it in test3").
//
// The census ran real Chrome over every view and Settings tab, in both themes, with the computer
// set to light and to dark. It listed every control whose face or lettering came from the browser
// and so followed the COMPUTER's scheme rather than Nexus's theme. The root colour-scheme rule
// (styles-color-scheme.test.ts) ends the dependence on the computer. What this file holds is that
// the controls it named do not depend on the browser at all, so they read in both themes however
// that rule is later changed.
//
// THE MIXED CONTROLS: the theme's ink on the browser's face, the amp strip's pattern (G7). The
// setup-health chips in Settings ▸ Radio (`--text` on a grey face: 3.09:1 and 1.02:1 across the
// two mismatches), Operate's roster Spot (`--text-dim`: 2.21:1 in the dark theme even on a dark
// computer) and the SSTV overlay remove (`--state-weak`: 1.57:1 in the dark theme on a dark
// computer). Each now paints its own face and border, the box the browser's was, and keeps the
// ink it had.
//
// THE MACRO KEYS. `.cw-macro` gave the key a face and a border and no ink. The keys that put their
// label in a `.cw-macro-label` child were fine, because that child pins `--text`. The ones with
// bare text inherited the browser's button ink: black or white by the computer. That was
// SSTV's manual-receive Start (1.03:1 in the light theme on a dark computer, 1.19:1 the other way),
// the SSTV picture viewer's Previous/Next/Save, JS8's station queries, inbox Read/Delete, Cancel and
// Drop, and two Remote Refresh/retry keys. The key now carries `--text` itself.
//
// The cascade guards read only the author sheet, never the browser's defaults, so the first check
// is that the author sheet declares the thing. The second is that the word reads 4.5:1 on its
// own face in every mode. The chains carry the real parent where the components name one; the
// `.cw-macro` rule is the same wherever the key sits, and the one ancestor-scoped rule in its
// family (`.rtty-macros .cw-macro:disabled`) sets only opacity.
//
// THE BUILT-IN THEMES (features/skins.ts): the sweeps also walk the themes MODES carries as the
// worst case of each base (SENTINEL_MODES). The overlay remove's red read 4.2–4.4:1 on the Slate
// and Lagoon themes' raised surfaces, showing through its transparent face, so its face is the
// panel's colour now.
//
// THE FULLY NATIVE THREE (2026-09-28, operator: "Restyle POTA's Start/Download and Satellites' ⧉
// in Nexus's own button look"). The census's readable-but-native list: POTA's Start and Download
// and the Satellites header's ⧉ had no rule at all, so after the root rule they followed the theme
// in the browser's own look, a grey face in a raised border beside Nexus's outlined buttons. Each
// now wears the class of the Nexus button beside it: Start and Download the park list's Import
// buttons', the ⧉ the header's refresh chip's (a pop-out's own class parks it at the far right and
// grows the header at the 1024 floor). Start and Download keep `.pota-act-start`, the handle the
// Remote views' guards look for. Start is disabled until a park is typed, the state the operator
// meets first, so it has to look disabled once the browser no longer greys it. The sweep reads
// every button those two views render, so a new one left to the browser there is caught too.
//
// THE WATCH LIST × (Settings ▸ Spots & Alerts), found by the same night's census: the overlay
// remove's pattern exactly, `--state-weak` on the browser's face, 1.57:1 in the dark theme. It
// exists only for an entry in the list, and the census's fixture list was empty, so this guard
// seeds two.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup, screen, waitFor } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { SetupHealth } from './SetupHealth'
import { OperateRoster } from './OperateRoster'
import { PotaSotaView } from './PotaSotaView'
import { SatellitesView } from './SatellitesView'
import { WatchlistPanel } from './WatchlistPanel'
import { StationControlContext } from '../stationAccess'
import { t } from '../i18n'
import type { AppSnapshot, Station } from '../types'
import {
  BASE_MODES,
  SENTINEL_MODES,
  chainOf,
  compoundMatches,
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
} from '../cssCascade'

// THE BUDGET (2026-10-09). The slowest case here, "renders Start, Download and the ⧉, and Start disabled…", takes
// 0.30 s and 0.23 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

/** What the POTA/SOTA and Satellites views ask the backend for as they mount. */
const views = vi.hoisted(() => ({
  getActivation: vi.fn(async (): Promise<unknown> => null),
  getOtaSpots: vi.fn(async () => []),
  parksCount: vi.fn(async () => 0),
  huntedParksCount: vi.fn(async () => 0),
  getSatellites: vi.fn(async () => null),
  getSatPassNeeds: vi.fn(async () => []),
  getSatTrackStatus: vi.fn(async () => null),
  getSatTransponder: vi.fn(async () => null),
  getSettings: vi.fn(async () => ({ mygrid: 'EN52' })),
}))
vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  getDeclination: vi.fn(async () => 0),
  ...views,
}))
vi.mock('./MapView', () => ({ MapView: () => null }))
afterEach(cleanup)

const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
/** The resolver treats a state pseudo-class as a state the element is not in. A button's
 *  `:disabled` IS its `disabled` attribute, so it is read as one. */
const withDisabled = (rules: Rule[]) =>
  rules.map((r) => (r.selector.includes(':disabled') ? { ...r, selector: r.selector.replace(/:disabled/g, '[disabled]') } : r))
const RULES = withDisabled(parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css')))

const TEXT_MIN = 4.5
/** The standard modes, and the built-in themes MODES carries as the worst case of each base. */
const SWEPT: readonly Mode[] = [...BASE_MODES, ...SENTINEL_MODES]
/** What these controls' hosts sit on. */
const BACKDROPS = ['--bg', '--panel', '--bg-elev'] as const

/** One control as the cascade sees it. */
interface Control {
  name: string
  chain: El[]
  disabled?: boolean
}
const el = (tag: string, classes: string[], attrs: Record<string, string> = {}): El => ({ tag, classes, attrs })

const memo = new WeakMap<Rule[], Map<string, unknown>>()
function once<T>(rules: Rule[], key: string, make: () => T): T {
  let byKey = memo.get(rules)
  if (!byKey) memo.set(rules, (byKey = new Map()))
  if (!byKey.has(key)) byKey.set(key, make())
  return byKey.get(key) as T
}
const keyOf = (chain: El[]) => JSON.stringify(chain.map((e) => [e.tag, e.classes, e.attrs]))
/** The compound a rule puts on the element itself, split off its selector the way reachesChain splits it. */
const SUBJECT = new WeakMap<Rule, string | undefined>()
function subjectOf(rule: Rule): string | undefined {
  if (!SUBJECT.has(rule)) SUBJECT.set(rule, rule.selector.replace(/\s*>\s*/g, ' > ').split(/\s+/).filter(Boolean).pop())
  return SUBJECT.get(rule)
}
/** The rules that can win `props` on `el`: those that declare one of them (winnerAt passes over the rest) and whose subject
 *  matches `el` (reachesChain gives up on any other before it reads an ancestor or the mode). So the cut is made once per
 *  element, not once per mode, and winnerAt over it names the same winner as over every rule. */
const cutFor = (rules: Rule[], el: El, props: string[]) =>
  once(rules, `c|${keyOf([el])}|${props.join()}`, () =>
    rules.filter((r) => {
      const subject = subjectOf(r)
      return r.decls.some((d) => props.includes(d.prop)) && !!subject && compoundMatches(subject, el)
    }),
  )
/** Only a rule that declares a custom property can set one: tokensAt over those alone gives the same tokens. */
const tokenRules = (rules: Rule[]) => once(rules, 'tokens', () => rules.filter((r) => r.decls.some((d) => d.prop.startsWith('--'))))
const win = (rules: Rule[], mode: Mode, chain: El[], ...props: string[]) =>
  once(rules, `w|${mode}|${keyOf(chain)}|${props.join()}`, () => winnerAt(cutFor(rules, chain[chain.length - 1], props), mode, chain, ...props))
const tokensFor = (rules: Rule[], mode: Mode, chain: El[]) =>
  once(rules, `t|${mode}|${keyOf(chain)}`, () => tokensAt(tokenRules(rules), mode, chain))
const colour = (tokens: Map<string, string>, value: string, backdrop: Rgb): Rgb => {
  // `background: none` is a transparent face, as `transparent` is.
  const c = toRgb(expandWith(tokens, value === 'none' ? 'transparent' : value), backdrop)
  if (!c) throw new Error(`not a colour: "${value}" → "${expandWith(tokens, value)}"`)
  return c
}
const through = (c: Rgb, under: Rgb, opacity: number): Rgb => [0, 1, 2].map((i) => Math.round(c[i] * opacity + under[i] * (1 - opacity))) as unknown as Rgb

const FACE = ['background', 'background-color'] as const
const BORDER = ['border', 'border-color', 'border-top-color'] as const

/** Every control the author sheet leaves to the browser for its face, ink or border. */
function leftToTheBrowser(rules: Rule[], controls: Control[]): string[] {
  const out = new Set<string>()
  for (const c of controls) {
    for (const mode of SWEPT) {
      for (const [what, props] of [['face', FACE], ['ink', ['color']], ['border', BORDER]] as const) {
        if (!win(rules, mode, c.chain, ...props)) out.add(`${c.name}: the browser draws its ${what}`)
      }
    }
  }
  return [...out]
}

/** Every enabled control whose word reads under 4.5:1 on its own face, in any mode and backdrop. */
function unreadable(rules: Rule[], controls: Control[]): string[] {
  const out: string[] = []
  for (const c of controls.filter((x) => !x.disabled)) {
    for (const mode of SWEPT) {
      const tokens = tokensFor(rules, mode, c.chain)
      const face = win(rules, mode, c.chain, ...FACE)
      const ink = win(rules, mode, c.chain, 'color')
      if (!face || !ink) continue // leftToTheBrowser names it
      const opacity = Number(win(rules, mode, c.chain, 'opacity')?.value ?? 1)
      for (const s of BACKDROPS) {
        const under = colour(tokens, `var(${s})`, [0, 0, 0])
        const bg = through(colour(tokens, face.value, under), under, opacity)
        const fg = through(colour(tokens, ink.value, colour(tokens, face.value, under)), under, opacity)
        const r = contrast(fg, bg)
        if (r < TEXT_MIN) out.push(`${c.name} ${mode} on ${s}: ${hex(fg)} on ${hex(bg)} = ${r.toFixed(2)}:1`)
      }
    }
  }
  return out
}

// ── The macro keys with bare text (SstvView.tsx, Js8Cockpit.tsx) ─────────────────────────────

const header = [el('div', ['cockpit-header']), el('div', ['ch-mode-extras'])]
const MACRO_KEYS: Control[] = [
  { name: 'SSTV manual-receive Start', chain: [...header, el('button', ['cw-macro', 'sstv-manual-rx'], { type: 'button' })] },
  { name: 'SSTV image retry', chain: [el('div', ['sstv-remote-image']), el('button', ['cw-macro'], { type: 'button' })] },
  { name: 'JS8 station query', chain: [el('div', []), el('button', ['cw-macro', 'js8-query'], { type: 'button' })] },
  { name: 'JS8 inbox Read/Delete', chain: [el('div', []), el('button', ['cw-macro', 'js8-inbox-act'], { type: 'button' })] },
  { name: 'JS8 reply Yes', chain: [el('div', []), el('button', ['cw-macro', 'js8-confirm-yes'], { type: 'button' })] },
  { name: 'JS8 reply No', chain: [el('div', []), el('button', ['cw-macro', 'js8-confirm-no'], { type: 'button' })] },
  { name: 'JS8 Drop queue', chain: [el('div', []), el('button', ['cw-macro', 'js8-drop'], { type: 'button' })] },
  { name: 'JS8 Refresh', chain: [el('div', []), el('button', ['cw-macro'], { type: 'button' })] },
  { name: 'SSTV viewer Previous/Next/Save', chain: [el('div', ['sstv-viewer']), el('button', ['cw-macro'], { type: 'button' })] },
]

describe('the macro keys carry their own ink', () => {
  it('no bare-text macro key takes its lettering from the browser', () => {
    expect(leftToTheBrowser(RULES, MACRO_KEYS)).toEqual([])
  })

  it('every one reads 4.5:1 on its own face, in every mode', () => {
    expect(unreadable(RULES, MACRO_KEYS)).toEqual([])
  })

  it('FIRES: the key as it shipped, a face and a border but no ink, is caught', () => {
    const shipped = RULES.map((r) => (r.selector === '.cw-macro' ? { ...r, decls: r.decls.filter((d) => d.prop !== 'color') } : r))
    expect(leftToTheBrowser(shipped, MACRO_KEYS)).toContain('SSTV manual-receive Start: the browser draws its ink')
  })
})

// ── The mixed controls (SetupHealth.tsx, OperateRoster.tsx, SstvView.tsx) ──────────────────────

function healthChips(): Control[] {
  const r = render(<SetupHealth radio={{ catOk: true, rxLevel: 0.3, txEnabled: false }} catResult={null} onGoTo={() => {}} />)
  const out = [...r.container.querySelectorAll<HTMLButtonElement>('button.health-item')].map((b) => ({
    name: `Setup health "${b.textContent?.trim()}"`,
    chain: chainOf(b),
    disabled: b.disabled,
  }))
  cleanup()
  return out
}

const heard: Station = { call: 'W1AW', grid: 'FN31', snr: -10, lastHeardSlot: 100, heardCount: 1, presence: 'heard' as Station['presence'], worked: false }
function rosterSpot(): Control[] {
  const out: Control[] = []
  for (const selectedCall of ['W1AW', null]) {
    const r = render(
      <StationControlContext.Provider value={true}>
        <OperateRoster stations={[heard]} myGrid="EN52" currentSlot={100} needByCall={new Map()} selectedCall={selectedCall}
          onSelect={() => {}} onCall={() => {}} onToggleIgnore={() => {}} onSpot={() => {}} />
      </StationControlContext.Provider>,
    )
    const b = r.container.querySelector<HTMLButtonElement>('button.or-spot')
    expect(b, 'the roster has no Spot button').not.toBeNull()
    out.push({ name: `Operate roster Spot${b!.disabled ? ' (disabled)' : ''}`, chain: chainOf(b!), disabled: b!.disabled })
    cleanup()
  }
  return out
}

/** SstvView.tsx: the overlay rows sit in the compose `section.sstv-compose.panel`. */
const OVERLAY_REMOVE: Control = {
  name: 'SSTV overlay remove',
  chain: [el('section', ['sstv-compose', 'panel']), el('div', ['sstv-ov-row']), el('button', ['sstv-ov-remove'], { type: 'button' })],
}

describe('the three mixed controls paint their own face', () => {
  const controls = () => [...healthChips(), ...rosterSpot(), OVERLAY_REMOVE]

  it('renders the chips and both states of Spot (the census cannot silently empty out)', () => {
    expect(controls().map((c) => c.name)).toEqual(
      expect.arrayContaining(['Operate roster Spot', 'Operate roster Spot (disabled)', 'SSTV overlay remove']),
    )
    expect(healthChips().length).toBeGreaterThanOrEqual(2)
  })

  it('none of them is left to the browser for its face, ink or border', () => {
    expect(leftToTheBrowser(RULES, controls())).toEqual([])
  })

  it('each reads 4.5:1 on its own face, in both themes and every mode', () => {
    expect(unreadable(RULES, controls())).toEqual([])
  })

  it('a disabled Spot looks disabled', () => {
    for (const c of controls().filter((x) => x.disabled)) {
      expect(Number(win(RULES, 'light', c.chain, 'opacity')?.value ?? 1), c.name).toBeLessThan(1)
    }
  })

  it('FIRES: the three as they shipped, the theme\'s ink on the browser\'s face, are caught', () => {
    const shipped = RULES.filter((r) => !/health-item-link|or-spot/.test(r.selector)).map((r) =>
      r.selector === '.sstv-ov-remove' ? { ...r, decls: r.decls.filter((d) => !/^(background|border)/.test(d.prop)) } : r,
    )
    const left = leftToTheBrowser(shipped, controls())
    expect(left.some((m) => m.startsWith('Setup health') && m.endsWith('its face')), left.join('\n')).toBe(true)
    expect(left).toContain('Operate roster Spot: the browser draws its face')
    expect(left).toContain('SSTV overlay remove: the browser draws its face')
  })

  it('FIRES: the overlay remove with the raised compose box showing through again is caught on a theme', () => {
    const showThrough = RULES.map((r) =>
      r.selector === '.sstv-ov-remove' ? { ...r, decls: r.decls.map((d) => (d.prop === 'background' ? { ...d, value: 'transparent' } : d)) } : r,
    )
    const found = unreadable(showThrough, [OVERLAY_REMOVE])
    expect(found.some((m) => m.startsWith('SSTV overlay remove dark skin=lagoon on --bg-elev')), found.join('\n')).toBe(true)
    expect(found.some((m) => / (dark|light)(-\S+)? on /.test(m)), 'a standard theme reads either way').toBe(false)
  })
})

// ── The roster's distance picker (OperateRoster.tsx, #386) ─────────────────────────────────────

/** The Call Roster's distance picker, read off the rendered DOM. */
function rosterDistance(): Control[] {
  const r = render(
    <OperateRoster stations={[heard]} myGrid="EN52" currentSlot={100} needByCall={new Map()} selectedCall={null}
      onSelect={() => {}} onCall={() => {}} />,
  )
  const s = r.container.querySelector<HTMLSelectElement>('select.or-distance')
  expect(s, 'the roster has no distance picker').not.toBeNull()
  const out = [{ name: 'Operate roster distance picker', chain: chainOf(s!) }]
  cleanup()
  return out
}

describe('the roster’s distance picker paints its own face (#386)', () => {
  it('is not left to the browser for its face, ink or border', () => {
    expect(leftToTheBrowser(RULES, rosterDistance())).toEqual([])
  })

  it('reads 4.5:1 on its own face, in both themes and every mode', () => {
    expect(unreadable(RULES, rosterDistance())).toEqual([])
  })

  it('FIRES: the picker with neither its own rule nor the select base look is caught', () => {
    const bare = RULES.filter((r) => r.selector !== '.or-distance' && r.selector !== 'select')
    expect(bare.length, 'the rules under test are not in the sheet').toBe(RULES.length - 2)
    expect(leftToTheBrowser(bare, rosterDistance())).toContain('Operate roster distance picker: the browser draws its face')
  })
})

// ── The fully native three (PotaSotaView.tsx, SatellitesView.tsx) ──────────────────────────────

const POTA_SNAP = { hunt: null, radio: { dialMhz: 14.285 } } as unknown as AppSnapshot
const SATS_SNAP = {
  mycall: 'W9XYZ', mygrid: 'EN52', hunt: null, fieldDay: null, link: { tier: 'TempoFast' },
  radio: { dialMhz: 145.8, band: '2m', catOk: true, transmitting: false, txEnabled: false, txAllowed: true },
} as unknown as AppSnapshot

/** Every button the POTA/SOTA view renders, with no activation (Start) and with one running (Spot
 *  me, Stop), and every button in the Satellites header, read off the rendered DOM. */
async function viewButtons(): Promise<Control[]> {
  const out: Control[] = []
  const take = (root: ParentNode, where: string) => {
    for (const b of root.querySelectorAll<HTMLButtonElement>('button')) {
      const name = b.getAttribute('aria-label') || b.textContent?.trim() || b.title
      out.push({ name: `${where} "${name}"${b.disabled ? ' (disabled)' : ''}`, chain: chainOf(b), disabled: b.disabled })
    }
  }
  for (const activation of [null, { program: 'POTA', reference: 'US-0001', qsoCount: 3 }]) {
    views.getActivation.mockResolvedValue(activation)
    const r = render(<PotaSotaView snap={POTA_SNAP} />)
    await screen.findByRole('button', { name: activation ? t('ota.selfSpot.button') : t('ota.activation.start') })
    take(r.container, 'POTA')
    cleanup()
  }
  const r = render(<SatellitesView snap={SATS_SNAP} onPopOut={() => {}} />)
  const head = await waitFor(() => {
    const h = r.container.querySelector('.sats-head')
    if (!h?.querySelector('button')) throw new Error('the Satellites header has no buttons yet')
    return h
  })
  take(head, 'Satellites header')
  cleanup()
  // Both POTA renders carry the park list, so its buttons come twice.
  return [...new Map(out.map((c) => [`${c.name}|${keyOf(c.chain)}`, c])).values()]
}

/** Every disabled control that looks exactly like its enabled self: the browser greys a disabled
 *  button only while it paints the button, so a Nexus face has to say it. */
const disabledLooksEnabled = (rules: Rule[], controls: Control[]) =>
  controls.filter((c) => c.disabled && Number(win(rules, 'light', c.chain, 'opacity')?.value ?? 1) >= 1).map((c) => c.name)

describe('POTA’s Start and Download and the Satellites ⧉ wear Nexus’s own look', () => {
  let buttons: Control[] = []
  it('renders Start, Download and the ⧉, and Start disabled with no park typed (the sweep cannot silently empty out)', async () => {
    buttons = await viewButtons()
    const names = buttons.map((b) => b.name)
    expect(names).toEqual(expect.arrayContaining([
      `POTA "${t('ota.activation.start')}" (disabled)`,
      `POTA "${t('ota.parks.download')}"`,
      `POTA "${t('ota.selfSpot.button')}"`,
      `Satellites header "⧉"`,
    ]))
  })

  it('no button in the POTA view or the Satellites header is left to the browser for its face, ink or border', () => {
    expect(leftToTheBrowser(RULES, buttons)).toEqual([])
  })

  it('each reads 4.5:1 on its own face, in both themes and every mode', () => {
    expect(unreadable(RULES, buttons)).toEqual([])
  })

  it('a disabled one looks disabled', () => {
    expect(disabledLooksEnabled(RULES, buttons)).toEqual([])
  })

  it('FIRES: the three as they shipped, classed for no rule, are caught', () => {
    const shipped = buttons
      .filter((b) => /"(Start|Download|⧉)"/.test(b.name))
      .map((b) => {
        const last = b.chain[b.chain.length - 1]
        const cls = last.classes.includes('pota-act-start') ? ['pota-act-start'] : ['pane-popout']
        return { ...b, chain: [...b.chain.slice(0, -1), { ...last, classes: cls }] }
      })
    expect(shipped.length).toBe(3)
    const left = leftToTheBrowser(RULES, shipped)
    for (const b of shipped) expect(left, left.join('\n')).toContain(`${b.name}: the browser draws its face`)
  })

  it('FIRES: a disabled Start that looks enabled is caught', () => {
    const noDim = RULES.filter((r) => r.selector !== '.pota-parklist-import[disabled]')
    expect(noDim.length, 'the rule under test is not in the sheet').toBe(RULES.length - 1)
    expect(disabledLooksEnabled(noDim, buttons)).toContain(`POTA "${t('ota.activation.start')}" (disabled)`)
  })
})

// ── The watch list × (WatchlistPanel.tsx) ──────────────────────────────────────────────────────

/** The × on each entry of a watch list holding two, read off the rendered DOM. */
function watchlistRemoves(): Control[] {
  localStorage.setItem('nexus.watchlist', JSON.stringify([
    { id: 'call-VP8-a1', kind: 'call', value: 'VP8*' },
    { id: 'grid-FN31-b2', kind: 'grid', value: 'FN31', cqOnly: true },
  ]))
  const r = render(<WatchlistPanel />)
  const out = [...r.container.querySelectorAll<HTMLButtonElement>('button.watchlist-remove')].map((b) => ({
    name: `Watch list × "${b.getAttribute('aria-label')}"`,
    chain: chainOf(b),
  }))
  cleanup()
  localStorage.removeItem('nexus.watchlist')
  return out
}

describe('the watch list × paints its own face', () => {
  it('renders a × for each entry (a guard over an empty list is inert)', () => {
    expect(watchlistRemoves().length).toBe(2)
  })

  it('it is not left to the browser for its face, ink or border', () => {
    expect(leftToTheBrowser(RULES, watchlistRemoves())).toEqual([])
  })

  it('it reads 4.5:1 on its own face, in both themes and every mode', () => {
    expect(unreadable(RULES, watchlistRemoves())).toEqual([])
  })

  it('FIRES: the × as it shipped, the red on the browser\'s face, is caught', () => {
    const shipped = RULES.map((r) =>
      r.selector === '.watchlist-remove' ? { ...r, decls: r.decls.filter((d) => !/^(background|border)/.test(d.prop)) } : r,
    )
    const left = leftToTheBrowser(shipped, watchlistRemoves())
    expect(left.some((m) => m.endsWith('the browser draws its face')), left.join('\n')).toBe(true)
  })
})

// ── The watch list's note fields (WatchlistPanel.tsx, #390) ────────────────────────────────────

/** The note field on each entry of a watch list holding two, and the add row's, off the DOM. */
function watchlistNotes(): Control[] {
  localStorage.setItem('nexus.watchlist', JSON.stringify([
    { id: 'call-VP8-a1', kind: 'call', value: 'VP8*', notes: 'Falklands, until November' },
    { id: 'grid-FN31-b2', kind: 'grid', value: 'FN31', cqOnly: true },
  ]))
  const r = render(<WatchlistPanel />)
  const out = [...r.container.querySelectorAll<HTMLInputElement>('input.watchlist-notes, input.watchlist-notes-new')].map((i) => ({
    name: `Watch list note "${i.getAttribute('aria-label')}"`,
    chain: chainOf(i),
  }))
  cleanup()
  localStorage.removeItem('nexus.watchlist')
  return out
}

describe('the watch list’s note fields paint their own face (#390)', () => {
  it('renders one on each entry and one to add with (a guard over none is inert)', () => {
    expect(watchlistNotes().length).toBe(3)
  })

  it('none is left to the browser for its face, ink or border', () => {
    expect(leftToTheBrowser(RULES, watchlistNotes())).toEqual([])
  })

  it('each reads 4.5:1 on its own face, in both themes and every mode', () => {
    expect(unreadable(RULES, watchlistNotes())).toEqual([])
  })

  it('FIRES: an entry’s note field with no rule of its own is caught', () => {
    const bare = RULES.filter((r) => r.selector !== '.watchlist-notes')
    expect(bare.length, 'the rule under test is not in the sheet').toBe(RULES.length - 1)
    expect(leftToTheBrowser(bare, watchlistNotes())).toContain(
      `Watch list note "${t('watchlist.item.notes.aria', { value: 'VP8*' })}": the browser draws its face`,
    )
  })
})
