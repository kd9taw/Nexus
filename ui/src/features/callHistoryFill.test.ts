import { describe, expect, it } from 'vitest'
import type { CallHistoryFile, ContestFieldSpec } from '../types'
import { applyFill, EMPTY_FILL, historyFill, typedIn } from './callHistoryFill'

// The slots as the engine really sends them (`FdFieldDto`), per contest.
const ILQP: ContestFieldSpec[] = [
  { key: 'RST', kind: 'rst', required: true, adif: 'RST_RCVD' },
  { key: 'QTH', kind: 'oneOf', required: true, domains: ['il_counties', 'il_mults'] },
]
const FD: ContestFieldSpec[] = [
  { key: 'CLASS', kind: 'pattern', required: true, adif: 'CLASS' },
  { key: 'SECTION', kind: 'enum', required: true, domain: 'fd_sections', domains: ['fd_sections'], adif: 'ARRL_SECT' },
]
const CQWW: ContestFieldSpec[] = [
  { key: 'RST', kind: 'rst', required: true, adif: 'RST_RCVD' },
  { key: 'ZN', kind: 'number', required: true, min: 1, max: 40, adif: 'CQZ' },
]
const SS: ContestFieldSpec[] = [
  { key: 'NR', kind: 'serial', required: true, adif: 'SRX' },
  { key: 'PREC', kind: 'pattern', required: true },
  { key: 'CK', kind: 'pattern', required: true },
  // Sweepstakes' own section list is not one the UI holds: the strip cannot check it.
  { key: 'SEC', kind: 'enum', required: true, domain: 'ss_sections', domains: ['ss_sections'], adif: 'ARRL_SECT' },
]

const file = (contest: string, entries: CallHistoryFile['entries']): CallHistoryFile => ({
  contest,
  fileName: 'history.txt',
  entries,
})

describe('what call history may fill', () => {
  it('fills only a code the contest accepts, a name becoming its code', () => {
    const h = file('ilqp', {
      K9AAA: { Loc1: 'COOK', State: 'IL' },
      K9BBB: { Loc1: 'St. Clair' },
      W9CCC: { State: 'WI' },
    })
    expect(historyFill('K9AAA', ILQP, h, 'ilqp')).toEqual({ QTH: 'COOK' })
    expect(historyFill('K9BBB', ILQP, h, 'ilqp')).toEqual({ QTH: 'SCLA' })
    expect(historyFill('w9ccc', ILQP, h, 'ilqp')).toEqual({ QTH: 'WI' })
  })

  it('leaves out a value the strip would refuse, and falls back only to another real code', () => {
    const h = file('ilqp', {
      K9AAA: { Loc1: 'CHICAGO' }, // a city, not a county
      K9BBB: { Loc1: 'WHTSD', State: 'IL' }, // not the sponsor's spelling; IL is no QTH either
      K9CCC: { Loc1: 'COOK/DUPG' }, // a county line is a contact, not a station
      K9DDD: { Loc1: 'NOWHERE', State: 'WI' },
    })
    expect(historyFill('K9AAA', ILQP, h, 'ilqp')).toEqual({})
    expect(historyFill('K9BBB', ILQP, h, 'ilqp')).toEqual({})
    expect(historyFill('K9CCC', ILQP, h, 'ilqp')).toEqual({})
    expect(historyFill('K9DDD', ILQP, h, 'ilqp')).toEqual({ QTH: 'WI' })
  })

  it('fills a section and a zone, and never a class, a check, a report or a serial', () => {
    const fd = file('arrlfd', { W1AW: { Exch1: '2A', Sect: 'CT' }, K1ZZ: { Sect: 'XYZ' } })
    expect(historyFill('W1AW', FD, fd, 'arrlfd')).toEqual({ SECTION: 'CT' })
    expect(historyFill('K1ZZ', FD, fd, 'arrlfd')).toEqual({})
    const ww = file('cqww_cw', { DL1ABC: { CqZone: '14' }, JA1XYZ: { CqZone: '41' } })
    expect(historyFill('DL1ABC', CQWW, ww, 'cqww_cw')).toEqual({ ZN: '14' })
    expect(historyFill('JA1XYZ', CQWW, ww, 'cqww_cw')).toEqual({})
    const ss = file('arrlss_cw', { K5ZD: { CK: '71', Sect: 'NH', Exch1: 'A' } })
    expect(historyFill('K5ZD', SS, ss, 'arrlss_cw')).toEqual({})
  })

  it('uses a file only for the contest it was imported for, and never for a station on the move', () => {
    const h = file('ilqp', { K9AAA: { Loc1: 'COOK' }, 'K9AAA/M': { Loc1: 'COOK' } })
    expect(historyFill('K9AAA', ILQP, h, 'wfd')).toEqual({})
    expect(historyFill('K9AAA', ILQP, null, 'ilqp')).toEqual({})
    expect(historyFill('K9AAA/M', ILQP, h, 'ilqp')).toEqual({})
  })
})

describe('how a fill goes in and comes back out', () => {
  it('fills an untouched box and remembers what it held', () => {
    const r = applyFill(EMPTY_FILL, 'K9AAA', { QTH: 'COOK' }, { RST: '599', QTH: '' })
    expect(r.writes).toEqual({ QTH: 'COOK' })
    expect(r.state.filled).toEqual({ QTH: { call: 'K9AAA', value: 'COOK', before: '' } })
  })

  it('never writes over what the operator typed', () => {
    const typed = typedIn(EMPTY_FILL, 'QTH')
    const r = applyFill(typed, 'K9AAA', { QTH: 'COOK' }, { QTH: 'LAKE' })
    expect(r.writes).toEqual({})
    expect(r.state.filled).toEqual({})
    // Typing over a fill makes the box the operator's: the mark goes, and it stays theirs.
    const filled = applyFill(EMPTY_FILL, 'K9AAA', { QTH: 'COOK' }, { QTH: '' }).state
    const over = typedIn(filled, 'QTH')
    expect(over.filled).toEqual({})
    expect(applyFill(over, 'K9AAA', { QTH: 'COOK' }, { QTH: 'LAKE' }).writes).toEqual({})
  })

  it('takes a fill back out when the call changes, restoring what the box held', () => {
    const first = applyFill(EMPTY_FILL, 'K9AA', { QTH: 'COOK' }, { QTH: 'LAKE' })
    // The operator keeps typing: K9AA becomes K9AAA, which the file does not hold.
    const r = applyFill(first.state, 'K9AAA', {}, { QTH: 'COOK' })
    expect(r.writes).toEqual({ QTH: 'LAKE' })
    expect(r.state.filled).toEqual({})
    // …or holds with another county, which replaces it, still marked.
    const s = applyFill(first.state, 'K9AAA', { QTH: 'DUPG' }, { QTH: 'COOK' })
    expect(s.writes).toEqual({ QTH: 'DUPG' })
    expect(s.state.filled).toEqual({ QTH: { call: 'K9AAA', value: 'DUPG', before: 'LAKE' } })
  })

  it('changes nothing when nothing moved', () => {
    const first = applyFill(EMPTY_FILL, 'K9AAA', { QTH: 'COOK' }, { QTH: '' })
    const again = applyFill(first.state, 'K9AAA', { QTH: 'COOK' }, { QTH: 'COOK' })
    expect(again.writes).toEqual({})
    expect(again.state).toBe(first.state)
  })
})
