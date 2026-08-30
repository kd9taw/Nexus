// @vitest-environment jsdom
//
// THE BAND × MODE GRID, and above all THE DUPE KEY IT PAINTS WITH.
//
// The key is a cross-language contract: this grid decides which cells go red from a key built
// in TypeScript, and the engine decides whether the contact commits from a key built in Rust.
// Neither side is wrong on its own — only the pair is — so the pair gets a test, the way
// wire-consistency.test.ts tests the DTO strings. It reads the two Rust sources that build the
// key and checks each of the three normalisations, and it checks the same normalisations
// behaviourally through `fdDupeKey`, so a drift on either side is a failure here.
//
// The band term is the one worth naming: Rust stamps the log's own band string VERBATIM on
// both sides. A TS-side `toUpperCase()` on the band would make `20M` and `20m` one cell on
// screen and two keys in the engine — a dupe check that silently disagrees with the commit
// that follows it.
import { describe, it, expect, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { FdBandModeGrid, FD_BAND_ORDER, fdDupeKey } from './FdBandModeGrid'
import type { FieldDayQso } from '../types'

const read = (rel: string) => readFileSync(fileURLToPath(new URL(rel, import.meta.url)), 'utf8')
const FIELDDAY_RS = read('../../../crates/tempo-core/src/fieldday.rs')
const FDEVENT_RS = read('../../../crates/tempo-app/src/fdevent.rs')

/** One Rust `pub fn`'s body, by name. Throws when it is gone — a rename must fail loudly
 *  rather than silently emptying this guard. */
function fnBody(src: string, where: string, name: string): string {
  const m = src.match(new RegExp(`pub fn ${name}\\([^)]*\\)[^{]*\\{([\\s\\S]*?)\\n    \\}`))
  if (!m) throw new Error(`pub fn ${name} not found in ${where}`)
  return m[1]
}

/** The three terms of a `(call, band, mode)` tuple. Every term is `x.y()` with EMPTY parens,
 *  so a plain comma split is exact here. */
function keyTerms(body: string): string[] {
  const norm = body.replace(/\s+/g, ' ')
  const at = norm.indexOf('(&(')
  const inner = at >= 0 ? norm.slice(at + 3) : norm.slice(norm.indexOf('(') + 1)
  return inner
    .split(',')
    .map((s) => s.trim())
    .filter((s) => /\w/.test(s))
}

/** What the three terms MUST do: uppercase the call, leave the band alone, uppercase the
 *  mode class. Returns every violation, so a failure names which one drifted. */
function keyFaults(terms: string[]): string[] {
  const faults: string[] = []
  if (terms.length !== 3) faults.push(`not a 3-tuple: ${terms.join(' | ')}`)
  const [call, band, mode] = terms
  if (!/^(self\.)?call\.to_(ascii_)?uppercase\(\)$/.test(call ?? ''))
    faults.push(`call term does not uppercase: ${call}`)
  if (!/^(self\.)?band\.(clone|to_string)\(\)$/.test(band ?? ''))
    faults.push(`band term is not verbatim: ${band}`)
  if (!/^(self\.)?(mode|mode_class)\.to_(ascii_)?uppercase\(\)$/.test(mode ?? ''))
    faults.push(`mode term does not uppercase: ${mode}`)
  return faults
}

describe('the dupe key matches the Rust key it is checked against', () => {
  it('own log — FieldDayLog::is_dupe_mode', () => {
    expect(keyFaults(keyTerms(fnBody(FIELDDAY_RS, 'fieldday.rs', 'is_dupe_mode')))).toEqual([])
  })

  it('own log, explicit band — FieldDayLog::worked_key (what the club sync subtracts with)', () => {
    expect(keyFaults(keyTerms(fnBody(FIELDDAY_RS, 'fieldday.rs', 'worked_key')))).toEqual([])
  })

  it('club log — MergedRow::dupe_key (the keys that arrive on the snapshot)', () => {
    expect(keyFaults(keyTerms(fnBody(FDEVENT_RS, 'fdevent.rs', 'dupe_key')))).toEqual([])
  })

  it('the key WRITTEN at log time stamps the log’s own band and an uppercased mode', () => {
    const body = fnBody(FIELDDAY_RS, 'fieldday.rs', 'log_submode_at').replace(/\s+/g, ' ')
    expect(body, 'the mode class is uppercased before it is keyed').toContain(
      'let mode = mode.to_ascii_uppercase();',
    )
    expect(body, 'the band is the log’s own, verbatim').toContain(
      'self.worked .insert((call.to_uppercase(), self.band.clone(), mode.clone()));',
    )
  })

  it('POSITIVE CONTROL: the checker fires, and a missing function is an error', () => {
    // The same three rules against a tuple that breaks each one.
    expect(keyFaults(['call.clone()', 'band.to_uppercase()', 'mode.clone()'])).toHaveLength(3)
    expect(keyFaults(['call.to_uppercase()', 'band.clone()'])).not.toEqual([])
    expect(() => fnBody(FIELDDAY_RS, 'fieldday.rs', 'no_such_function')).toThrow()
  })
})

describe('fdDupeKey applies exactly those normalisations', () => {
  it('uppercases the call and the mode class, and trims the draft field', () => {
    expect(fdDupeKey('k1abc', '20m', 'ph')).toBe(fdDupeKey(' K1ABC ', '20m', 'PH'))
  })

  it('leaves the band VERBATIM — 20M is not 20m', () => {
    expect(fdDupeKey('K1ABC', '20M', 'PH')).not.toBe(fdDupeKey('K1ABC', '20m', 'PH'))
  })

  it('keeps band and mode apart — the three parts never run together', () => {
    expect(fdDupeKey('K1ABC', '20m', 'PH')).not.toBe(fdDupeKey('K1ABC', '2', '0mPH'))
  })
})

// ── the grid ──────────────────────────────────────────────────────────────────────────

const qso = (call: string, band: string, mode: string): FieldDayQso => ({
  call,
  class: '1A',
  section: 'IL',
  band,
  mode,
})

const cells = () => [...document.querySelectorAll('td')]
const labels = () => cells().map((c) => c.getAttribute('aria-label') ?? '')
const rows = () => [...document.querySelectorAll('tbody th')].map((th) => th.textContent)
/** One cell by its `band|mode` id, and the sentence a hover or a screen reader gets. */
const cell = (id: string) => document.querySelector(`td[data-cell="${id}"]`)
const says = (id: string) => cell(id)?.getAttribute('aria-label') ?? ''

afterEach(cleanup)

describe('the counts', () => {
  it('counts UNIQUE contacts per cell — a repeated key never inflates the number', () => {
    render(
      <FdBandModeGrid
        log={[qso('W1AW', '20m', 'CW'), qso('W1AW', '20m', 'CW'), qso('K1ABC', '20m', 'CW')]}
        band="20m"
        modeClass="CW"
      />,
    )
    expect(cell('20m|CW')?.textContent).toContain('2')
    expect(says('20m|CW')).toContain('2 from this position')
  })

  it('keeps the same call on two bands, and two modes on one band, apart', () => {
    render(
      <FdBandModeGrid
        log={[qso('W1AW', '20m', 'CW'), qso('W1AW', '40m', 'CW'), qso('W1AW', '20m', 'PH')]}
        band="20m"
        modeClass="CW"
      />,
    )
    expect(says('20m|CW')).toContain('1 from this position')
    expect(says('40m|CW')).toContain('1 from this position')
    expect(says('20m|PH')).toContain('1 from this position')
    expect(says('40m|PH')).toContain('0 from this position')
  })

  it('adds the club-only keys into the club total, and never into this position’s count', () => {
    render(
      <FdBandModeGrid
        log={[qso('W1AW', '20m', 'CW')]}
        clubDupes={[
          ['K1ABC', '20m', 'CW'],
          ['K2DEF', '20m', 'CW'],
        ]}
        band="20m"
        modeClass="CW"
      />,
    )
    expect(says('20m|CW')).toContain('1 from this position')
    expect(says('20m|CW')).toContain('3 across the club')
    expect(cell('20m|CW')?.querySelector('sub')?.textContent).toBe('3')
  })

  it('a solo Field Day (no club block) reads own-only and shows no club total', () => {
    render(<FdBandModeGrid log={[qso('W1AW', '20m', 'CW')]} band="20m" modeClass="CW" />)
    expect(says('20m|CW')).toContain('1 from this position')
    expect(says('20m|CW')).not.toContain('club')
    expect(document.querySelector('sub')).toBeNull()
  })

  it('recounts when a contact is appended — the growth key never strands the board', () => {
    const log = [qso('W1AW', '20m', 'CW')]
    const { rerender } = render(<FdBandModeGrid log={log} band="20m" modeClass="CW" />)
    expect(says('20m|CW')).toContain('1 from this position')
    rerender(<FdBandModeGrid log={[...log, qso('K1ABC', '20m', 'CW')]} band="20m" modeClass="CW" />)
    expect(says('20m|CW')).toContain('2 from this position')
  })
})

describe('the rows', () => {
  it('shows the bands worked plus the band the rig is on, in canonical HF→VHF order', () => {
    render(
      <FdBandModeGrid
        log={[qso('W1AW', '20m', 'CW'), qso('K1ABC', '80m', 'PH')]}
        band="40m"
        modeClass="CW"
      />,
    )
    expect(rows()).toEqual(['80m', '40m', '20m'])
  })

  it('marks the cell the rig is actually on, and only that one', () => {
    render(<FdBandModeGrid log={[qso('W1AW', '20m', 'CW')]} band="20m" modeClass="PH" />)
    const marked = cells().filter((c) => c.textContent?.includes('◀'))
    expect(marked).toHaveLength(1)
    expect(marked[0].getAttribute('data-cell')).toBe('20m|PH')
  })

  it('says so when there is nothing to show at all', () => {
    render(<FdBandModeGrid log={[]} band="" modeClass="CW" />)
    expect(document.querySelector('table')).toBeNull()
    expect(document.body.textContent).toContain('No contacts yet')
  })
})

describe('the draft paint — where this callsign is already worked', () => {
  const LOG = [qso('K1ABC', '20m', 'CW'), qso('W1AW', '40m', 'PH')]
  const CLUB: [string, string, string][] = [['K1ABC', '15m', 'PH']]

  it('paints the OWN-log cell as a hard dupe and leaves the open cells alone', () => {
    render(
      <FdBandModeGrid log={LOG} clubDupes={CLUB} band="20m" modeClass="CW" draftCall="k1abc" />,
    )
    const painted = labels().filter((l) => l.includes('K1ABC'))
    expect(painted).toHaveLength(2) // one own, one club — no others
    expect(says('20m|CW')).toContain('already in this log')
    expect(says('40m|PH')).not.toContain('K1ABC')
  })

  it('paints a club-only cell as the softer warning — logging is allowed, it just scores nothing', () => {
    render(
      <FdBandModeGrid log={LOG} clubDupes={CLUB} band="20m" modeClass="CW" draftCall="K1ABC" />,
    )
    expect(says('15m|PH')).toContain('another club position')
    expect(says('15m|PH')).toContain('adds no points')
    expect(says('15m|PH')).not.toContain('already in this log')
  })

  it('paints nothing at all with an empty draft — the board is quiet between contacts', () => {
    render(<FdBandModeGrid log={LOG} clubDupes={CLUB} band="20m" modeClass="CW" />)
    expect(labels().filter((l) => l.includes('K1ABC'))).toEqual([])
  })

  it('agrees with the verdict LogEntry shows for the same draft — cell for cell', () => {
    // LogEntry's shipped own-dupe predicate, verbatim (LogEntry.tsx, the FD variant): the
    // grid's red cells and the entry field's red line must never disagree about one contact.
    const typed = 'K1ABC'
    const logEntryVerdict = (band: string, mode: string) =>
      LOG.some(
        (q) => q.call.toUpperCase() === typed && q.band === band && (q.mode ?? '') === mode,
      )
    render(<FdBandModeGrid log={LOG} clubDupes={[]} band="20m" modeClass="CW" draftCall="k1abc" />)
    expect(cells().length).toBeGreaterThan(3)
    for (const c of cells()) {
      const id = c.getAttribute('data-cell') ?? ''
      const [band, mode] = id.split('|')
      const label = c.getAttribute('aria-label') ?? ''
      expect(label.includes('already in this log'), id).toBe(logEntryVerdict(band, mode))
    }
  })
})

describe('the band order mirrors the one the FD exports print', () => {
  it('is the same list, in the same order, as FieldDayView’s BAND_ORDER', () => {
    const src = read('./FieldDayView.tsx')
    const m = src.match(/const BAND_ORDER = \[([^\]]*)\]/)
    expect(m, 'BAND_ORDER not found in FieldDayView.tsx').not.toBeNull()
    const bands = [...m![1].matchAll(/'([^']+)'/g)].map((x) => x[1])
    expect(bands.length).toBeGreaterThan(10)
    expect(FD_BAND_ORDER).toEqual(bands)
  })
})
