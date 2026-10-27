// ⭐ EDITING A LOGGED CONTEST CONTACT, the words and the form: what a contact opens with, what an
// edit sends (a county line as one vector per county), what the strip's own rules refuse, the
// key, and how the answer is said.
import { describe, it, expect } from 'vitest'
import type { ContestEditAnswer } from '../api'
import type { ContestFieldSpec, FieldDayQso } from '../types'
import { bandOptions, editForm, editOf, editText, isEditKey, modeOptions } from './contestEdit'

const T0 = 1_792_342_800 // 2026-10-18 17:00Z

const PARTY: ContestFieldSpec[] = [
  { key: 'RST', kind: 'rst', required: true },
  { key: 'QTH', kind: 'enum', required: true, domain: 'il_counties', domains: ['il_counties'] },
]
const FIELD_DAY: ContestFieldSpec[] = [
  { key: 'CLASS', kind: 'pattern', required: true },
  { key: 'SECTION', kind: 'enum', required: true, domain: 'fd_sections' },
]

const row = (call: string, qth: string, over: Partial<FieldDayQso> = {}): FieldDayQso =>
  ({ call, class: '', section: '', band: '20m', mode: 'CW', submode: '', whenUnix: T0, rcvd: ['599', qth], ...over }) as FieldDayQso

describe('what a contact opens with', () => {
  it('its call, what they sent, its band, and its own mode kept', () => {
    expect(editForm([row('K9AAA', 'COOK')], PARTY)).toEqual({
      call: 'K9AAA',
      fields: { RST: '599', QTH: 'COOK' },
      band: '20m',
      mode: '',
    })
  })

  it('a county line as the strip takes one: its counties joined by /, in row order', () => {
    expect(editForm([row('K9AAA', 'COOK'), row('K9AAA', 'LAKE')], PARTY).fields).toEqual({ RST: '599', QTH: 'COOK/LAKE' })
  })

  it('Field Day’s class and section, which ride the row', () => {
    const fd = { call: 'W1AW', class: '2A', section: 'CT', band: '40m', mode: 'DIG', whenUnix: T0 } as FieldDayQso
    expect(editForm([fd], FIELD_DAY).fields).toEqual({ CLASS: '2A', SECTION: 'CT' })
  })

  it('offers its own mode first, kept, then the others; its own band first, then the contest’s', () => {
    expect(modeOptions(row('K9AAA', 'COOK', { mode: 'PH', submode: 'USB' })).map((m) => [m.value, m.label])).toEqual([
      ['', 'USB'],
      ['CW', 'CW'],
      ['LSB', 'LSB'],
      ['FM', 'FM'],
      ['AM', 'AM'],
      ['RTTY', 'RTTY'],
      ['FT8', 'FT8'],
      ['FT4', 'FT4'],
    ])
    expect(bandOptions(row('K9AAA', 'COOK'), ['80m', '40m', '20m'])).toEqual(['20m', '80m', '40m'])
  })
})

describe('what an edit sends', () => {
  it('the call as a call is written, and each box as the strip would log it', () => {
    const form = { call: ' k9 aab ', fields: { RST: '579', QTH: 'Cook' }, band: '40m', mode: 'USB' }
    expect(editOf(form, [row('K9AAA', 'COOK')], PARTY)).toEqual({
      edit: { call: 'K9AAB', rows: [[['RST', '579'], ['QTH', 'COOK']]], band: '40m', mode: 'USB' },
    })
  })

  it('a county line as one vector per county, in the order typed', () => {
    const line = [row('K9AAA', 'COOK'), row('K9AAA', 'LAKE')]
    const form = { ...editForm(line, PARTY), fields: { RST: '599', QTH: 'COOK+DUPG' } }
    expect(editOf(form, line, PARTY)).toEqual({
      edit: {
        call: 'K9AAA',
        rows: [
          [['RST', '599'], ['QTH', 'COOK']],
          [['RST', '599'], ['QTH', 'DUPG']],
        ],
        band: '20m',
        mode: '',
      },
    })
  })

  it('refuses what the strip would refuse to log, in its words, and a county line of another count', () => {
    const one = [row('K9AAA', 'COOK')]
    const form = editForm(one, PARTY)
    expect(editOf({ ...form, call: '  ' }, one, PARTY)).toEqual({ why: 'A contact needs a call.' })
    expect(editOf({ ...form, fields: { RST: '599', QTH: 'XXXX' } }, one, PARTY)).toEqual({
      why: 'County "XXXX" isn\'t a known value — required to log.',
    })
    // …naming the box as it is captioned for the station's role: outside Illinois, IL county.
    expect(editOf({ ...form, fields: { RST: '599', QTH: 'XXXX' } }, one, PARTY, 'w_ve')).toEqual({
      why: 'IL county "XXXX" isn\'t a known value — required to log.',
    })
    expect(editOf({ ...form, fields: { RST: '599', QTH: 'COOK/LAKE' } }, one, PARTY)).toEqual({
      why: 'This contact was logged for one county, and an edit keeps it one.',
    })
    const line = [row('K9AAA', 'COOK'), row('K9AAA', 'LAKE')]
    expect(editOf({ ...editForm(line, PARTY), fields: { RST: '599', QTH: 'COOK' } }, line, PARTY)).toEqual({
      why: 'This contact is a county line of 2 counties, and an edit keeps 2: give them joined by /.',
    })
    // CONTROL: the same form, right, is an edit.
    expect('edit' in editOf(form, one, PARTY)).toBe(true)
  })

  it('refuses a QTH on no list from a call in the USA or Canada, as the strip does; a DX call sends its country', () => {
    // The Illinois QSO Party's box: the county list, the state and province list, and the free-text
    // arm a DX station's country goes in.
    const qth: ContestFieldSpec[] = [
      { key: 'RST', kind: 'rst', required: true },
      { key: 'QTH', kind: 'oneOf', required: true, domains: ['il_counties', 'il_mults'] },
    ]
    const one = [row('K9AAA', 'COOK')]
    const form = { call: 'K9AAA', fields: { RST: '599', QTH: 'GERMANY' }, band: '20m', mode: '' }
    expect(editOf(form, one, qth, 'in_state', 'United States')).toEqual({
      why: "GERMANY is not a county, state or province on this contest's list.",
    })
    expect(editOf(form, one, qth, 'in_state', 'Fed. Rep. of Germany')).toEqual({
      edit: { call: 'K9AAA', rows: [[['RST', '599'], ['QTH', 'GERMANY']]], band: '20m', mode: '' },
    })
    // A box naming no list at all has no list to be off, so it refuses nothing, whoever sends.
    const bare: ContestFieldSpec[] = [qth[0], { key: 'QTH', kind: 'oneOf', required: true }]
    expect('edit' in editOf(form, one, bare, 'in_state', 'United States')).toBe(true)
  })
})

describe('Ctrl+Y is the key that edits the last contact', () => {
  it('Ctrl alone, never with Alt, Shift or Cmd', () => {
    const key = (init: Partial<KeyboardEvent>) =>
      isEditKey({ key: 'y', ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...init } as KeyboardEvent)
    expect(key({ ctrlKey: true })).toBe(true)
    expect(key({ ctrlKey: true, key: 'Y' })).toBe(true)
    expect(key({ ctrlKey: true, altKey: true })).toBe(false)
    expect(key({ ctrlKey: true, shiftKey: true })).toBe(false)
    expect(key({ ctrlKey: true, metaKey: true })).toBe(false)
    expect(key({})).toBe(false)
  })
})

describe('the answer, in words', () => {
  const edited = (sentTo: Partial<Extract<ContestEditAnswer, { outcome: 'edited' }>['sentTo']> = {}): ContestEditAnswer => ({
    outcome: 'edited',
    before: [row('K9AAB', 'COOK')],
    after: [row('K9AAA', 'COOK')],
    sentTo: { wsjtx: false, logbook: false, uploaded: [], ...sentTo },
  })

  it('names the contact as it is now, and nothing else when it had gone nowhere', () => {
    expect(editText(edited())).toBe('Edited K9AAB: now K9AAA · 20m CW · 17:00 · 599 COOK.')
  })

  it('names every place it had already gone, and the logbook copy that still says the old call', () => {
    expect(
      editText(edited({ n3fjp: '192.168.1.20', n1mm: '192.168.1.255:12060', wsjtx: true, logbook: true, uploaded: ['qrz', 'clublog'] })),
    ).toBe(
      'Edited K9AAB: now K9AAA · 20m CW · 17:00 · 599 COOK. It had already gone to N3FJP (192.168.1.20), the N1MM broadcast, ' +
        'WSJT-X listeners, which still have it as it was: fix it there too. Your logbook has a copy merged before the edit, ' +
        'which still says K9AAB, and QRZ, Club Log have it: edit it there too.',
    )
    expect(editText(edited({ logbook: true }))).toBe(
      'Edited K9AAB: now K9AAA · 20m CW · 17:00 · 599 COOK. Your logbook has a copy merged before the edit, which still says K9AAB: edit it there too.',
    )
  })

  it('says why nothing was edited', () => {
    const refused = (refusal: Extract<ContestEditAnswer, { outcome: 'refused' }>['refusal']) => editText({ outcome: 'refused', refusal })
    expect(refused('changed')).toBe('That contact changed in the contest log since its edit opened, so nothing was edited. To edit it as it is now, open it again.')
    expect(refused('incomplete')).toBe('A contact needs a call and a band, so nothing was edited.')
    expect(refused('rows')).toBe('A county line keeps its number of counties, so nothing was edited.')
    expect(refused('satellite')).toBe('A contact made through a satellite keeps the band of its pass, so nothing was edited.')
    expect(refused('club')).toMatch(/^At a club an edit cannot reach the other laptops yet, so nothing was edited\. To fix your newest contact, take it back \(Ctrl\+D twice/)
  })
})
