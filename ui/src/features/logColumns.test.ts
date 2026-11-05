// The Logbook's column model: what the table shows, how wide, what is saved and how a saved
// choice is made safe on the window it is read on. The templates below are the ones the stylesheet
// held literally before the model, copied from it, so "the defaults are today's table" is checked
// against the table as it was and not against the model's own idea of it.
import { describe, expect, it } from 'vitest'
import {
  ACTIONS_TRACK,
  addedColumnText,
  clampLogColumns,
  DEFAULT_LOG_COLUMNS,
  isWideTable,
  legacyLogColumns,
  LOG_COLUMNS,
  logGridTemplate,
  logTableMinWidth,
  parkNameText,
  renderedColumns,
  uploadMark,
  uploadWord,
  visibleColumns,
  withColumnMoved,
  withColumnShown,
  withColumnStep,
  withColumnWidth,
  type LogColumnsSaved,
} from './logColumns'
import type { LoggedQso } from '../types'
import { t } from '../i18n'

/** `.logbook-row`'s template before the model (styles.css, 59aeae99e). */
const OLD_COMPACT = 'minmax(96px, 1fr) 1.3fr 0.7fr 1fr 0.7fr 0.6fr 0.6fr minmax(84px, 1fr) minmax(64px, 0.6fr) 0.9fr 0.7fr 1.4fr minmax(80px, 80px)'
/** `[data-viewport='sm'] .logbook-row`'s. */
const OLD_NARROW = 'minmax(96px, 1fr) 1.3fr 0.7fr 0.7fr 1fr minmax(80px, 80px)'
/** `.logbook-table.wide .logbook-row`'s ("More columns"). */
const OLD_WIDE =
  'minmax(96px, 1fr) 1.3fr 0.7fr 1fr 0.7fr 0.6fr 0.6fr minmax(84px, 1fr) minmax(64px, 0.6fr) 0.9fr 0.7fr 0.9fr 0.8fr 1.1fr 1fr 1fr 0.5fr 0.6fr 0.8fr 1.4fr minmax(80px, 80px)'

const template = (saved: LogColumnsSaved, narrow = false) =>
  logGridTemplate(renderedColumns(saved, narrow), saved, narrow)
/** A template's tracks: split on the spaces outside `minmax(…)`. */
const tracksOf = (tpl: string) => tpl.split(/ (?![^(]*\))/)
const ids = (saved: LogColumnsSaved, narrow = false) => renderedColumns(saved, narrow).map((c) => c.id)

describe('the defaults are the table the Logbook had', () => {
  it('nothing saved: the compact template, track for track', () => {
    expect(template(DEFAULT_LOG_COLUMNS)).toBe(OLD_COMPACT)
    expect(logTableMinWidth(renderedColumns(DEFAULT_LOG_COLUMNS, false), DEFAULT_LOG_COLUMNS)).toBeNull()
  })

  it('at sm/xs: the six columns and the template the narrow rules kept', () => {
    expect(ids(DEFAULT_LOG_COLUMNS, true)).toEqual(['call', 'country', 'band', 'qsl', 'notes', 'actions'])
    expect(template(DEFAULT_LOG_COLUMNS, true)).toBe(OLD_NARROW)
  })

  it('"More columns" on in this browser: the wide table, at the width it always had', () => {
    const more = legacyLogColumns(true)
    expect(template(more)).toBe(OLD_WIDE)
    // …and at a narrow window it still shows every column, scrolling sideways.
    expect(template(more, true)).toBe(OLD_WIDE)
    expect(logTableMinWidth(renderedColumns(more, false), more)).toBe('calc(117.53rem + 80px)')
    expect(legacyLogColumns(false)).toEqual(DEFAULT_LOG_COLUMNS)
  })

  it('the action cluster keeps its measured floor and stays last', () => {
    for (const saved of [DEFAULT_LOG_COLUMNS, legacyLogColumns(true)]) {
      for (const narrow of [false, true]) {
        const cols = renderedColumns(saved, narrow)
        expect(cols[cols.length - 1].id).toBe('actions')
        expect(logGridTemplate(cols, saved, narrow).endsWith(ACTIONS_TRACK)).toBe(true)
      }
    }
  })
})

describe('show and hide', () => {
  it('turns an added column on and off', () => {
    const on = withColumnShown(DEFAULT_LOG_COLUMNS, 'iota', true)
    expect(on.shown).toEqual(['iota'])
    expect(ids(on)).toContain('iota')
    // An added column makes a wide table: kept at every width, and the notes/actions stay last.
    expect(isWideTable(on)).toBe(true)
    expect(ids(on, true)).toContain('freq')
    expect(ids(on).slice(-3)).toEqual(['iota', 'notes', 'actions'])
    const off = withColumnShown(on, 'iota', false)
    expect(off.shown).toEqual([])
    expect(ids(off)).toEqual(ids(DEFAULT_LOG_COLUMNS))
  })

  it('turns a default column off and back on', () => {
    const off = withColumnShown(DEFAULT_LOG_COLUMNS, 'freq', false)
    expect(off.hidden).toEqual(['freq'])
    expect(ids(off)).not.toContain('freq')
    // One track fewer, and still one per column shown.
    expect(tracksOf(template(off))).toHaveLength(ids(off).length)
    expect(withColumnShown(off, 'freq', true)).toEqual(DEFAULT_LOG_COLUMNS)
  })

  it('never hides the callsign or the controls', () => {
    expect(withColumnShown(DEFAULT_LOG_COLUMNS, 'call', false)).toBe(DEFAULT_LOG_COLUMNS)
    expect(withColumnShown(DEFAULT_LOG_COLUMNS, 'actions', false)).toBe(DEFAULT_LOG_COLUMNS)
    // …and a saved file that says otherwise is read without it.
    expect(visibleColumns(clampLogColumns({ hidden: ['call', 'actions'] }, null)).map((c) => c.id)).toContain('call')
  })
})

describe('resize', () => {
  it('a dragged width replaces the share with px, and null puts the share back', () => {
    const wide = withColumnWidth(DEFAULT_LOG_COLUMNS, 'country', 210, 1366)
    expect(wide.widths).toEqual({ country: 210 })
    expect(tracksOf(template(wide))[1]).toBe('210px')
    expect(withColumnWidth(wide, 'country', null, 1366).widths).toEqual({})
  })

  it('is held between the column floor and half the window', () => {
    expect(withColumnWidth(DEFAULT_LOG_COLUMNS, 'country', 5, 1366).widths.country).toBe(48)
    expect(withColumnWidth(DEFAULT_LOG_COLUMNS, 'country', 4000, 1366).widths.country).toBe(683)
  })

  it('the action cluster has no width to set', () => {
    expect(withColumnWidth(DEFAULT_LOG_COLUMNS, 'actions', 100, 1366)).toBe(DEFAULT_LOG_COLUMNS)
  })

  it('a wide table grows by the px it was given', () => {
    const saved = withColumnWidth(withColumnShown(DEFAULT_LOG_COLUMNS, 'county', true), 'county', 150, 1920)
    expect(logTableMinWidth(renderedColumns(saved, false), saved)).toMatch(/^calc\([\d.]+rem \+ 230px\)$/)
  })
})

describe('a saved choice is clamped on load', () => {
  it('a width saved on a 4K monitor is cut to a share of the laptop it is read on', () => {
    const saved = { shown: ['county'], hidden: [], widths: { county: 1800, notes: 2400 } }
    expect(clampLogColumns(saved, 1366).widths).toEqual({ county: 683, notes: 683 })
    // …and the same file on the 4K monitor is as it was saved.
    expect(clampLogColumns(saved, 3840).widths).toEqual({ county: 1800, notes: 1920 })
  })

  it('reads anything that is not a choice as nothing saved, and drops what it cannot place', () => {
    expect(clampLogColumns(null, 1366)).toEqual(DEFAULT_LOG_COLUMNS)
    expect(clampLogColumns('wide', 1366)).toEqual(DEFAULT_LOG_COLUMNS)
    expect(
      clampLogColumns(
        {
          shown: ['iota', 'nope', 'freq', 7, 'iota'],
          hidden: ['freq', 'iota'],
          widths: { iota: 'big', grid: Number.NaN, nope: 90, actions: 900, cqz: 2 },
        },
        1366,
      ),
    ).toEqual({ shown: ['iota'], hidden: ['freq'], widths: { cqz: 28 } })
  })

  it('round-trips through JSON as settings.json keeps it', () => {
    const saved = withColumnWidth(withColumnShown(withColumnShown(DEFAULT_LOG_COLUMNS, 'grid', true), 'mode', false), 'grid', 90, 1366)
    expect(clampLogColumns(JSON.parse(JSON.stringify(saved)), 1366)).toEqual(saved)
  })
})

describe('order', () => {
  it('a dragged column takes its new place, and the template and the row follow it', () => {
    const moved = withColumnMoved(DEFAULT_LOG_COLUMNS, 'notes', 'call', 'after')
    expect(moved.order).toBeDefined()
    expect(ids(moved)).toEqual(['call', 'notes', 'country', 'band', 'freq', 'mode', 'sent', 'rcvd', 'date', 'time', 'park', 'qsl', 'actions'])
    // The template is cut from the same list: Notes' share follows Notes.
    expect(tracksOf(template(moved)).slice(0, 3)).toEqual(['minmax(96px, 1fr)', '1.4fr', '1.3fr'])
    // Before the first column, and back.
    expect(ids(withColumnMoved(moved, 'qsl', 'call', 'before')).slice(0, 2)).toEqual(['qsl', 'call'])
  })

  it('the action cluster stays last, whatever is dropped on it, and never moves itself', () => {
    const toEnd = withColumnMoved(DEFAULT_LOG_COLUMNS, 'call', 'actions', 'after')
    expect(ids(toEnd).slice(-2)).toEqual(['call', 'actions'])
    expect(withColumnMoved(DEFAULT_LOG_COLUMNS, 'actions', 'call', 'before')).toBe(DEFAULT_LOG_COLUMNS)
  })

  it('a step moves a column past its rendered neighbour, and stops at either end', () => {
    const cols = renderedColumns(DEFAULT_LOG_COLUMNS, false)
    const right = withColumnStep(DEFAULT_LOG_COLUMNS, cols, 'country', 1)
    expect(ids(right).slice(0, 3)).toEqual(['call', 'band', 'country'])
    expect(withColumnStep(DEFAULT_LOG_COLUMNS, cols, 'call', -1)).toBe(DEFAULT_LOG_COLUMNS)
    // The last data column cannot step past the action cluster.
    expect(withColumnStep(DEFAULT_LOG_COLUMNS, cols, 'notes', 1)).toBe(DEFAULT_LOG_COLUMNS)
    // At a narrow window the neighbour is the one on screen: Country past Band, then past QSL.
    const narrow = renderedColumns(DEFAULT_LOG_COLUMNS, true)
    const once = withColumnStep(DEFAULT_LOG_COLUMNS, narrow, 'band', 1)
    expect(ids(once, true)).toEqual(['call', 'country', 'qsl', 'band', 'notes', 'actions'])
  })

  it('moving a column back to where it began saves no order at all', () => {
    const there = withColumnMoved(DEFAULT_LOG_COLUMNS, 'band', 'call', 'before')
    const back = withColumnMoved(there, 'band', 'country', 'after')
    expect(back).toEqual(DEFAULT_LOG_COLUMNS)
    expect('order' in back).toBe(false)
  })

  it('a column turned on later shows beside its default neighbour in the moved table', () => {
    // Notes moved to the front, then IOTA turned on: IOTA sits after Grid's default neighbour
    // chain (it follows Grid by default), so after the last default column before it.
    const moved = withColumnMoved(DEFAULT_LOG_COLUMNS, 'notes', 'call', 'before')
    const withIota = withColumnShown(moved, 'iota', true)
    expect(ids(withIota)).toEqual(['notes', 'call', 'country', 'band', 'freq', 'mode', 'sent', 'rcvd', 'date', 'time', 'park', 'qsl', 'iota', 'actions'])
  })

  it('a saved order is clamped on load: unknown and repeated ids dropped, the actions ignored, the rest placed', () => {
    const got = clampLogColumns({ shown: [], hidden: [], widths: {}, order: ['notes', 'nope', 7, 'notes', 'actions', 'call'] }, 1366)
    expect(ids(got).slice(0, 3)).toEqual(['notes', 'call', 'country'])
    expect(ids(got).slice(-1)).toEqual(['actions'])
    // An order that is not a list, or is the default one, is no order.
    expect(clampLogColumns({ shown: [], hidden: [], widths: {}, order: 'notes' }, 1366)).toEqual(DEFAULT_LOG_COLUMNS)
    expect(clampLogColumns({ shown: [], hidden: [], widths: {}, order: LOG_COLUMNS.map((c) => c.id) }, 1366)).toEqual(DEFAULT_LOG_COLUMNS)
  })

  it('an old saved choice (no order) loads exactly as it did', () => {
    const old = { shown: ['iota'], hidden: ['freq'], widths: { country: 210 } }
    const got = clampLogColumns(old, 1366)
    expect(got).toStrictEqual(old)
    expect(ids(got)).toEqual(visibleColumns(old).map((c) => c.id))
    expect(ids(got)).toEqual(LOG_COLUMNS.filter((c) => c.fixed || (c.base ? c.id !== 'freq' : c.id === 'iota')).map((c) => c.id))
  })

  it('round-trips an order through JSON as settings.json keeps it', () => {
    const saved = withColumnMoved(withColumnShown(DEFAULT_LOG_COLUMNS, 'grid', true), 'grid', 'call', 'after')
    expect(clampLogColumns(JSON.parse(JSON.stringify(saved)), 1366)).toEqual(saved)
  })
})

describe('the values the added columns print', () => {
  const q = {
    call: 'DL1ABC',
    grid: 'JO31',
    band: '20m',
    freqMhz: 14.074,
    mode: 'CW',
    rstSent: '599',
    rstRcvd: '599',
    whenUnix: 1_758_000_000,
    confirmed: false,
    awardConfirmed: false,
    ota: { iota: 'EU-005' },
    qslSent: { sent: true, via: 'B', dateUnix: 1_758_100_000 },
    upload: { lotw: { outcome: 'accepted', whenUnix: 1 }, qrz: { outcome: 'rejected', whenUnix: 2 } },
    contest: { contestId: 'CQ-WW-CW', stx: 12, srx: 345, sent: [['RST', '599'], ['ZONE', '4']], rcvd: [] },
    extra: [
      ['CNTY', 'MA,MIDDLESEX'],
      ['CQZ', '14'],
      ['ITUZ', '28'],
      ['CONT', 'EU'],
      ['DISTANCE', '6512.4'],
      ['QSLRDATE', '20250102'],
      ['LOTW_QSLRDATE', '20250203'],
    ],
  } as unknown as LoggedQso

  it('reads each from where the record keeps it', () => {
    const got = Object.fromEntries(
      LOG_COLUMNS.filter((c) => !c.base && !c.more && c.group !== 'uploads').map((c) => [c.id, addedColumnText(q, c.id)]),
    )
    expect(got).toEqual({
      grid: 'JO31',
      iota: 'EU-005',
      county: 'MA,MIDDLESEX',
      cqz: '14',
      ituz: '28',
      cont: 'EU',
      distance: '6512',
      qslSentDate: '2025-09-17',
      qslRcvdDate: '2025-01-02',
      lotwRcvdDate: '2025-02-03',
      eqslRcvdDate: null,
      contest: 'CQ-WW-CW',
      exchSent: '599 4',
      // No exchange logged on this side: the serial.
      exchRcvd: '345',
      // The path is not printed from the record: its cell asks the shack for the grade
      // (components/prop/PathGradeCell.tsx, features/pathGrade).
      path: null,
      // Not a satellite contact.
      satName: null,
      propMode: null,
      // Nor is the park's name: its cell reads it from the station's park list (parkNameText).
      parkName: null,
    })
  })

  it('a distance that is not a number, a 0x one among them, prints as it came', () => {
    const at = (v: string) => addedColumnText({ ...q, extra: [['DISTANCE', v]] } as LoggedQso, 'distance')
    expect(at('approx 500')).toBe('approx 500')
    expect(at('0x10')).toBe('0x10')
    expect(at('0b11')).toBe('0b11')
    expect(at('  9.6 ')).toBe('10')
  })

  it('marks each upload stamp', () => {
    expect(uploadMark(q.upload?.lotw)).toBe('done')
    expect(uploadMark(q.upload?.qrz)).toBe('failed')
    expect(uploadMark({ outcome: 'pending', whenUnix: 1 })).toBe('waiting')
    expect(uploadMark(undefined)).toBe('none')
  })

  it('an ordinary contact has no contest values', () => {
    const plain = { ...q, contest: undefined, extra: [] } as LoggedQso
    expect(addedColumnText(plain, 'contest')).toBeNull()
    expect(addedColumnText(plain, 'exchSent')).toBeNull()
    expect(addedColumnText(plain, 'county')).toBeNull()
  })
})

describe('the satellite columns (#423) and the park name (#437)', () => {
  const ADDED = ['satName', 'propMode', 'parkName']

  it('are in the chooser, off by default, and each sorts', () => {
    for (const id of ADDED) {
      const c = LOG_COLUMNS.find((x) => x.id === id)
      expect(c, id).toBeDefined()
      expect(c?.base || c?.more || c?.fixed, id).toBeFalsy()
      expect(c?.sort, id).toBe(id)
    }
    for (const id of ADDED) expect(ids(DEFAULT_LOG_COLUMNS)).not.toContain(id)
    expect(ids(legacyLogColumns(true)).filter((id) => ADDED.includes(id))).toEqual([])
  })

  it('an old saved choice, an old saved order among them, loads without them, and each turns on', () => {
    const old = { shown: ['iota'], hidden: ['freq'], widths: { country: 210 } }
    const got = clampLogColumns(old, 1366)
    expect(got).toStrictEqual(old)
    for (const id of ADDED) expect(ids(got)).not.toContain(id)
    const oldOrder = clampLogColumns({ ...old, order: ['notes', 'call', 'country'] }, 1366)
    for (const id of ADDED) expect(ids(oldOrder)).not.toContain(id)
    let on = got
    for (const id of ADDED) on = withColumnShown(on, id, true)
    expect(on.shown).toEqual(['iota', ...ADDED])
    expect(ids(on)).toEqual(expect.arrayContaining(ADDED))
    expect(tracksOf(template(on))).toHaveLength(ids(on).length)
    // …and back off, to the choice it was.
    let off = on
    for (const id of ADDED) off = withColumnShown(off, id, false)
    expect(off).toStrictEqual(old)
  })

  it('print the satellite and the propagation mode as the contact keeps them', () => {
    const sat = { call: 'RL1F', ota: {}, satName: ' RS-44 ', propMode: 'SAT' } as unknown as LoggedQso
    expect(addedColumnText(sat, 'satName')).toBe('RS-44')
    expect(addedColumnText(sat, 'propMode')).toBe('SAT')
    const plain = { call: 'RL1F', satName: '  ', propMode: null } as unknown as LoggedQso
    expect(addedColumnText(plain, 'satName')).toBeNull()
    expect(addedColumnText(plain, 'propMode')).toBeNull()
  })

  describe('the park name, read from the park list', () => {
    const LIST: Record<string, string> = { 'US-0001': 'Acadia National Park', 'US-1234': 'Lake Park', 'K-0005': 'Old Style Park' }
    const nameOf = (ref: string) => LIST[ref] ?? null
    const at = (ota: LoggedQso['ota']) => parkNameText({ call: 'K1ABC', ota } as unknown as LoggedQso, nameOf)

    it('names the park the Park column shows: theirs, else mine', () => {
      expect(at({ theirProgram: 'POTA', theirRef: 'US-0001' })).toBe('Acadia National Park')
      expect(at({ myProgram: 'POTA', myRef: 'US-1234' })).toBe('Lake Park')
      // Theirs first, as the Park column prints it.
      expect(at({ theirProgram: 'POTA', theirRef: 'US-0001', myProgram: 'POTA', myRef: 'US-1234' })).toBe('Acadia National Park')
      // No program recorded reads as POTA, as the Park column's hover does; case and spaces do not matter.
      expect(at({ theirRef: ' us-0001 ' })).toBe('Acadia National Park')
    })

    it('names every park of a two-fer it knows, in the order logged', () => {
      expect(at({ theirProgram: 'POTA', theirRef: 'US-1234,US-0001' })).toBe('Lake Park / Acadia National Park')
      expect(at({ theirProgram: 'POTA', theirRef: 'US-1234; US-9999 ;US-1234' })).toBe('Lake Park')
    })

    it('is empty with no reference, an unknown one, or a summit', () => {
      expect(at(undefined)).toBeNull()
      expect(at({})).toBeNull()
      expect(at({ theirProgram: 'POTA', theirRef: 'US-9999' })).toBeNull()
      // SOTA has no name list: a summit is never looked up, even one the list happens to hold.
      expect(at({ theirProgram: 'SOTA', theirRef: 'K-0005' })).toBeNull()
      expect(at({ theirProgram: 'SOTA', theirRef: 'W7A/MN-001', myProgram: 'POTA', myRef: 'US-1234' })).toBeNull()
    })
  })
})

// #163: a satellite contact the one-click LoTW fix corrected and has not sent yet is its own state —
// "Corrected, not sent yet" — never a refused upload, and it sorts with the contacts not uploaded.
describe('a corrected contact not sent yet', () => {
  it('is its own mark, not a failure', () => {
    expect(uploadMark({ outcome: 'rejected', whenUnix: 1, detail: 'resend' })).toBe('resend')
    expect(uploadWord('resend')).toBe(t('logbook.upload.resend'))
    expect(uploadMark({ outcome: 'rejected', whenUnix: 1, detail: 'unclassified' })).toBe('failed')
    expect(uploadMark({ outcome: 'rejected', whenUnix: 1 })).toBe('failed')
  })
})
