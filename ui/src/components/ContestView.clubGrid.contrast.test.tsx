// @vitest-environment jsdom
//
// EVERY NAME IN THE CLUB GRID READS, AND EVERY LINK COLOUR SHOWS, IN EVERY THEME AND MODE —
// computed from the sheet with the app's own resolver (cssCascade.ts), the way the dashboard bar and
// the band chip are held. jsdom lays nothing out, so this measures what the cascade PAINTS.
//
// The rule it holds is the app's for a state colour: the WORD is in ink and the state rides on the
// name's edge and a tint of its face, because the state colours lettered on the page miss 4.5:1 in
// the light themes (the warning amber reads 4.23:1 on the light panel; FIRES below). A name is read
// across a room, so its edge has to stand 3:1 off the face behind it, the floor for anything that is
// not text. The amber is the warning one: the status amber the table used to letter Reconnecting in
// stood 2.5:1 off the pop-out's light page, which this guard caught on the grid's first draft.
import { afterEach, describe, expect, it } from 'vitest'
import { cleanup, render } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import {
  MODES,
  chainOf,
  contrast,
  expandWith,
  parseRules,
  rgbHex as hex,
  subjectCut,
  toRgb,
  tokenCut,
  tokensAt,
  winnerAt,
  withProps,
  type El,
  type Mode,
  type Rgb,
  type Rule,
} from '../cssCascade'
import { FdClubSection } from './ContestView'
import type { FdClubBoardRow, FdClubStatus } from '../types'

const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES = parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css'))
/** The FIRES control's own rule, planted here and never shipped: a name lettered in the amber the
 *  table letters Reconnecting in. */
const FIXTURE = parseRules('.club-grid-fires-fixture { color: var(--alert-warning); }', { n: RULES.length + 1 })

const TEXT_MIN = 4.5
const EDGE_MIN = 3

const row = (posid: string, posName: string, extra: Partial<FdClubBoardRow>): FdClubBoardRow => ({
  posid,
  posName,
  band: '20m',
  mode: 'CW',
  operator: 'W9ABC',
  qsos: 10,
  rate: 4,
  lastSeenSecs: 1,
  ...extra,
})
/** One of each look a name can have: this laptop, the three links, no link (a position's board and
 *  Remote's), and one with no band yet. */
const HOST: FdClubStatus = {
  syncState: 'synced',
  queued: 0,
  offlineSinceUnix: 0,
  hosting: true,
  event: 'W9ABC Field Day',
  hostCall: 'W9ABC',
  score: 0,
  qsos: 0,
  sections: 0,
  skewSecs: 0,
  dupes: [],
  board: [
    row('aaaa0001', 'Comms Trailer', { band: '20m', mode: 'PH', link: 'host' }),
    row('bbbb0002', 'CW tent', { band: '40m', mode: 'CW', link: 'connected', heardSecs: 3 }),
    row('cccc0003', 'Digital', { band: '40m', mode: 'DIG', link: 'reconnecting', heardSecs: 12 }),
    row('dddd0004', 'GOTA', { band: '6m', mode: 'PH', link: 'lost', heardSecs: 250 }),
    row('eeee0005', 'Rover', { band: '', mode: 'CW', link: 'connected', heardSecs: 1 }),
  ],
}
/** The same board as a position is sent it, with no link words, and three positions the host has not
 *  heard lately: one in a cell, this laptop's own (`OWN`) and the one with no band yet. */
const UNHEARD: Record<string, number> = { Digital: 16, GOTA: 250, Rover: 40 }
const POSITION: FdClubStatus = {
  ...HOST,
  hosting: false,
  board: HOST.board.map(({ link: _l, heardSecs: _h, ...r }) => ({ ...r, lastSeenSecs: UNHEARD[r.posName] ?? r.lastSeenSecs })),
}
const OWN = 'dddd0004'

/** The two faces the board is drawn on: the Contest screen's panel and the pop-out window's. */
const HOSTS = ['panel', 'popout'] as const

/** Every lettered element of the grid, its legend and its no-band line. */
const LETTERED = '.club-grid-name, .club-grid-mark, .club-grid-legend .club-grid-pos, .club-grid-band, .club-grid-mode, .club-grid-waiting-label, .club-grid-fires-fixture'

interface Word {
  what: string
  chain: El[]
}

function wordsOn(host: (typeof HOSTS)[number], club: FdClubStatus, extra?: React.ReactNode, ownPosid?: string): Word[] {
  const board = <FdClubSection club={club} detached={host === 'popout'} ownPosid={ownPosid} />
  const r = render(
    host === 'panel' ? (
      <section className="conversation panel fieldday">
        {board}
        {extra}
      </section>
    ) : (
      <div className="app detached">
        {board}
        {extra}
      </div>
    ),
  )
  const out = [...r.container.querySelectorAll(LETTERED)].map((el) => ({
    what: `${host} ${el.closest('.club-grid-pos') ? [...el.closest('.club-grid-pos')!.classList].join('.') + ' ' : ''}.${[...el.classList].join('.')}`,
    chain: chainOf(el),
  }))
  cleanup()
  return out
}

const chainKey = (chain: El[]) =>
  JSON.stringify(chain.map((e) => [e.tag, e.classes, Object.entries(e.attrs).filter(([k]) => !/^(id|title|aria-label|aria-hidden)$/.test(k))]))
const memo = new Map<string, unknown>()
function once<T>(rules: Rule[], key: string, make: () => T): T {
  const k = `${rules.length}|${key}`
  if (!memo.has(k)) memo.set(k, make())
  return memo.get(k) as T
}
// THE CUTS (2026-10-20). The first case took ~33 s alone, nearly all of it rejecting rules at their subject compound,
// and ran past its 120 s budget in a loaded full run. So winnerAt and tokensAt are handed only the rules that can
// answer: cssCascade's withProps, subjectCut and tokenCut, which say why every winner and token is the same.

/** The winning declaration of `props` on the element or the nearest ancestor that has one. */
function nearest(rules: Rule[], mode: Mode, chain: El[], ...props: string[]) {
  return once(rules, `n|${mode}|${chainKey(chain)}|${props.join()}`, () => {
    for (let i = chain.length; i > 0; i--) {
      const at = chain.slice(0, i)
      const w = winnerAt(subjectCut(withProps(rules, props), at[i - 1]), mode, at, ...props)
      if (w && !/^(inherit|transparent|none)$/.test(w.value.trim())) return { value: w.value, at }
    }
    return null
  })
}
const colourOf = (rules: Rule[], mode: Mode, at: El[], value: string, under: Rgb): Rgb => {
  const tokens = once(rules, `t|${mode}|${chainKey(at)}`, () => tokensAt(tokenCut(rules, mode, at), mode, at))
  const c = toRgb(expandWith(tokens, value), under)
  if (!c) throw new Error(`not a colour: "${value}"`)
  return c
}
const distinct = (words: Word[]) => [...new Map(words.map((w) => [chainKey(w.chain), w])).values()]
/** The face a chain's last element sits on, as the browser paints it: the nearest painted face up
 *  the chain, laid over the face beneath it when it is a tint (a colour mixed with transparent),
 *  down to the window's own, and the document's page colour under all of them. */
function faceOf(rules: Rule[], mode: Mode, chain: El[]): Rgb {
  const face = nearest(rules, mode, chain, 'background', 'background-color')
  const page = () => colourOf(rules, mode, chain.slice(0, 1), 'var(--bg)', [0, 0, 0])
  if (!face) return page()
  const under = face.at.length > 1 ? faceOf(rules, mode, face.at.slice(0, -1)) : page()
  return colourOf(rules, mode, face.at, face.value, under)
}

function unreadable(rules: Rule[], words: Word[], modes: readonly Mode[] = MODES): string[] {
  const out: string[] = []
  for (const w of words)
    for (const mode of modes) {
      const ink = nearest(rules, mode, w.chain, 'color')
      if (!ink) {
        out.push(`${w.what} ${mode}: no ink from the sheet`)
        continue
      }
      const bg = faceOf(rules, mode, w.chain)
      const fg = colourOf(rules, mode, ink.at, ink.value, bg)
      const r = contrast(fg, bg)
      if (r < TEXT_MIN) out.push(`${w.what} ${mode}: ${hex(fg)} on ${hex(bg)} = ${r.toFixed(2)}:1`)
    }
  return out
}

afterEach(() => cleanup())

/** Every box a name or a legend key is drawn in that carries a link colour, one per shape: in a
 *  cell, on the no-band line, and in the legend. */
function linkedChips(host: (typeof HOSTS)[number]): Word[] {
  const out: Word[] = []
  for (const w of wordsOn(host, HOST)) {
    const last = w.chain[w.chain.length - 1]
    const chip = last.classes.includes('club-grid-pos') ? w.chain : last.classes.includes('club-grid-name') ? w.chain.slice(0, -1) : null
    if (chip && chip[chip.length - 1].classes.some((c) => c.startsWith('link-'))) out.push({ what: `${host} ${chip[chip.length - 1].classes.join('.')}`, chain: chip })
  }
  return distinct(out)
}
const linkOf = (w: Word) => w.chain[w.chain.length - 1].classes.find((c) => c.startsWith('link-'))!
const placeOf = (w: Word) =>
  w.chain.some((e) => e.tag === 'td') ? 'cell' : w.chain.some((e) => e.classes.includes('club-grid-waiting')) ? 'no-band line' : 'legend'

describe('the club grid reads in every theme and mode', () => {
  it('every name, mark, key and head clears 4.5:1 on its face, in both windows, on the host’s board and a position’s', () => {
    const words: Word[] = []
    for (const host of HOSTS) words.push(...wordsOn(host, HOST), ...wordsOn(host, POSITION, undefined, OWN))
    // The census cannot silently empty out. On the host's board: 5 names (4 in cells, 1 on the
    // no-band line), 9 marks (theirs and the legend's), 4 legend keys, 10 band heads, the corner and
    // 3 mode heads, the no-band label; on a position's board the names, 6 marks (⚠ after the three
    // not heard, ⌂ before this laptop's, and the legend's two), 2 legend keys, the heads and the label.
    expect(words.length, 'words found').toBe(2 * (5 + 9 + 4 + 10 + 4 + 1) + 2 * (5 + 6 + 2 + 10 + 4 + 1))
    expect(unreadable(RULES, distinct(words))).toEqual([])
  }, 120_000)

  it('each link colour and this laptop’s ride on an edge that stands 3:1 off the window, in every mode', () => {
    const low: string[] = []
    for (const host of HOSTS) {
      const chips = linkedChips(host)
      // Control: every look, in every place a name or a key is drawn.
      expect(new Set(chips.map(linkOf)), `${host}: one of each link`).toEqual(
        new Set(['link-self', 'link-connected', 'link-reconnecting', 'link-lost']),
      )
      expect(new Set(chips.map(placeOf)), `${host}: in a cell, on the line and in the legend`).toEqual(
        new Set(['cell', 'no-band line', 'legend']),
      )
      for (const chip of chips)
        for (const mode of MODES) {
          const edge = winnerAt(RULES, mode, chip.chain, 'border-color')
          if (!edge) {
            low.push(`${chip.what} (${placeOf(chip)}) ${mode}: no edge colour`)
            continue
          }
          const behind = faceOf(RULES, mode, chip.chain.slice(0, -1))
          const fg = colourOf(RULES, mode, chip.chain, edge.value, behind)
          if (contrast(fg, behind) < EDGE_MIN)
            low.push(`${chip.what} (${placeOf(chip)}) ${mode}: ${hex(fg)} on ${hex(behind)} = ${contrast(fg, behind).toFixed(2)}:1`)
        }
    }
    expect(low).toEqual([])
  }, 120_000)

  it('the three links are three different colours, and this laptop a fourth, in every mode', () => {
    const byLink = new Map(linkedChips('panel').filter((c) => placeOf(c) === 'cell').map((c) => [linkOf(c), c]))
    expect(byLink.size, 'control: one name of each look in a cell').toBe(4)
    const same: string[] = []
    for (const mode of MODES) {
      const edges = [...byLink.values()].map((c) =>
        hex(colourOf(RULES, mode, c.chain, winnerAt(RULES, mode, c.chain, 'border-color')!.value, faceOf(RULES, mode, c.chain.slice(0, -1)))),
      )
      if (new Set(edges).size !== 4) same.push(`${mode}: ${[...byLink.keys()].map((k, i) => `${k} ${edges[i]}`).join(', ')}`)
    }
    expect(same).toEqual([])
  }, 120_000)

  it('greys a name the host has not heard lately, and its ⚠: the dim ink, where a fresh name is in the ink, in every mode', () => {
    for (const host of HOSTS) {
      const { container } = render(
        host === 'panel' ? (
          <section className="conversation panel fieldday">
            <FdClubSection club={POSITION} ownPosid={OWN} />
          </section>
        ) : (
          <div className="app detached">
            <FdClubSection club={POSITION} ownPosid={OWN} detached />
          </div>
        ),
      )
      const named = (who: string) => [...container.querySelectorAll('.club-grid-name')].find((n) => n.textContent === who)!
      const after = Object.keys(UNHEARD).map((who) => named(who).nextElementSibling)
      expect(
        after.map((e) => e?.className ?? null),
        `${host}: control, the ⚠ after each name not heard`,
      ).toEqual(Object.keys(UNHEARD).map(() => 'club-grid-mark'))
      const unheard = Object.keys(UNHEARD).flatMap((who, i) => [named(who), after[i]!])
      const fresh = named('CW tent')
      const off: string[] = []
      for (const mode of MODES) {
        const inkOf = (el: Element) => {
          const chain = chainOf(el)
          const ink = nearest(RULES, mode, chain, 'color')
          return ink ? hex(colourOf(RULES, mode, ink.at, ink.value, faceOf(RULES, mode, chain))) : 'no ink'
        }
        const token = (name: string) => hex(colourOf(RULES, mode, chainOf(fresh), `var(${name})`, [0, 0, 0]))
        for (const e of unheard)
          if (inkOf(e) !== token('--text-dim')) off.push(`${host} ${mode}: ${e.parentElement!.className} ${e.className} ${inkOf(e)}, the dim ink ${token('--text-dim')}`)
        if (inkOf(fresh) !== token('--text')) off.push(`${host} ${mode}: a fresh name ${inkOf(fresh)}, the ink ${token('--text')}`)
      }
      expect(off).toEqual([])
      cleanup()
    }
    // …and the grey shows: the standard themes' dim ink is not their ink.
    const chain = chainOf(document.body)
    for (const mode of ['dark', 'light'] as const)
      expect(hex(colourOf(RULES, mode, chain, 'var(--text-dim)', [0, 0, 0])), mode).not.toBe(hex(colourOf(RULES, mode, chain, 'var(--text)', [0, 0, 0])))
  }, 120_000)

  it('FIRES: a name lettered in the amber state colour is caught in the light theme', () => {
    const words = wordsOn('panel', HOST, <span className="club-grid-fires-fixture">Digital</span>)
    const planted = words.filter((w) => w.what.endsWith('.club-grid-fires-fixture'))
    expect(planted, 'control: the planted word is drawn').toHaveLength(1)
    const failing = MODES.filter((mode) => unreadable([...RULES, ...FIXTURE], planted, [mode]).length > 0)
    expect(failing, 'the control must fire in the standard light theme').toContain('light')
    expect(failing, 'and not in the standard dark theme').not.toContain('dark')
  })
})
