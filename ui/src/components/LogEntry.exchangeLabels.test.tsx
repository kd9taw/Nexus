// @vitest-environment jsdom
//
// THE CONTEST LOG LINE NAMES EACH EXCHANGE BOX BY WHAT IT HOLDS.
//
// An operator rehearsing the Illinois QSO Party could not find where the other station's county
// goes. The box was there, captioned with its rules-file slot id, "QTH", which he did not read as
// "their county". Every received box in a contest line now says in words what it holds for THIS
// station's role ("County / State" inside Illinois, "IL county" outside it), and is blank while
// it is empty: no sample value from its list.
//
// The fixtures are the rules seed itself, put in the strip's wire shape the way
// `FdFieldDto::from_spec` puts it, so every caption below is the caption of a shipped contest in a
// role it ships. The table must name every ruleset and role in the seed: a contest added there is
// a red here until somebody says what its boxes are called.
//
// ⚠️ jsdom lays out nothing; no assertion here is about geometry.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, fireEvent, cleanup, act, screen } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { LogEntry } from './LogEntry'
import { ContestLogHead } from './ContestView'
import { ContestContactEdit } from './ContestContactEdit'
import { t } from '../i18n'
import type { AppSnapshot, ContestFieldSpec, FieldDayQso, FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  fdLogManual: vi.fn(() => Promise.resolve({})),
  contestLogManual: vi.fn(() => Promise.resolve(true)),
  contestWorking: vi.fn(() => Promise.resolve(true)),
  contestEntryReset: vi.fn(() => Promise.resolve(true)),
  contestIMoved: vi.fn(() => Promise.resolve({})),
  contestPlace: vi.fn(() => Promise.resolve(null)),
  contestEdit: vi.fn(() => Promise.resolve({ outcome: 'edited' })),
  logQso: vi.fn(() => Promise.resolve({})),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  resolveEntity: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
  setLogFormGrid: vi.fn(() => Promise.resolve()),
}))
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>

// ── the rules seed, as the strip receives it ─────────────────────────────────────────────────

type SeedKind = { type: string; domain?: string; of?: SeedKind[]; min?: number; max?: number }
type SeedField = { key: string; label: string; required: boolean; adif: { rcvd: string }; kind: SeedKind }
type SeedRuleset = {
  event: string
  exchange: { fields: SeedField[]; roles: { id: string; receives: string[] }[] }
}
const SEED = JSON.parse(
  readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), '../../../crates/tempo-core/src/fd_rules.seed.json'), 'utf8'),
) as { rulesets: SeedRuleset[] }

/** One received slot as `FdFieldDto::from_spec` writes it: the seed's `one_of` is the wire's
 *  `oneOf`, `domains` is every enum arm, and an empty ADIF tag or label is left off. */
function wire(f: SeedField): ContestFieldSpec {
  const k = f.kind
  const domains =
    k.type === 'enum' ? [k.domain!] : k.type === 'one_of' ? (k.of ?? []).filter((a) => a.type === 'enum').map((a) => a.domain!) : []
  return {
    key: f.key,
    kind: k.type === 'one_of' ? 'oneOf' : k.type,
    required: f.required,
    ...(k.type === 'enum' ? { domain: k.domain } : {}),
    ...(domains.length > 0 ? { domains } : {}),
    ...(k.type === 'number' ? { min: k.min, max: k.max } : {}),
    ...(f.adif.rcvd !== '' ? { adif: f.adif.rcvd } : {}),
    ...(f.label !== '' ? { label: f.label } : {}),
  }
}

/** The slots `role` receives in `event`, in receive order. */
function receivesOf(event: string, role: string): ContestFieldSpec[] {
  const rs = SEED.rulesets.find((r) => r.event === event)!
  const r = rs.exchange.roles.find((x) => x.id === role)!
  return r.receives.map((key) => wire(rs.exchange.fields.find((f) => f.key === key)!))
}

/** A session of `event` in `role`, as the engine serialises it. */
const contest = (event: string, role: string, receives = receivesOf(event, role)): FieldDayStatus =>
  ({
    running: false,
    state: 'Idle',
    qsoCount: 0,
    sections: 0,
    points: 0,
    event,
    log: [],
    role,
    receives,
  }) as unknown as FieldDayStatus

const snap = { radio: { band: '20m', dialMhz: 14.25 }, hunt: null } as unknown as AppSnapshot

/** The props a phone cockpit passes. */
function strip(fieldDay: FieldDayStatus | null) {
  return (
    <LogEntry
      onOpenLogbook={() => {}}
      snap={snap}
      mode="SSB"
      defaultRst="59"
      exchange="terrestrial"
      titled={false}
      fieldDay={fieldDay}
      fdMode="PH"
    />
  )
}

/** The received boxes' captions, after the strip's own Call. */
const captions = () =>
  [...document.querySelectorAll('.le-fd-big .le-fd-field:not(.le-fd-field-call) .le-fd-cap')].map((n) => n.textContent)
/** Each received box's placeholder, '' where it has none. */
const examples = () =>
  [...document.querySelectorAll<HTMLInputElement>('.le-fd-big .le-fd-field:not(.le-fd-field-call) input')].map(
    (i) => i.placeholder,
  )

afterEach(() => {
  cleanup()
  for (const f of Object.values(api)) f.mockClear()
})

// ── the operator's report ────────────────────────────────────────────────────────────────────

describe('the Illinois QSO Party: the county box says it is the county box', () => {
  it('in Illinois it reads County / State, not QTH, and shows no sample county while empty', () => {
    render(strip(contest('ilqp', 'in_state')))
    expect(captions(), 'an Illinois station hears counties and everyone else’s state').toEqual(['RST', 'County / State'])
    expect(examples()).toEqual(['', ''])
  })

  it('outside Illinois it reads IL county, whichever outside role the station is in', () => {
    for (const role of ['w_ve', 'dx']) {
      render(strip(contest('ilqp', role)))
      expect(captions(), role).toEqual(['RST', 'IL county'])
      expect(examples(), role).toEqual(['', ''])
      cleanup()
    }
  })
})

// ── every contest and role the seed ships ────────────────────────────────────────────────────

/** [event, role, the captions after Call, the placeholders ('' = none)]. */
const TABLE: [string, string, string[], string[]][] = [
  ['arrlfd', '', ['Class', 'Section'], ['', '']],
  ['wfd', '', ['Class', 'Section'], ['', '']],
  ['tnqp', 'in_state', ['RST', 'County / State'], ['', '']],
  ['tnqp', 'out_of_state', ['RST', 'TN county'], ['', '']],
  ['ohqp', 'in_state', ['RST', 'County / State'], ['', '']],
  ['ohqp', 'w_ve', ['RST', 'OH county'], ['', '']],
  ['ohqp', 'dx', ['RST', 'OH county'], ['', '']],
  ['cqp', 'in_state', ['Serial', 'County / State'], ['', '']],
  ['cqp', 'out_of_state', ['Serial', 'CA county'], ['', '']],
  ['txqp', 'in_state', ['RST', 'County / State'], ['', '']],
  ['txqp', 'out_of_state', ['RST', 'TX county'], ['', '']],
  ['ilqp', 'in_state', ['RST', 'County / State'], ['', '']],
  ['ilqp', 'w_ve', ['RST', 'IL county'], ['', '']],
  ['ilqp', 'dx', ['RST', 'IL county'], ['', '']],
  ['nyqp', 'in_state', ['RST', 'County / State'], ['', '']],
  ['nyqp', 'w_ve', ['RST', 'NY county'], ['', '']],
  ['nyqp', 'dx', ['RST', 'NY county'], ['', '']],
  ['arrlss_cw', '', ['Serial', 'Precedence', 'Call', 'Check', 'Section'], ['', '', '', '', '']],
  ['arrlss_ssb', '', ['Serial', 'Precedence', 'Call', 'Check', 'Section'], ['', '', '', '', '']],
  // CQ WW's zone box is blank too until a call is typed; then its placeholder is the country
  // file's hint for that call, the one hint a received box shows.
  ['cqww_cw', '', ['RST', 'Zone'], ['', '']],
  ['cqww_ssb', '', ['RST', 'Zone'], ['', '']],
  ['cqww_rtty', 'w_ve', ['RST', 'Zone', 'State'], ['', '', '']],
  ['cqww_rtty', 'dx', ['RST', 'Zone', 'State'], ['', '', '']],
  ['cqwpx_cw', '', ['RST', 'Serial'], ['', '']],
  ['cqwpx_ssb', '', ['RST', 'Serial'], ['', '']],
  ['arrlvhf_jan', '', ['Grid'], ['']],
  ['arrlvhf_jun', '', ['Grid'], ['']],
  ['arrlvhf_sep', '', ['Grid'], ['']],
  // The RTTY Roundup's two received boxes, either of which a contact fills: a W/VE station's state
  // or province, a DX station's serial.
  ['arrlrtty', 'w_ve', ['RST', 'State', 'Serial'], ['', '', '']],
  ['arrlrtty', 'dx', ['RST', 'State', 'Serial'], ['', '', '']],
]

describe('every received box in every shipped contest says what it holds', () => {
  it('names every ruleset and role in the seed', () => {
    const seeded = SEED.rulesets.flatMap((r) => r.exchange.roles.map((x) => `${r.event}/${x.id}`)).sort()
    expect(TABLE.map(([e, r]) => `${e}/${r}`).sort()).toEqual(seeded)
  })

  it.each(TABLE)('%s %s', (event, role, caps, eg) => {
    render(strip(contest(event, role)))
    expect(captions()).toEqual(caps)
    expect(examples()).toEqual(eg)
  })
})

// ── controls ─────────────────────────────────────────────────────────────────────────────────

describe('controls', () => {
  it('a slot whose rules label is not empty keeps that label', () => {
    const [rst, qth] = receivesOf('ilqp', 'in_state')
    render(strip(contest('ilqp', 'in_state', [rst, { ...qth, label: 'CNTY' }])))
    expect(captions()).toEqual(['RST', 'CNTY'])
    cleanup()
    // …and a blank one is no label at all.
    render(strip(contest('ilqp', 'in_state', [rst, { ...qth, label: '  ' }])))
    expect(captions()).toEqual(['RST', 'County / State'])
  })

  it('a log line outside a contest is the line it was', () => {
    render(strip(null))
    expect(document.querySelector('.le-fd-big')).toBeNull()
    expect(document.querySelectorAll('.le-fd-cap')).toHaveLength(0)
    expect([...document.querySelectorAll<HTMLInputElement>('input')].map((i) => i.placeholder)).toEqual(
      NON_CONTEST_PLACEHOLDERS,
    )
    // The shared comment is the line's one multi-line box, in a row of its own.
    expect([...document.querySelectorAll<HTMLTextAreaElement>('textarea')].map((i) => i.placeholder)).toContain(
      t('logEntry.comment.placeholder'),
    )
  })

  it('what is logged is the slot ids and the typed values, as it was', async () => {
    render(strip(contest('ilqp', 'in_state')))
    fireEvent.change(screen.getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' }), { target: { value: 'W9ABC' } })
    // By place, not by caption or example: this control has to read the same before the change.
    const [, county] = document.querySelectorAll<HTMLInputElement>('.le-fd-big .le-fd-field:not(.le-fd-field-call) input')
    fireEvent.change(county, { target: { value: 'cook' } })
    await act(async () => {
      fireEvent.click(document.querySelector('.le-fd-log-btn')!)
    })
    expect(api.contestLogManual).toHaveBeenCalledTimes(1)
    expect(api.contestLogManual.mock.calls[0]).toEqual([
      'W9ABC',
      [
        ['RST', '59'],
        ['QTH', 'COOK'],
      ],
      'PH',
      undefined,
    ])
  })

  it('the line under the boxes names the box by its caption', () => {
    render(strip(contest('ilqp', 'w_ve')))
    fireEvent.change(screen.getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' }), { target: { value: 'W9ABC' } })
    expect(screen.getByRole('alert').textContent).toBe(t('logEntry.fd.needField', { field: 'IL county' }))
  })
})

/** The ordinary log line's placeholders, in document order, read off the line before this change. */
const NON_CONTEST_PLACEHOLDERS: string[] = [
  t('logEntry.call.placeholder'),
  'RST',
  'RST',
  t('logEntry.name.placeholder'),
  t('logEntry.qth.placeholder'),
  t('logEntry.state.placeholder'),
  t('logEntry.country.placeholder'),
  t('logEntry.park.ref.placeholderPota'),
]

// ── the contest screen's column and the edit of a logged contact ─────────────────────────────

describe('the log table and the edit call a box what the strip calls it', () => {
  it('the column over the box is captioned the same, for the same role', () => {
    for (const [role, word] of [
      ['in_state', 'County / State'],
      ['w_ve', 'IL county'],
    ]) {
      render(<ContestLogHead fieldDayColumns={false} isWfd={false} slotColumns={receivesOf('ilqp', role)} role={role} />)
      expect([...document.querySelectorAll('.fd-log-head .fd-col.slot')].map((c) => c.textContent), role).toEqual([
        'RST',
        word,
      ])
      cleanup()
    }
  })

  it('the edit of a logged contact is captioned the same', () => {
    const row = {
      call: 'W9ABC',
      class: '',
      section: '',
      band: '20m',
      mode: 'PH',
      whenUnix: 1,
      rcvd: ['59', 'COOK'],
    } as unknown as FieldDayQso
    render(<ContestContactEdit rows={[row]} receives={receivesOf('ilqp', 'dx')} role="dx" onDone={() => {}} />)
    expect([...document.querySelectorAll('.contest-edit-cap')].map((c) => c.textContent)).toEqual([
      t('logEntry.fd.call.label'),
      'RST',
      'IL county',
      t('fieldDay.log.column.band'),
      t('fieldDay.log.column.mode'),
    ])
    // …and the line that refuses an emptied box names it the same way.
    const [, , county] = document.querySelectorAll<HTMLInputElement>('.contest-edit-field input')
    fireEvent.change(county, { target: { value: '' } })
    fireEvent.click(document.querySelector('.contest-edit-save')!)
    expect(document.querySelector('.contest-edit-why')?.textContent).toBe(
      t('logEntry.fd.needField', { field: 'IL county' }),
    )
    expect(api.contestEdit).not.toHaveBeenCalled()
  })
})
