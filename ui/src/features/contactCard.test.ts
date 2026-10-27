import { describe, expect, it } from 'vitest'
import { cardDirty, cardSave, clearedByCallFix, draftOf, fieldChanges, fieldsOf, ownStationNotResent, storedEdit, typedLines, type CardDraft, type CardField } from './contactCard'
import type { LoggedQso } from '../types'

const Q: LoggedQso = {
  id: 'a', call: 'W1AW', grid: null, band: '20m', freqMhz: 14.0251, mode: 'CW', rstSent: '599', rstRcvd: '579',
  whenUnix: Date.UTC(2026, 9, 26, 23, 58, 0) / 1000, confirmed: false, awardConfirmed: false,
  ota: { theirProgram: 'POTA', theirRef: 'US-0001', myProgram: null, myRef: null, iota: null },
  extra: [['CNTY', 'CT,HARTFORD']],
}
const saved = (over: Partial<CardDraft>, q: LoggedQso = Q) => {
  const start = draftOf(q)
  return cardSave(q, start, { ...start, ...over }, fieldsOf(q))
}

describe('the contact card’s save', () => {
  it('left alone, is the stored contact exactly, with no field change', () => {
    expect(saved({})).toEqual({ ok: true, edit: storedEdit(Q), fields: [] })
  })

  it('an end time earlier in the day than the start is the next day (the rollover)', () => {
    const r = saved({ timeOff: '00:03' })
    expect(r.ok && r.edit.timeOffUnix).toBe(Q.whenUnix + 5 * 60)
  })

  it('a date-only contact keeps its instant when only another box changes', () => {
    const q = { ...Q, timeKnown: false, whenUnix: Date.UTC(2020, 0, 2) / 1000 }
    expect(draftOf(q).timeOn).toBe('')
    const r = saved({ name: 'Hiram' }, q)
    expect(r.ok && r.edit.whenUnix).toBe(q.whenUnix)
  })

  it('a changed programme rides with the stored reference', () => {
    const r = saved({ theirProgram: 'WWFF' })
    expect(r.ok && r.edit.ota).toEqual({ theirProgram: 'WWFF', theirRef: 'US-0001', myProgram: null, myRef: null, iota: null })
  })

  it('emptying both park boxes is refused: an empty park is "leave it" to the log', () => {
    expect(saved({ theirRef: '' })).toEqual({ ok: false, error: { code: 'cannotClear', field: 'parks' } })
  })

  it('a frequency or a time that is not one is refused, never guessed', () => {
    expect(saved({ freq: '14.0x' })).toEqual({ ok: false, error: { code: 'freq' } })
    expect(saved({ timeOn: '25:00' })).toEqual({ ok: false, error: { code: 'when' } })
  })

  it('an added field the contact already holds is refused', () => {
    const start = draftOf(Q)
    const fields = [...fieldsOf(Q), { tag: 'cnty', orig: null, value: 'x', removed: false }]
    expect(cardSave(Q, start, start, fields)).toEqual({ ok: false, error: { code: 'twice', tag: 'CNTY' } })
  })
})

describe('fields kept as written: only what the operator touched is sent', () => {
  // Imported with values that are empty or blank: the ADIF reader keeps `<QSLMSG:0>` as written.
  const IMPORTED: LoggedQso = { ...Q, extra: [['APP_NEXUS_X', ''], ['APP_X_NOTE', ' '], ['CNTY', 'CT,HARTFORD'], ['QSLMSG', '']] }

  it('an untouched empty or blank field is not removed, and the card does not open changed', () => {
    const start = draftOf(IMPORTED)
    const fields = fieldsOf(IMPORTED)
    expect(fieldChanges(fields)).toEqual([])
    expect(cardDirty(start, start, fields)).toBe(false)
    expect(cardSave(IMPORTED, start, { ...start, name: 'Hiram' }, fields)).toEqual({
      ok: true,
      edit: { ...storedEdit(IMPORTED), name: 'Hiram' },
      fields: [],
    })
  })

  it('a field the operator emptied or removed goes as a removal', () => {
    const fields = fieldsOf(IMPORTED).map((f) =>
      f.tag === 'CNTY' ? { ...f, value: '  ' } : f.tag === 'QSLMSG' ? { ...f, removed: true } : f,
    )
    expect(fieldChanges(fields)).toEqual([
      { tag: 'CNTY', old: 'CT,HARTFORD', new: null },
      { tag: 'QSLMSG', old: '', new: null },
    ])
  })

  it('a typed value is trimmed, and one that trims to what it held is no change', () => {
    const fields: CardField[] = [
      { tag: 'CNTY', orig: 'CT,HARTFORD', value: ' CT,TOLLAND ', removed: false },
      { tag: 'CQZ', orig: '5', value: '5 ', removed: false },
      { tag: 'ITUZ', orig: null, value: ' 8\t', removed: false },
    ]
    expect(fieldChanges(fields)).toEqual([
      { tag: 'CNTY', old: 'CT,HARTFORD', new: 'CT,TOLLAND' },
      { tag: 'ITUZ', old: null, new: '8' },
    ])
  })
})

describe('a merged contest contact', () => {
  // The exchange holds the county and zone it was worked in; the record still carries a stale CNTY.
  const C: LoggedQso = { ...Q, contest: { contestId: 'OH-QSO-PARTY', adif: [['CNTY', 'OH,FRANKLIN'], ['CQZ', '5']] } }

  it('a field its contest exchange holds is refused, changed or added, and says why', () => {
    const start = draftOf(C)
    const changed = fieldsOf(C).map((f) => (f.tag === 'CNTY' ? { ...f, value: 'OH,DELAWARE' } : f))
    expect(cardSave(C, start, start, changed)).toEqual({ ok: false, error: { code: 'contest', tag: 'CNTY' } })
    const added = [...fieldsOf(C), { tag: 'CQZ', orig: null, value: '4', removed: false }]
    expect(cardSave(C, start, start, added)).toEqual({ ok: false, error: { code: 'contest', tag: 'CQZ' } })
    const other = [...fieldsOf(C), { tag: 'ITUZ', orig: null, value: '8', removed: false }]
    expect(cardSave(C, start, start, other)).toEqual({ ok: true, edit: storedEdit(C), fields: [{ tag: 'ITUZ', old: null, new: '8' }] })
  })

  it('a column holding only blanks holds the field too; an empty one does not', () => {
    // The ADIF writer writes a blank column (in place of a field beside it) and skips an empty one.
    const B: LoggedQso = { ...Q, contest: { contestId: 'OH-QSO-PARTY', adif: [['CQZ', ' '], ['ITUZ', '']] } }
    const start = draftOf(B)
    const blank = [...fieldsOf(B), { tag: 'CQZ', orig: null, value: '4', removed: false }]
    expect(cardSave(B, start, start, blank)).toEqual({ ok: false, error: { code: 'contest', tag: 'CQZ' } })
    const empty = [...fieldsOf(B), { tag: 'ITUZ', orig: null, value: '8', removed: false }]
    expect(cardSave(B, start, start, empty)).toEqual({ ok: true, edit: storedEdit(B), fields: [{ tag: 'ITUZ', old: null, new: '8' }] })
  })
})

describe('own-station fields on a contact already uploaded to LoTW', () => {
  const UP: LoggedQso = { ...Q, myGrid: 'EN52', upload: { lotw: { outcome: 'accepted', whenUnix: 1 } } }
  const own = (q: LoggedQso, over: Partial<CardDraft>, fields: CardField[] = fieldsOf(q)) =>
    ownStationNotResent(q, draftOf(q), { ...draftOf(q), ...over }, fields)

  it('names each one changed — the grid box and MY_ or OWNER_CALLSIGN fields — and nothing else', () => {
    const fields: CardField[] = [
      ...fieldsOf(UP),
      { tag: 'MY_STATE', orig: null, value: 'IL', removed: false },
      { tag: 'OWNER_CALLSIGN', orig: null, value: 'KD9XXX', removed: false },
      { tag: 'ITUZ', orig: null, value: '8', removed: false },
    ]
    expect(own(UP, { myGrid: 'EN53', name: 'Hiram' }, fields)).toEqual(['MY_GRIDSQUARE', 'MY_STATE', 'OWNER_CALLSIGN'])
    expect(own(UP, { name: 'Hiram' })).toEqual([])
  })

  it('says nothing where LoTW has not taken the contact, or a corrected call sends it again anyway', () => {
    expect(own({ ...UP, upload: { lotw: { outcome: 'pending', whenUnix: 1 } } }, { myGrid: 'EN53' })).toEqual([])
    expect(own({ ...UP, upload: undefined }, { myGrid: 'EN53' })).toEqual([])
    expect(own(UP, { myGrid: 'EN53', call: 'W1AX' })).toEqual([])
  })
})

describe('a multi-line field’s line breaks', () => {
  // A browser's box gives back bare line feeds, whatever the field was written with.
  const field = (orig: string): CardField => ({ tag: 'ADDRESS', orig, value: orig, removed: false })

  it('are the stored ones exactly while the text is as stored — typed away and back included', () => {
    for (const orig of ['1 Main St\r\nTown\nState', '1 Main St\rTown', '1 Main St\r\nTown', '1 Main St\nTown']) {
      const shown = orig.replace(/\r\n?/g, '\n')
      expect(typedLines(field(orig), shown)).toBe(orig)
    }
  })

  it('follow the field’s own once its text is edited: CR LF, a bare CR, or a line feed', () => {
    expect(typedLines(field('1 Main St\r\nTown'), '2 Main St\nTown')).toBe('2 Main St\r\nTown')
    expect(typedLines(field('1 Main St\rTown'), '2 Main St\nTown')).toBe('2 Main St\rTown')
    expect(typedLines(field('1 Main St\nTown'), '2 Main St\nTown')).toBe('2 Main St\nTown')
  })
})

describe('what a corrected call takes off, said before the save', () => {
  const HELD: LoggedQso = {
    ...Q,
    qslRcvd: { card: true, lotw: true, eqsl: false, qrz: false },
    extra: [['CNTY', 'CT,HARTFORD'], ['CQZ', '5'], ['ITUZ', '8'], ['CONT', 'NA'], ['EQSL_QSLRDATE', '20260101'], ['PFX', 'W1']],
  }
  const clears = (over: Partial<CardDraft>, fields: CardField[] = fieldsOf(HELD)) =>
    clearedByCallFix(HELD, { ...draftOf(HELD), ...over }, fields)

  it('names the zones, continent, county and the LoTW and eQSL confirmations the contact holds', () => {
    expect(clears({ call: 'W1AX' })).toEqual(['CQZ', 'ITUZ', 'CONT', 'CNTY', 'lotw', 'eqsl'])
  })

  it('nothing when the call is not corrected: the same call in other capitals or padded is not', () => {
    expect(clears({})).toEqual([])
    expect(clears({ call: ' w1aw ' })).toEqual([])
  })

  it('a value changed or removed in the same save is not named: changed it stays, removed it goes anyway', () => {
    const fields = fieldsOf(HELD).map((f) =>
      f.tag === 'CQZ' ? { ...f, value: '4' } : f.tag === 'ITUZ' ? { ...f, removed: true } : f)
    expect(clears({ call: 'W1AX' }, fields)).toEqual(['CONT', 'CNTY', 'lotw', 'eqsl'])
  })

  it('names only what the contact holds', () => {
    expect(clearedByCallFix(Q, { ...draftOf(Q), call: 'W1AX' }, fieldsOf(Q))).toEqual(['CNTY'])
  })
})
