// @vitest-environment jsdom
//
// ⭐ **THE COUNTY TYPE-AHEAD** — "Cook" reaches the log as COOK.
//
// A QSO-party county exchange is four letters an operator has to know (JODA, SCLA, MCDN)
// and the NAME is the part they actually know. The strip therefore accepts either, offers
// the codes while they type, and — this is the part that matters — LOGS the code, because
// a strip that shows a value good and logs a different one is the screen lying about what
// was written.
//
// Rendered with the props `PhoneCockpit` passes and a `fieldDay` of the shape the DTO
// really produces, the shape `LogEntry.contestStrip.test.tsx` established. jsdom lays
// nothing out; not one assertion here is about geometry.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, fireEvent, cleanup, act } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import { subscribeToasts } from '../toast'
import type { AppSnapshot, FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  fdLogManual: vi.fn(() => Promise.resolve({})),
  contestLogManual: vi.fn(() => Promise.resolve({})),
  contestWorking: vi.fn(() => Promise.resolve({})),
  contestEntryReset: vi.fn(() => Promise.resolve({})),
  contestIMoved: vi.fn(() => Promise.resolve({})),
  logQso: vi.fn(() => Promise.resolve({})),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  resolveEntity: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
  setLogFormGrid: vi.fn(() => Promise.resolve()),
  contestPlace: vi.fn(() => Promise.resolve(null)),
}))
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>

const snap = {
  radio: { band: '20m', dialMhz: 14.2 },
  hunt: null,
} as unknown as AppSnapshot

/** The Illinois QSO Party as the engine serialises it: RST plus ONE QTH box whose value
 *  may come from either of two universes — which is why the slot carries `domains` and
 *  no `domain` (a `oneOf` has no membership verdict; its free-text arm is how the
 *  sponsor says "or a country"). */
const ilqp = (): FieldDayStatus =>
  ({
    running: true,
    state: 'Idle',
    event: 'ilqp',
    qsoCount: 0,
    sections: 0,
    points: 0,
    log: [],
    role: 'in_state',
    receives: [
      { key: 'RST', kind: 'rst', required: true },
      { key: 'QTH', kind: 'oneOf', required: true, domains: ['il_counties', 'il_mults'] },
    ],
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'QTH', raw: 'COOK', domain: 'il_counties' },
    ],
  }) as unknown as FieldDayStatus

function renderStrip(fieldDay: FieldDayStatus = ilqp(), fdMode: 'CW' | 'PH' | 'DIG' = 'CW') {
  return render(
    <LogEntry
      onOpenLogbook={() => {}}
      snap={snap}
      mode="CW"
      defaultRst="599"
      exchange="terrestrial"
      titled={false}
      onSpot={() => {}}
      pendingWork={null}
      onConsumeWork={() => {}}
      fieldDay={fieldDay}
      fdMode={fdMode}
    />,
  )
}

/** A RECEIVED box by its caption — scoped to `.le-fd-big`, the row of boxes the operator
 *  types the other station's exchange into. The read-only sent display beside it captions
 *  the same slots, so an unscoped query can hand back the wrong input and pass. */
const box = (caption: string): HTMLInputElement => {
  const cap = [...document.querySelectorAll('.le-fd-big .le-fd-cap')].find(
    (n) => n.textContent === caption,
  )
  if (!cap) throw new Error(`no received box captioned ${caption}`)
  return cap.closest('label')!.querySelector('input')! as HTMLInputElement
}
const qthBox = () => box('County / State')
const callBox = () => screen.getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' }) as HTMLInputElement
const hits = () => [...document.querySelectorAll('.le-fd-suggest .le-fd-hit-code')].map((n) => n.textContent)

afterEach(() => {
  cleanup()
  for (const f of Object.values(api)) f.mockClear()
})

describe('the exchange type-ahead', () => {
  it('offers nothing until something is typed, and nothing for a slot with no domain', () => {
    renderStrip()
    expect(hits()).toEqual([])
    fireEvent.change(qthBox(), { target: { value: 'C' } })
    expect(hits().length).toBeGreaterThan(0)
    // The RST box draws on no domain at all: typing in it opens no list — and closes the
    // one under the box the operator has left. (`579`, not `599`: the box already holds
    // its default report, and React fires no change for a value that did not change.)
    fireEvent.change(box('RST'), { target: { value: '579' } })
    expect(hits()).toEqual([])
  })

  it('offers county codes for a name and fills the box when one is picked', () => {
    renderStrip()
    fireEvent.change(qthBox(), { target: { value: 'Cook' } })
    expect(hits()).toEqual(['COOK'])
    fireEvent.mouseDown(document.querySelector('.le-fd-suggest button')!)
    expect(qthBox().value).toBe('COOK')
    expect(hits()).toEqual([]) // the list closes behind the pick
  })

  it('resolves a typed name to its code when SPACE leaves the box', () => {
    renderStrip()
    fireEvent.change(qthBox(), { target: { value: 'st clair' } })
    fireEvent.keyDown(qthBox(), { key: ' ', code: 'Space' })
    expect(qthBox().value).toBe('SCLA')
  })

  it('leaves an ambiguous name exactly as typed — it never guesses', () => {
    renderStrip()
    // Macon, Macoupin, Madison, Marion, Marshall, Mason, Massac…
    fireEvent.change(qthBox(), { target: { value: 'Ma' } })
    fireEvent.keyDown(qthBox(), { key: ' ', code: 'Space' })
    expect(qthBox().value).toBe('MA')
    // …and a country a DX station sends belongs to no universe here, so it survives too.
    fireEvent.change(qthBox(), { target: { value: 'Germany' } })
    fireEvent.keyDown(qthBox(), { key: ' ', code: 'Space' })
    expect(qthBox().value).toBe('GERMANY')
  })

  it('LOGS the code for a name typed straight into the box', async () => {
    renderStrip()
    fireEvent.change(callBox(), { target: { value: 'K9NR' } })
    fireEvent.change(qthBox(), { target: { value: 'Kankakee' } })
    // Enter from the box itself — no space, no click, nothing that could have resolved
    // it on the way.
    fireEvent.keyDown(qthBox(), { key: 'Enter' })
    await vi.waitFor(() => expect(api.contestLogManual).toHaveBeenCalled())
    expect(api.contestLogManual.mock.calls[0][1]).toEqual([
      ['RST', '599'],
      ['QTH', 'KANK'],
    ])
  })

  it('LOGS Prince Edward Island, named in full, as the province: PEI', async () => {
    // The list holds PE and PEI, both Prince Edward Island — one province, so the name is not
    // ambiguous.
    renderStrip()
    fireEvent.change(callBox(), { target: { value: 'VY2AB' } })
    fireEvent.change(qthBox(), { target: { value: 'Prince Edward Island' } })
    fireEvent.keyDown(qthBox(), { key: 'Enter' })
    await vi.waitFor(() => expect(api.contestLogManual).toHaveBeenCalled())
    expect(api.contestLogManual.mock.calls[0][1]).toEqual([
      ['RST', '599'],
      ['QTH', 'PEI'],
    ])
  })

  it('costs no IPC: the whole type-ahead is a lookup on data already in hand', () => {
    renderStrip()
    for (const v of ['C', 'CO', 'COO', 'Cook', 'st clair', 'Ma']) {
      fireEvent.change(qthBox(), { target: { value: v } })
      fireEvent.keyDown(qthBox(), { key: ' ', code: 'Space' })
    }
    for (const [name, f] of Object.entries(api)) {
      expect(f, `${name} was called`).not.toHaveBeenCalled()
    }
  })

  it('does not change a contest whose slot has one universe — Field Day still refuses a bad section', () => {
    renderStrip({
      ...ilqp(),
      event: 'arrlfd',
      receives: [
        { key: 'CLASS', kind: 'pattern', required: true },
        { key: 'SECTION', kind: 'enum', required: true, domain: 'fd_sections', domains: ['fd_sections'] },
      ],
    } as unknown as FieldDayStatus)
    const section = box('Section')
    fireEvent.change(section, { target: { value: 'ZZ' } })
    expect(hits()).toEqual([]) // no section code or name begins ZZ
    // …and the section NAME resolves to its code, which is the same feature reaching a
    // contest that has always had a closed universe.
    fireEvent.change(section, { target: { value: 'Wisconsin' } })
    expect(hits()).toEqual(['WI'])
    fireEvent.keyDown(section, { key: ' ', code: 'Space' })
    expect(section.value).toBe('WI')
  })
})

// ---------------------------------------------------------------------------
// ⭐ THE DUPE BADGE, over a contest whose dupe rule counts two mode classes as ONE.
//
// ILQP works a station "once per band and mode (phone and CW/digital)", so a station
// worked on CW is a dupe on RTTY. The engine refuses it either way; the BADGE is what
// stops the operator calling them first and finding out after the over.
// ---------------------------------------------------------------------------

const worked = (mode: string) => ({
  call: 'K9NR',
  band: '20m',
  mode,
  class: '',
  section: '',
  submode: '',
  whenUnix: 1_792_342_800,
})

describe('the while-typing DUPE badge', () => {
  const dupeShown = () =>
    [...document.querySelectorAll('.le-fd-hint')].some((n) => /Dupe:/.test(n.textContent ?? ''))

  it('warns on the other half of a grouped mode: worked on CW, now on RTTY', () => {
    renderStrip(
      {
        ...ilqp(),
        dupeModeGroups: [['CW', 'DIG']],
        log: [worked('CW')],
      } as unknown as FieldDayStatus,
      'DIG',
    )
    fireEvent.change(callBox(), { target: { value: 'K9NR' } })
    expect(dupeShown()).toBe(true)
    // POSITIVE CONTROL: the identical fixture WITHOUT the grouping — the shipped
    // behaviour — leaves the badge off, so the true above is the fold and nothing else.
    cleanup()
    renderStrip({ ...ilqp(), log: [worked('CW')] } as unknown as FieldDayStatus, 'DIG')
    fireEvent.change(callBox(), { target: { value: 'K9NR' } })
    expect(dupeShown()).toBe(false)
  })

  it('does NOT warn when the ruleset groups nothing — the shipped behaviour', () => {
    renderStrip({
      ...ilqp(),
      event: 'ohqp',
      log: [worked('DIG')],
    } as unknown as FieldDayStatus)
    fireEvent.change(callBox(), { target: { value: 'K9NR' } })
    expect(dupeShown()).toBe(false)
    // POSITIVE CONTROL for the query itself: the SAME station on the same mode class
    // still raises the badge, so a false above is the grouping and not a broken check.
    cleanup()
    renderStrip({ ...ilqp(), event: 'ohqp', log: [worked('CW')] } as unknown as FieldDayStatus)
    fireEvent.change(callBox(), { target: { value: 'K9NR' } })
    expect(dupeShown()).toBe(true)
  })

  it('warns across the group in the other direction too', () => {
    renderStrip({
      ...ilqp(),
      dupeModeGroups: [['CW', 'DIG']],
      log: [worked('DIG')],
    } as unknown as FieldDayStatus)
    fireEvent.change(callBox(), { target: { value: 'K9NR' } })
    expect(dupeShown()).toBe(true)
    // …and PHONE is still its own mode: the grouping names two classes, not three.
    cleanup()
    renderStrip({
      ...ilqp(),
      dupeModeGroups: [['CW', 'DIG']],
      log: [worked('PH')],
    } as unknown as FieldDayStatus)
    fireEvent.change(callBox(), { target: { value: 'K9NR' } })
    expect(dupeShown()).toBe(false)
  })
})

// ⭐ "I MOVED" READS A COUNTY THE WAY THE QTH BOX DOES. A mobile knows the county it has just
// driven into by its NAME; the box sent exactly what was typed, so `DuPage` was refused as "not
// one of this contest's counties" while `DUPG` went through. Each box is now read by the QTH
// box's own function, and the engine still decides what is a county.
describe('"I moved" reads a county the way the QTH box does', () => {
  /** Open "I moved", type into its QTH box and press Save. */
  const moveTo = async (typed: string) => {
    fireEvent.click(screen.getByRole('button', { name: 'I moved' }))
    const qth = [...document.querySelectorAll('.le-fd-sent input')].find(
      (i) => i.getAttribute('aria-label') === 'QTH',
    )!
    fireEvent.change(qth, { target: { value: typed } })
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    })
  }

  it.each([
    ['a name', 'MONROE', 'MNRO'],
    ['a code', 'DUPG', 'DUPG'],
    ['a mixed-case name', 'DuPage', 'DUPG'],
    ['a name with its punctuation and spacing', 'st. clair', 'SCLA'],
  ])('%s: %s goes to the engine as %s', async (_what, typed, code) => {
    renderStrip()
    await moveTo(typed)
    expect(api.contestIMoved).toHaveBeenCalledWith([['QTH', code]])
  })

  it("reads New York's own list in the New York QSO Party", async () => {
    renderStrip({
      ...ilqp(),
      event: 'nyqp',
      receives: [
        { key: 'RST', kind: 'rst', required: true },
        { key: 'QTH', kind: 'oneOf', required: true, domains: ['ny_counties', 'ny_mults', 'dx_literal'] },
      ],
      composing: [
        { key: 'RST', raw: '599' },
        { key: 'QTH', raw: 'ALB', domain: 'ny_counties' },
      ],
    } as unknown as FieldDayStatus)
    await moveTo('Monroe')
    expect(api.contestIMoved).toHaveBeenCalledWith([['QTH', 'MON']])
  })

  /** A QSO party as the engine serialises it for a `role` station sending `raw`, from the list
   *  `domain` names (none for a value typed free, as a DX station's is). */
  const party = (st: 'il' | 'ny', role: string, raw: string, domain?: string): FieldDayStatus =>
    ({
      ...ilqp(),
      event: `${st}qp`,
      role,
      receives: [
        { key: 'RST', kind: 'rst', required: true },
        {
          key: 'QTH',
          kind: 'oneOf',
          required: true,
          domains: st === 'il' ? ['il_counties', 'il_mults'] : ['ny_counties', 'ny_mults', 'dx_literal'],
        },
      ],
      composing: [
        { key: 'RST', raw: '599' },
        { key: 'QTH', raw, ...(domain ? { domain } : {}) },
      ],
    }) as unknown as FieldDayStatus

  // Five counties share their name with a state or province on the same contest's list. The
  // QTH box cannot tell which was meant; I moved can, from the list the station sends from.
  it.each([
    ['il', 'COOK', 'il_counties', 'Washington', 'WASH'],
    ['ny', 'ALB', 'ny_counties', 'Delaware', 'DEL'],
    ['ny', 'ALB', 'ny_counties', 'Ontario', 'ONT'],
    ['ny', 'ALB', 'ny_counties', 'Washington', 'WAS'],
    ['ny', 'ALB', 'ny_counties', 'Wyoming', 'WYO'],
  ])('%sqp in-state, sending %s (%s): %s is the county %s', async (st, sent, list, typed, code) => {
    renderStrip(party(st as 'il' | 'ny', 'in_state', sent, list))
    await moveTo(typed)
    expect(api.contestIMoved).toHaveBeenCalledWith([['QTH', code]])
  })

  it('outside the state, the same name is the state the station sends', async () => {
    renderStrip(party('il', 'w_ve', 'IN', 'il_mults'))
    await moveTo('Washington')
    expect(api.contestIMoved).toHaveBeenCalledWith([['QTH', 'WA']])
  })

  it('a value sent from no list (a DX station types its own) is read as the QTH box reads it', async () => {
    renderStrip(party('il', 'dx', 'GERMANY'))
    await moveTo('DuPage')
    expect(api.contestIMoved).toHaveBeenCalledWith([['QTH', 'DUPG']])
  })

  it("leaves the QTH box's own reading alone: a name two lists hold stays as typed there", () => {
    renderStrip()
    fireEvent.change(qthBox(), { target: { value: 'Washington' } })
    fireEvent.keyDown(qthBox(), { key: ' ', code: 'Space' })
    expect(qthBox().value.toUpperCase()).toBe('WASHINGTON')
  })

  it('a name that is no county goes as typed, and the engine refuses it in its own words', async () => {
    const refusal = '"NARNIA" is not one of this contest\'s counties.'
    api.contestIMoved.mockRejectedValueOnce(refusal)
    let shown: string[] = []
    const off = subscribeToasts((ts) => {
      shown = ts.map((x) => x.message)
    })
    renderStrip()
    await moveTo('Narnia')
    off()
    expect(api.contestIMoved).toHaveBeenCalledWith([['QTH', 'NARNIA']])
    expect(shown).toContain(`Could not change what you send: ${refusal}`)
  })

  it('naming the county already being sent moves nothing', async () => {
    renderStrip({
      ...ilqp(),
      composing: [
        { key: 'RST', raw: '599' },
        { key: 'QTH', raw: 'SCLA', domain: 'il_counties' },
      ],
    } as unknown as FieldDayStatus)
    await moveTo('St. Clair')
    expect(api.contestIMoved).not.toHaveBeenCalled()
  })

  it('a slot with no received box keeps what was typed, as before', async () => {
    renderStrip({ ...ilqp(), receives: [{ key: 'RST', kind: 'rst', required: true }] } as unknown as FieldDayStatus)
    await moveTo(' DuPage ')
    expect(api.contestIMoved).toHaveBeenCalledWith([['QTH', 'DUPAGE']])
  })
})

// ---------------------------------------------------------------------------
// ⭐ A VALUE ON NO LIST, FROM A STATION IN THE USA OR CANADA.
//
// Such a station sends a county, a state or a province, and each is on the contest's lists. Only a
// station elsewhere sends what no list holds, its country, which is what the box's free-text arm is
// for. So from a call the country file places in the USA (Alaska and Hawaii are states on these
// lists) or in Canada, a value on no list is a typo (COKO), a half-typed code (DUP) or a name two
// lists share (Washington). It used to log as typed and score no county for either station. The
// verdict line now names it, offers the codes a name or a prefix matches, and Log stays off until
// the value is real.
// ---------------------------------------------------------------------------

/** What the country file says of each call typed below (`resolveEntity`), in cty.dat's names. */
const COUNTRY: Record<string, string> = {
  K9NR: 'United States',
  VE3ABC: 'Canada',
  KL7XX: 'Alaska',
  DL1ABC: 'Fed. Rep. of Germany',
}

describe('a value on no list, from a station in the USA or Canada', () => {
  beforeEach(() => {
    api.resolveEntity.mockImplementation((call: string) => Promise.resolve(COUNTRY[call] ?? null))
  })
  afterEach(() => {
    api.resolveEntity.mockImplementation(() => Promise.resolve(null))
  })

  const verdicts = () => [...document.querySelectorAll('.le-fd-verdicts .le-fd-hint')].map((n) => n.textContent)
  const logButton = () => document.querySelector('.le-fd-log-btn') as HTMLButtonElement
  /** Type the call, let the country file answer for it, then type the QTH. */
  const typeContact = async (call: string, qth: string) => {
    fireEvent.change(callBox(), { target: { value: call } })
    await act(async () => {})
    fireEvent.change(qthBox(), { target: { value: qth } })
  }
  /** Press Enter in the QTH box; what the single-contact command was handed, every call. */
  const pressEnter = async () => {
    await act(async () => {
      fireEvent.keyDown(qthBox(), { key: 'Enter' })
    })
    return api.contestLogManual.mock.calls.map((c) => c[1])
  }

  it.each([
    ['K9NR', 'the USA'],
    ['VE3ABC', 'Canada'],
    ['KL7XX', 'Alaska, a state on both lists'],
  ])('%s (%s) sending COKO: the line names it, Log stays off, and Enter logs nothing', async (call) => {
    renderStrip()
    await typeContact(call, 'COKO')
    expect(verdicts()).toEqual(["COKO is not a county, state or province on this contest's list."])
    expect(logButton().disabled).toBe(true)
    expect(await pressEnter()).toEqual([])
  })

  it.each([
    // A name two lists share: the county, then the state.
    ['K9NR', 'Washington', 'WASHINGTON', 'WASH or WA'],
    // A prefix: the codes that begin with it, then St. Clair by its name.
    ['K9NR', 'ST', 'ST', 'STAR, STEP, or SCLA'],
    // A code typed in part.
    ['K9NR', 'DUP', 'DUP', 'DUPG'],
    // A province's name typed in part.
    ['VE3ABC', 'Ont', 'ONT', 'ON'],
  ])('%s sending %s, shown as %s: the line offers %s', async (call, typed, shown, codes) => {
    renderStrip()
    await typeContact(call, typed)
    expect(verdicts()).toEqual([`${shown} is not a code on this contest's list. Did you mean ${codes}?`])
    expect(logButton().disabled).toBe(true)
    expect(await pressEnter()).toEqual([])
  })

  it.each(['IL', 'SAINT CLAIR', 'WHITESIDE CO'])(
    'K9NR sending %s: named, and nothing offered, since it begins no code and no name',
    async (typed) => {
      renderStrip()
      await typeContact('K9NR', typed)
      expect(verdicts()).toEqual([`${typed} is not a county, state or province on this contest's list.`])
      expect(await pressEnter()).toEqual([])
    },
  )

  it.each([
    ['K9NR', 'WASH', 'WASH'],
    ['K9NR', 'Kankakee', 'KANK'],
    ['K9NR', 'wa', 'WA'],
    ['VE3ABC', 'ON', 'ON'],
  ])('%s sending %s logs %s: a code or a name on the list is real', async (call, typed, code) => {
    renderStrip()
    await typeContact(call, typed)
    expect(verdicts()).toEqual([])
    expect(logButton().disabled).toBe(false)
    expect(await pressEnter()).toEqual([[['RST', '599'], ['QTH', code]]])
  })

  it('a DX call still sends its country, which no list holds; the same text from a US call does not log', async () => {
    renderStrip()
    await typeContact('DL1ABC', 'Germany')
    expect(verdicts()).toEqual([])
    expect(logButton().disabled).toBe(false)
    expect(await pressEnter()).toEqual([[['RST', '599'], ['QTH', 'GERMANY']]])
    cleanup()
    api.contestLogManual.mockClear()
    // CONTROL: the same box and the same text, from a call in the USA.
    renderStrip()
    await typeContact('K9NR', 'Germany')
    expect(verdicts()).toEqual(["GERMANY is not a county, state or province on this contest's list."])
    expect(await pressEnter()).toEqual([])
  })

  it('a call the country file cannot place is never refused on a guess: the box takes it as it always did', async () => {
    renderStrip()
    await typeContact('ZZ9ZZZ', 'COKO')
    expect(api.resolveEntity).toHaveBeenCalledWith('ZZ9ZZZ')
    expect(verdicts()).toEqual([])
    expect(await pressEnter()).toEqual([[['RST', '599'], ['QTH', 'COKO']]])
  })

  it('a county line keeps its own verdict', async () => {
    renderStrip()
    await typeContact('K9NR', 'COOK/XYZ')
    expect(verdicts()).toEqual(["XYZ is not a county on the sponsor's list, so this county line cannot be logged."])
    expect(logButton().disabled).toBe(true)
  })

  it('judges no box drawing on a list this build does not hold, as New York’s DX', async () => {
    renderStrip({
      ...ilqp(),
      event: 'nyqp',
      receives: [
        { key: 'RST', kind: 'rst', required: true },
        { key: 'QTH', kind: 'oneOf', required: true, domains: ['ny_counties', 'ny_mults', 'dx_literal'] },
      ],
    } as unknown as FieldDayStatus)
    await typeContact('K9NR', 'XYZ')
    expect(verdicts()).toEqual([])
    expect(await pressEnter()).toEqual([[['RST', '599'], ['QTH', 'XYZ']]])
    cleanup()
    api.contestLogManual.mockClear()
    // CONTROL: the Illinois box, whose two lists are both held here, refuses the same value.
    renderStrip()
    await typeContact('K9NR', 'XYZ')
    expect(verdicts()).toEqual(["XYZ is not a county, state or province on this contest's list."])
    expect(await pressEnter()).toEqual([])
  })
})
