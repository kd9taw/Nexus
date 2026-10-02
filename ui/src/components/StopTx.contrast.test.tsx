// @vitest-environment jsdom
//
// EVERY STOP TX READS, IN EVERY THEME. The stop line (CLAUDE.md) says the operator must never be
// unable to stop a transmission, and a Stop TX the screen can barely separate from what it sits on
// is that loss by degrees. Measured on the cascade in every theme, and confirmed in Chrome:
//   - Operate's FT-strip Stop TX, the top bar's and the Remote log dialog's (one rule) drew the
//     outline as a 45 % mix of the red: under 3:1 in every theme, 1.61:1 on Lagoon's strip. Its
//     hover wrote white on the red, 3.39:1 in every dark theme, and its red word sat on a raised
//     surface: 3.64:1 on Lagoon's strip, 4.16:1 on its dialog.
//   - The cockpit header's Stop TX drew a 50 % outline, under 3:1 in every dark theme (2.37:1 on
//     Lagoon), and its hover's 22 % fill took the word under 4.5:1 in 18 of the 48 (3.95:1, Slate).
//   - SSTV's TX-bar Stop set its red word on a raised surface: 4.16:1 on Lagoon.
// The fix (styles.css): every stop outline is the solid red (the dock's Esc Stop already was); a
// hover fills the red and writes on it in the page colour; and a stop on a raised surface sits on
// the page colour, so its red word reads.
//
// Each host is RENDERED and placed in App's own chain (.app, .shell, the view's keep-alive host;
// on the Remote page .app.remote-workspace, and .remote-contact-host for Phone and CW), and every
// value is the cascade WINNER on that chain:
//   - the outline: the border, composited over the button's own face, against what the button sits
//     on (every ancestor's background, composited from the page down) — 3:1;
//   - the words: each run of text, read on the element that holds it (the dock key's words are two
//     spans with their own ink), against the face — 4.5:1;
//   - the focus ring: the focus outline against what the button sits on — 3:1;
//   - and nothing fades a live stop.
// States: idle; hover (:hover on the button and every ancestor, as a pointer sets it); keyboard
// focus (:focus and :focus-visible on the button, :focus-within up the chain). Each with the button
// ENABLED: the dock key and the TX-bar Stop render disabled until an over is on the air, and a stop
// matters while it is live. Modes: every base mode, every theme in each of its base's four modes,
// and each under every colour-role set (the accent moves the ring and the dock key's hover face).
// Sheets: the desktop's, and the Remote page's (+ remote-web/application.css).
//
// What it cannot see: jsdom never lays out, so a stop that is covered or pushed off screen reads
// fine here (the stop-line sweeps and scripts/browser-probe own that). Roam's Stop shares the
// strip's rule but ends a QSY sequence, not a transmission, so it is not held here.
import { describe, it, expect, vi, beforeAll } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { PhoneCockpit } from './PhoneCockpit'
import { CwCockpit } from './CwCockpit'
import { RttyCockpit } from './RttyCockpit'
import { PskCockpit } from './PskCockpit'
import { Js8Cockpit } from './Js8Cockpit'
import { SstvView } from './SstvView'
import { OperateCockpit } from './OperateCockpit'
import { TopBar } from './TopBar'
import { LogConfirm } from './LogConfirm'
import {
  BASE_MODES,
  MODES,
  SKIN_MODES,
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
import { SKINS } from '../features/skins'
import { PALETTE_ROLES } from '../features/paletteRoles'
import type { PanelLayoutApi } from '../features/panelState'
import type { AppSnapshot, Js8State, LoggedQso, PskState, RadioStatus, RttyState, SstvState } from '../types'

// ── The hosts, rendered as App renders them ─────────────────────────────────────────────────

const rttyState = {
  armed: true, afcHz: 0, afcLocked: false, text: '', charConf: [], baud: 45.45, shiftHz: 170, markHz: 2125,
  spaceHz: 2295, sending: false, latched: false, backend: 'afsk', keyerError: null, auto: false, seqState: 'idle',
  peer: null, peerExchange: [], heardCq: null,
} as unknown as RttyState
const pskState = {
  armed: true, afcHz: 0, signal: false, centerHz: 1000, text: '', charConf: [], sending: false, latched: false,
  keyerError: null,
} as unknown as PskState
const js8State = {
  speed: 'normal', rxSpeeds: 15, txEnabled: true, sending: false, hbOn: false, hbNextAtMs: null, hbIntervalMin: 0,
  cqOn: false, cqNextAtMs: null, cqIntervalMin: 0, autoreply: true, relay: true, hbAck: false,
  armed: { autoreply: false, relay: false, hbAck: false, hb: false, cq: false }, idleMinutes: 0, idleLimitMin: 60,
  idleTripped: false, activity: [], stations: [], inbox: [], queue: [], pendingReply: null, lastError: null,
} as unknown as Js8State
const sstvState = {
  armed: false, mode: null, linesDone: 0, linesTotal: 0, previewRgbBase64: null, previewWidth: 0, previewHeight: 0,
  hedrShiftHz: 0, gallery: [],
  health: {
    armed: false, audioPeak: 0, lastAudioUnix: null, drains: 0, visSeen: 0, lastVisUnix: null, unknownVis: 0,
    lastUnknownVisCode: null, lastUnknownVisUnix: null, images: 0, lastImageUnix: null,
  },
  sending: false, txMode: null, txProgress: 0, txElapsedSecs: 0, txTotalSecs: 0,
} as unknown as SstvState
const cwDecode = {
  text: '', wpm: 22, sent: [], keyerError: null, candidates: [], state: 'listening', headline: '', prompt: '',
  recommended: null, workedCall: null, rst: null, name: null,
}

// The real module, every function auto-stubbed (a hand-kept list throws on mount the day a host
// calls an export it omits); the entries below are the ones whose SHAPE a host reads on mount.
vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  return {
    ...auto,
    appVersion: vi.fn(async () => '0.0.0'),
    getCatCwUnprovenRigModels: vi.fn(async () => []),
    getVoiceMessages: vi.fn(async () => []),
    stopVoiceRecording: vi.fn(async () => []),
    clearVoiceMessage: vi.fn(async () => []),
    importVoiceMessage: vi.fn(async () => []),
    getSettings: vi.fn(async () => ({ macros: { cwProfiles: [], activeCwProfile: 0 } })),
    setCwKeyer: vi.fn(async () => null),
    cwDecode: vi.fn(async () => cwDecode),
    selectPeer: vi.fn(async () => null),
    previewCw: vi.fn(async (t: string) => t),
    pointRotatorAtCall: vi.fn(async () => 0),
    readRotator: vi.fn(async () => null),
    getDeclination: vi.fn(async () => 0),
    getSatTrackStatus: vi.fn(async () => null),
    getSatTransponder: vi.fn(async () => null),
    getLicensedBandPlan: vi.fn(async () => []),
    getSpectrumRow: vi.fn(async () => null),
    getRttyState: vi.fn(async () => rttyState),
    rttyArm: vi.fn(async () => rttyState),
    rttyAutoArm: vi.fn(async () => rttyState),
    getPskState: vi.fn(async () => pskState),
    pskArm: vi.fn(async () => pskState),
    pskAutoArm: vi.fn(async () => pskState),
    getJs8State: vi.fn(async () => js8State),
    js8Enter: vi.fn(async () => js8State),
    js8Arm: vi.fn(async () => js8State),
    getSstvState: vi.fn(async () => sstvState),
    sstvArm: vi.fn(async () => sstvState),
    sstvAutoArm: vi.fn(async () => sstvState),
  }
})
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()) }))
// Canvases and scopes only; every header, strip and dock is real.
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))

const noop = () => {}
function panels<P extends string>(): PanelLayoutApi<P> {
  return {
    layout: { v: 1, state: {}, share: {} },
    stateOf: () => 'docked',
    setPanelState: noop,
    shareOf: () => 1,
    setShare: noop,
    setShares: noop,
    undo: noop,
    canUndo: false,
    undoRemoves: [],
    reset: noop,
  }
}
const radio = {
  dialMhz: 14.2, band: '20m', catOk: true, sideband: 'USB', sidebandOverride: null, rigMode: 'USB', rigConfirmed: true,
  transmitting: false, tuning: false, txEnabled: true, txAllowed: true, qsoRecording: false, rfPower: null, micGain: null,
  nrLevel: 0.3, agc: 'fast', nb: true, nr: true, notch: null, comp: null, vox: null, filterWidthHz: 500, splitTxMhz: null,
  smeterDb: null, cwWpm: 22, cwKeyer: 'cat', phoneSegLo: null, phoneSegHi: null, nextSlotMs: 5000, slot: 0,
  rxOffsetHz: 1500, txOffsetHz: 1500, txLevel: 0.5, txEven: true, txCycleAuto: true, txBusyReason: null, rigKeyed: false,
  dtSec: 0, clockOffsetMs: 0, source: 'native', sourceLabel: 'Native', atu: true,
} as unknown as RadioStatus
const snap = {
  mycall: 'KD9TAW', mygrid: 'EN61', radio, stations: [], recentDecodes: [], conversations: [], highlights: [],
  harqRescues: 0, clearTick: 0, qso: null, link: { tier: 'FT8' },
} as unknown as AppSnapshot
const pending = { call: 'W9XYZ', grid: 'EN37', rstSent: '-07', rstRcvd: '-10', band: '20m', mode: 'FT8', whenUnix: 1700000000 } as LoggedQso

type Variant = 'txstrip' | 'strip' | 'topbar' | 'dialog' | 'dock' | 'txbar'
const VARIANTS: Record<Variant, { name: string; selector: string }> = {
  // Every screen's TX strip but FT's (2026-10-01): the cockpit header's Stop TX moved here.
  txstrip: { name: "the TX strip's Stop TX", selector: '.cockpit-txstrip .op-btn.stop' },
  strip: { name: "Operate's FT-strip Stop TX", selector: '.op-btn.stop' },
  topbar: { name: "the top bar's Stop TX", selector: '.op-btn.stop' },
  dialog: { name: "the log dialog's Stop TX", selector: '.op-btn.stop' },
  dock: { name: "the dock's Esc Stop key", selector: '.rtty-stop' },
  txbar: { name: "SSTV's TX-bar Stop", selector: '.sstv-tx-stop' },
}

type Sheet = 'desktop' | 'Remote'
interface Host {
  name: string
  /** App's view id, which the Remote page carries as `data-remote-view`. */
  view: string
  /** Under `.shell` in App's keep-alive host (null: the view's workspace itself), or a direct child of `.app`. */
  host: string | null | 'app'
  sheets: readonly Sheet[]
  stops: readonly Variant[]
  mount: () => void
}
const BOTH: readonly Sheet[] = ['desktop', 'Remote']
const HOSTS: readonly Host[] = [
  { name: 'Phone', view: 'phone', host: null, sheets: BOTH, stops: ['txstrip'], mount: () => render(<PhoneCockpit snap={snap} theme="dark" onWorkSpot={noop} spots={[]} panels={panels()} />) },
  { name: 'CW', view: 'cw', host: null, sheets: BOTH, stops: ['txstrip'], mount: () => render(<CwCockpit snap={snap} theme="dark" onWorkSpot={noop} spots={[]} panels={panels()} />) },
  { name: 'RTTY', view: 'rtty', host: 'rtty-host', sheets: BOTH, stops: ['txstrip', 'dock'], mount: () => render(<RttyCockpit snap={snap} panels={panels()} onSetTxEnabled={noop} />) },
  { name: 'PSK', view: 'psk', host: 'psk-host', sheets: BOTH, stops: ['txstrip', 'dock'], mount: () => render(<PskCockpit snap={snap} panels={panels()} onSetTxEnabled={noop} />) },
  { name: 'JS8', view: 'js8', host: 'js8-host', sheets: BOTH, stops: ['txstrip'], mount: () => render(<Js8Cockpit snap={snap} panels={panels()} onSetTxEnabled={noop} />) },
  { name: 'SSTV', view: 'sstv', host: 'sstv-host', sheets: BOTH, stops: ['txstrip', 'txbar'], mount: () => render(<SstvView snap={snap} panels={panels()} onSetTxEnabled={noop} />) },
  {
    name: 'Operate', view: 'operate', host: 'operate-host', sheets: BOTH, stops: ['strip'],
    mount: () =>
      render(
        <OperateCockpit
          snap={snap} theme="dark" tier="FT8" fdActive={false} fdRuleset={null} onTierChange={noop} bandPlan={[]}
          onSetFrequency={noop} onSourceChange={noop} onTune={noop} onCall={noop} onSetTxLevel={noop} onSetMode={noop}
          onSetTxEven={noop} onSetTxCycleAuto={noop} onResend={noop} onFreetext={noop} onLog={noop} onOverrideTx={noop}
          onHaltTx={noop} roster={<div />} needByCall={new Map()} selectedCall={null} onSelect={noop}
          layoutMode="classic" onLayoutMode={noop} panels={panels()} active={false}
        />,
      ),
  },
  {
    // Shown in every view that is not a cockpit (App hides its TX cluster there): the map, Connect,
    // the log, the Field Day scoreboard. It can be the only stop on the screen.
    name: 'the top bar', view: 'map', host: 'app', sheets: BOTH, stops: ['topbar'],
    mount: () =>
      render(
        <TopBar
          mycall="KD9TAW" mygrid="EN61" radio={radio} link={{ tier: 'FT8' } as never} bandPlan={[]}
          onSetFrequency={noop} onSetTxEnabled={noop} onSetTune={noop} onHaltTx={noop} onSetTxEven={noop}
          onSetTxCycleAuto={noop} onSetHoldTxFreq={noop} tier="FT8" onTierChange={noop} onOpenGuide={noop}
        />,
      ),
  },
  {
    // App gives the log dialog its Stop TX on the Remote page only (`onStop={remote ? … : undefined}`).
    name: 'the log dialog', view: 'operate', host: 'app', sheets: ['Remote'], stops: ['dialog'],
    mount: () => render(<LogConfirm record={pending} onConfirm={noop} onDiscard={noop} onStop={noop} />),
  },
]

const el = (tag: string, classes: string[], attrs: Record<string, string> = {}): El => ({ tag, classes, attrs })
/** App's elements above a host's own root. */
function aboveHost(h: Host, sheet: Sheet): El[] {
  const app = sheet === 'Remote'
    ? el('div', ['app', 'remote-workspace'], { 'data-remote-presentation': 'full', 'data-remote-view': h.view })
    : el('div', ['app'])
  if (h.host === 'app') return [app]
  const host = sheet === 'Remote' && (h.view === 'phone' || h.view === 'cw') ? 'remote-contact-host' : h.host
  return [app, el('div', ['shell']), ...(host ? [el('div', [host])] : [])]
}

interface Stop {
  host: Host
  variant: Variant
  /** From the host's root down to the button (the test container dropped). */
  chain: El[]
  /** Each element holding a run of visible text, from the host's root down. */
  texts: { text: string; chain: El[] }[]
}

async function mountAll(): Promise<Stop[]> {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  const out: Stop[] = []
  for (const h of HOSTS) {
    h.mount()
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })
    for (const v of new Set(h.stops)) {
      const found = [...document.body.querySelectorAll(VARIANTS[v].selector)]
      for (const b of found) {
        const texts: Stop['texts'] = []
        const visit = (e: Element) => {
          const own = [...e.childNodes].filter((n) => n.nodeType === 3).map((n) => n.textContent ?? '').join('').trim()
          if (own) texts.push({ text: own, chain: chainOf(e).slice(1) })
          for (const c of e.children) visit(c)
        }
        visit(b)
        out.push({ host: h, variant: v, chain: chainOf(b).slice(1), texts })
      }
    }
    cleanup()
  }
  return out
}

// ── The cascade ─────────────────────────────────────────────────────────────────────────────

const sheetText = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const ORDER = { n: 0 }
const DESKTOP_RULES = parseRules(sheetText('styles.css') + '\n' + sheetText('cockpit-panes.css'), ORDER)
const REMOTE_RULES = [...DESKTOP_RULES, ...parseRules(sheetText('remote-web/application.css'), { n: ORDER.n })]
/** Rules of our own, parsed after the sheet so they win its ties (the controls below). */
const withBlock = (rules: Rule[], css: string): Rule[] => [...rules, ...parseRules(css, { n: ORDER.n + 100_000 })]

/** The resolver reads a state pseudo-class as a state the element is not in; each state is named
 *  as an attribute instead, carried by the elements that are in it. */
const asStates = (rules: Rule[]): Rule[] =>
  rules.map((r) => ({
    ...r,
    selector: r.selector
      .replace(/:hover/g, '[data-hover]')
      .replace(/:focus-visible/g, '[data-focus]')
      .replace(/:focus-within/g, '[data-focus-within]')
      .replace(/:focus(?![-\w])/g, '[data-focus]')
      .replace(/:disabled/g, '[disabled]')
      .replace(/:enabled/g, ':not([disabled])'),
  }))

type State = 'idle' | 'hover' | 'focus'
const STATES: readonly State[] = ['idle', 'hover', 'focus']
/** `chain` (the button at `at`, descendants after it) in `state`, the button enabled. */
function inState(chain: El[], at: number, state: State): El[] {
  return chain.map((e, i) => {
    const attrs = { ...e.attrs }
    if (i === at) delete attrs.disabled
    if (state === 'hover' && i <= at) attrs['data-hover'] = ''
    if (state === 'focus' && i <= at) attrs['data-focus-within'] = ''
    if (state === 'focus' && i === at) attrs['data-focus'] = ''
    return { ...e, attrs }
  })
}

const memo = new WeakMap<Rule[], Map<string, unknown>>()
function once<T>(rules: Rule[], key: string, make: () => T): T {
  let byKey = memo.get(rules)
  if (!byKey) memo.set(rules, (byKey = new Map()))
  if (!byKey.has(key)) byKey.set(key, make())
  return byKey.get(key) as T
}

/** Each rule as the sweep needs it, worked out once per rule set. */
interface Ix {
  rule: Rule
  /** <html> may match it (theme, contrast, night and preset blocks), so the tokens keep it. */
  root: boolean
  /** The compound the element itself must match — null for a sibling combinator, which never
   *  reaches (reachesChain) — and the classes that compound needs. */
  subject: string | null
  classes: string[]
  tokens: boolean
  props: ReadonlySet<string>
  /** Names an attribute only <html> carries: the one way a mode enters whether a rule reaches. */
  modal: boolean
}
const ROOT_ATTR = new RegExp(`\\[data-(theme|contrast|night|skin|${PALETTE_ROLES.map((r) => r.id).join('|')})\\b`)
const indexOf = (rules: Rule[]): Ix[] =>
  once(rules, 'ix', () =>
    rules.map((rule) => {
      const sel = rule.selector
      const sibling = /[+~]/.test(sel.replace(/\[[^\]]*\]/g, '').replace(/\([^)]*\)/g, ''))
      const subject = sibling ? null : (sel.replace(/\s*>\s*/g, ' > ').split(/\s+/).filter(Boolean).pop() ?? null)
      return {
        rule,
        root: /^(html)?(:root|\[[^\]]*\])+$|^html$/.test(sel.replace(/\s+/g, '')),
        subject,
        classes: (subject?.replace(/:not\([^()]*\)/g, '').replace(/\[[^\]]*\]/g, '').match(/\.[\w-]+/g) ?? []).map((c) => c.slice(1)),
        tokens: rule.decls.some((d) => d.prop.startsWith('--')),
        props: new Set(rule.decls.map((d) => d.prop)),
        modal: ROOT_ATTR.test(sel),
      }
    }),
  )
const reaches = (x: Ix, e: El) => x.subject !== null && x.classes.every((c) => e.classes.includes(c)) && compoundMatches(x.subject, e)

/** One chain in one state: an id for each prefix, and at each element the rules whose subject it
 *  matches — the only rules that can win or declare anything there. */
interface Cut {
  chain: El[]
  ids: number[]
  at: Ix[][]
  /** The cut whose prefixes decide the tokens: the idle one when no token rule names a state. */
  tokens: Cut
}
const FACE = ['background', 'background-color'] as const
const INK = ['color'] as const
const BORDER = ['border', 'border-color', 'border-top-color'] as const
const OUTLINE = ['outline', 'outline-color'] as const
const OPACITY = ['opacity'] as const
const colour = (tokens: Map<string, string>, value: string, backdrop: Rgb): Rgb => {
  const v = expandWith(tokens, value).trim()
  // `none` is no paint, as `transparent` is; a border or outline that names no colour paints nothing.
  const c = toRgb(v === '' || v === 'none' ? 'transparent' : v, backdrop)
  if (!c) throw new Error(`not a colour: "${value}" → "${v}"`)
  return c
}
/** The colour in a `border` or `outline` shorthand, or the value as it stands ('' when it names none). */
function paintOf(value: string): string {
  const parts: string[] = []
  let depth = 0
  let buf = ''
  for (const ch of `${value.trim()} `) {
    if (ch === '(') depth++
    else if (ch === ')') depth--
    if (/\s/.test(ch) && depth === 0) {
      if (buf) parts.push(buf)
      buf = ''
    } else buf += ch
  }
  if (parts.some((p) => /^(none|hidden)$/.test(p)) || parts.some((p) => /^0(px|em|rem)?$/.test(p))) return ''
  return parts.filter((p) => !/^-?[\d.]+(px|em|rem)?$/.test(p) && !/^(solid|dashed|dotted|double|groove|ridge|inset|outset|thin|medium|thick|auto)$/.test(p)).join(' ')
}
const at = (w: { rule: Rule } | null) => (w ? w.rule.selector : 'nothing')

// ── One reading per stop × sheet × mode × state ─────────────────────────────────────────────

/** Every base mode, every theme in each of its base's four, each under every colour-role set. */
const SWEPT: readonly Mode[] = [...new Set<Mode>([...MODES, ...SKIN_MODES])]

interface Reading {
  stop: Stop
  sheet: Sheet
  mode: Mode
  state: State
  outline: { ratio: number; border: string; under: string; rule: string }
  words: { text: string; ratio: number; ink: string; face: string; rule: string }[]
  ring: { ratio: number; ring: string; under: string } | null
  faded: string | null
}

function read(stop: Stop, sheet: Sheet, all: Rule[], modes: readonly Mode[]): Reading[] {
  const top = aboveHost(stop.host, sheet)
  const button = [...top, ...stop.chain]
  const at0 = button.length - 1
  const raw = STATES.map((s) => ({ s, button: inState(button, at0, s), texts: stop.texts.map((t) => inState([...top, ...t.chain], at0, s)) }))
  const els = raw.flatMap((x) => [x.button, ...x.texts]).flat()
  const have = new Set(els.flatMap((e) => e.classes))
  // Every rule that can reach an element here, and every rule <html> can match: the cascade's
  // answer on these chains is the same with the others gone, and the sweep takes seconds.
  const relevant = indexOf(once(all, 'states', () => asStates(all))).filter(
    (x) => x.root || (x.classes.every((c) => have.has(c)) && els.some((e) => reaches(x, e))),
  )
  const tokenRules = relevant.filter((x) => x.tokens).map((x) => x.rule)
  const stateFree = !tokenRules.some((r) => /\[(data-hover|data-focus|data-focus-within|disabled)\]/.test(r.selector))
  const cache = new Map<string, unknown>()
  const mine = <T,>(key: string, make: () => T): T => {
    if (!cache.has(key)) cache.set(key, make())
    return cache.get(key) as T
  }
  const ids = new Map<string, number>()
  const cutOf = (chain: El[], idle?: Cut): Cut => {
    let k = ''
    const c: Cut = {
      chain,
      ids: chain.map((e) => {
        k += `/${JSON.stringify([e.tag, e.classes, e.attrs])}`
        let id = ids.get(k)
        if (id === undefined) ids.set(k, (id = ids.size))
        return id
      }),
      // A rule <html> can match may match an element too (`[data-focus]`, the focus ring's).
      at: chain.map((e) => relevant.filter((x) => reaches(x, e))),
      tokens: undefined as unknown as Cut,
    }
    c.tokens = stateFree && idle ? idle : c
    return c
  }
  const idleButton = cutOf(raw[0].button)
  const idleTexts = raw[0].texts.map((t) => cutOf(t))
  const byState = raw.map((x, n) => ({
    s: x.s,
    button: n === 0 ? idleButton : cutOf(x.button, idleButton),
    texts: stop.texts.map((t, j) => ({ text: t.text, k: n === 0 ? idleTexts[j] : cutOf(x.texts[j], idleTexts[j]) })),
  }))
  /** The winner of `props` on the chain cut at `i`. Computed once for every mode when no rule
   *  that could win there names an attribute of <html>. */
  const win = (mode: Mode, c: Cut, i: number, props: readonly string[]) => {
    const cands = c.at[i].filter((x) => props.some((p) => x.props.has(p)))
    if (!cands.length) return null
    const modal = cands.some((x) => x.modal)
    return mine(`w|${modal ? mode : '*'}|${c.ids[i]}|${props.join()}`, () => winnerAt(cands.map((x) => x.rule), mode, c.chain.slice(0, i + 1), ...props))
  }
  /** The tokens at the chain cut at `i`: those at the last element there that a token rule can
   *  reach, since an element that declares nothing inherits every token unchanged. */
  const tokensFor = (mode: Mode, c: Cut, i: number) => {
    const t = c.tokens
    let j = i
    while (j >= 0 && !t.at[j].some((x) => x.tokens)) j--
    return mine(`t|${mode}|${j < 0 ? -1 : t.ids[j]}`, () => tokensAt(tokenRules, mode, t.chain.slice(0, j + 1)))
  }
  const out: Reading[] = []
  for (const mode of modes) {
    for (const { s, button: b, texts: words } of byState) {
      // What the button sits on: every ancestor's own background, composited from the page down.
      let under = colour(tokensFor(mode, b, 0), 'var(--bg)', [0, 0, 0])
      let faded: string | null = null
      for (let i = 0; i <= at0; i++) {
        const op = win(mode, b, i, OPACITY)
        if (op && Number(expandWith(tokensFor(mode, b, i), op.value)) < 1) faded = `${op.rule.selector} { opacity: ${op.value} }`
        if (i === at0) break
        const bg = win(mode, b, i, FACE)
        if (bg) under = colour(tokensFor(mode, b, i), bg.value, under)
      }
      const tokens = tokensFor(mode, b, at0)
      const faceW = win(mode, b, at0, FACE)
      const face = faceW ? colour(tokens, faceW.value, under) : under
      const borderW = win(mode, b, at0, BORDER)
      const border = borderW ? colour(tokens, paintOf(borderW.value), face) : face
      const ringW = s === 'focus' ? win(mode, b, at0, OUTLINE) : null
      const ringPaint = ringW ? paintOf(ringW.value) : ''
      const ring = ringPaint ? colour(tokens, ringPaint, under) : null
      out.push({
        stop, sheet, mode, state: s,
        outline: { ratio: contrast(border, under), border: hex(border), under: hex(under), rule: at(borderW) },
        words: words.map(({ text, k }) => {
          // The ink: the nearest `color` winner at or above the element holding the words.
          let backdrop = face
          for (let i = at0 + 1; i < k.chain.length; i++) {
            const bg = win(mode, k, i, FACE)
            if (bg) backdrop = colour(tokensFor(mode, k, i), bg.value, backdrop)
          }
          for (let i = k.chain.length - 1; i >= 0; i--) {
            const c = win(mode, k, i, INK)
            if (!c || /^(inherit|currentcolor)$/i.test(c.value.trim())) continue
            const ink = colour(tokensFor(mode, k, i), c.value, backdrop)
            return { text, ratio: contrast(ink, backdrop), ink: hex(ink), face: hex(backdrop), rule: at(c) }
          }
          throw new Error(`${stop.host.name}: nothing colours "${text}"`)
        }),
        ring: s === 'focus' ? (ring ? { ratio: contrast(ring, under), ring: hex(ring), under: hex(under) } : { ratio: 0, ring: 'none', under: hex(under) }) : null,
        faded,
      })
    }
  }
  return out
}

let STOPS: Stop[] = []
let READINGS: Reading[] = []
beforeAll(async () => {
  STOPS = await mountAll()
  READINGS = STOPS.flatMap((s) => s.host.sheets.flatMap((sheet) => read(s, sheet, sheet === 'Remote' ? REMOTE_RULES : DESKTOP_RULES, SWEPT)))
}, 300_000)

// ── The checks, each a pure function of readings so the controls below can make one fail ────

const OUTLINE_MIN = 3
const TEXT_MIN = 4.5
const RING_MIN = 3

/** One line per stop × sheet × state that misses, naming its worst mode: every mode is swept, and
 *  a list of every failing one says less than its count and its worst case. */
function summary(rows: { r: Reading; ratio: number; what: string }[], min: number): string[] {
  const groups = new Map<string, { r: Reading; ratio: number; what: string }[]>()
  for (const x of rows.filter((x) => x.ratio < min)) {
    const k = `${x.r.stop.host.name} (${x.r.sheet}) ${x.r.state}`
    groups.set(k, [...(groups.get(k) ?? []), x])
  }
  return [...groups]
    .map(([k, xs]) => {
      const w = xs.reduce((a, b) => (b.ratio < a.ratio ? b : a))
      const of = new Set(rows.filter((x) => `${x.r.stop.host.name} (${x.r.sheet}) ${x.r.state}` === k).map((x) => x.r.mode)).size
      return { w, line: `${k}: ${w.what} under ${min}:1 in ${new Set(xs.map((x) => x.r.mode)).size} of ${of} modes, worst ${w.ratio.toFixed(2)}:1 in ${w.r.mode}` }
    })
    .sort((a, b) => a.w.ratio - b.w.ratio)
    .map((x) => x.line)
}
const outlineProblems = (rs: Reading[]) =>
  summary(rs.map((r) => ({ r, ratio: r.outline.ratio, what: `the outline ${r.outline.border} on ${r.outline.under} (${r.outline.rule})` })), OUTLINE_MIN)
const wordProblems = (rs: Reading[]) =>
  summary(rs.flatMap((r) => r.words.map((w) => ({ r, ratio: w.ratio, what: `"${w.text}" ${w.ink} on ${w.face} (${w.rule})` }))), TEXT_MIN)
const ringProblems = (rs: Reading[]) =>
  summary(rs.filter((r) => r.ring).map((r) => ({ r, ratio: r.ring!.ratio, what: `the focus ring ${r.ring!.ring} on ${r.ring!.under}` })), RING_MIN)
const fadeProblems = (rs: Reading[]) => [...new Set(rs.filter((r) => r.faded).map((r) => `${r.stop.host.name} (${r.sheet}) ${r.state} ${r.mode}: ${r.faded}`))]

const ofVariant = (v: Variant) => READINGS.filter((r) => r.stop.variant === v)

describe('every Stop TX is on screen to be measured', () => {
  it('each host renders its stops, words and all, and the sweep covers every theme', () => {
    expect(STOPS.map((s) => `${s.host.name}: ${VARIANTS[s.variant].name} — ${s.texts.map((t) => t.text).join(' ')}`)).toEqual([
      "Phone: the TX strip's Stop TX — Stop TX",
      "CW: the TX strip's Stop TX — Stop TX",
      "RTTY: the TX strip's Stop TX — Stop TX",
      "RTTY: the dock's Esc Stop key — Esc Stop",
      "PSK: the TX strip's Stop TX — Stop TX",
      "PSK: the dock's Esc Stop key — Esc Stop",
      "JS8: the TX strip's Stop TX — Stop TX",
      "SSTV: the TX strip's Stop TX — Stop TX",
      "SSTV: SSTV's TX-bar Stop — Stop",
      "Operate: Operate's FT-strip Stop TX — Stop TX",
      "the top bar: the top bar's Stop TX — Stop TX",
      "the log dialog: the log dialog's Stop TX — Stop TX",
    ])
    // Every base mode and every theme in each of its base's four modes, at the least.
    for (const m of [...BASE_MODES, ...SKINS.flatMap((s) => BASE_MODES.filter((b) => b.startsWith(s.base)).map((b) => `${b} skin=${s.id}`))]) {
      expect(SWEPT, `${m} is not swept`).toContain(m)
    }
    expect(READINGS.length).toBe(STOPS.reduce((n, s) => n + s.host.sheets.length, 0) * SWEPT.length * STATES.length)
  })
})

describe.each((Object.keys(VARIANTS) as Variant[]).map((v) => [VARIANTS[v].name, v] as const))('%s', (_name, v) => {
  it('its outline is 3:1 against what it sits on, idle, hovered and focused, in every theme', () => {
    expect(outlineProblems(ofVariant(v))).toEqual([])
  })
  it('its words are 4.5:1 on its face, idle, hovered and focused, in every theme', () => {
    expect(wordProblems(ofVariant(v))).toEqual([])
  })
  it('its focus ring is 3:1 against what it sits on, in every theme', () => {
    expect(ringProblems(ofVariant(v))).toEqual([])
  })
  it('nothing fades it while it is live', () => {
    expect(fadeProblems(ofVariant(v))).toEqual([])
  })
})

// ── Positive controls: each check says no to the rules this file was written against ─────────

describe('the checks fire on the rules before the fix', () => {
  const again = (v: Variant, css: string, modes: readonly Mode[]) =>
    STOPS.filter((s) => s.variant === v).flatMap((s) => read(s, 'desktop', withBlock(DESKTOP_RULES, css), modes))

  it("the strip's 45 % outline", () => {
    const rs = again('strip', `.op-btn.stop { background: transparent; border-color: color-mix(in srgb, var(--state-weak) 45%, transparent); }`, ['dark skin=lagoon'])
    expect(outlineProblems(rs)[0]).toMatch(/^Operate \(desktop\) (idle|focus): the outline .* worst 1\.61:1 in dark skin=lagoon$/)
  })
  it("the strip's white hover word", () => {
    const rs = again('strip', `.op-btn.stop:hover { color: #fff; }`, ['dark'])
    expect(wordProblems(rs)).toEqual([expect.stringMatching(/^Operate \(desktop\) hover: "Stop TX" #ffffff on #ec5b57 .* worst 3\.39:1 in dark$/)])
  })
  // The six TX strips draw FT's Stop TX (`.op-btn.stop`), so the strip's own regressions are
  // what fires on them: they replaced the cockpit header's `.cockpit-stoptx` (2026-10-01).
  it("the TX strips' 45 % outline", () => {
    const rs = again('txstrip', `.op-btn.stop { background: transparent; border-color: color-mix(in srgb, var(--state-weak) 45%, transparent); }`, ['dark skin=lagoon'])
    // All six strips, idle (and focused, which paints the same); the hover's fill decides its own.
    expect(outlineProblems(rs).filter((l) => l.includes(' idle: '))).toHaveLength(6)
    expect(outlineProblems(rs)[0]).toMatch(/worst 1\.61:1 in dark skin=lagoon$/)
  })
  it("the TX strips' 22 % hover", () => {
    const rs = again('txstrip', `.op-btn.stop:hover { background: color-mix(in srgb, var(--state-weak) 22%, transparent); color: var(--state-weak); }`, ['dark skin=slate'])
    expect(wordProblems(rs)).toHaveLength(6)
    expect(wordProblems(rs)[0]).toMatch(/hover: "Stop TX" #ec5b57 on #[0-9a-f]{6} .* worst \d\.\d\d:1 in dark skin=slate$/)
  })
  it("the TX bar's transparent face", () => {
    const rs = again('txbar', `.sstv-tx-stop:not(:disabled) { background: transparent; }`, ['dark skin=lagoon'])
    expect(wordProblems(rs)[0]).toMatch(/^SSTV \(desktop\) (idle|hover|focus): "Stop" #ec5b57 on #063039 .* worst 4\.16:1 in dark skin=lagoon$/)
  })
  it('a focus ring taken away, and a faded stop', () => {
    const rs = again('txstrip', `.op-btn.stop:focus-visible { outline: none; } .op-btn.stop { opacity: .5; }`, ['dark'])
    expect(ringProblems(rs)[0]).toMatch(/focus: the focus ring none .* worst 0\.00:1 in dark$/)
    expect(fadeProblems(rs)[0]).toMatch(/: \.op-btn\.stop \{ opacity: \.5 \}$/)
  })
})
