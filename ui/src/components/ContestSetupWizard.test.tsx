// @vitest-environment jsdom
//
// ⭐ THE CONTEST SETUP: its steps, the engine's own blocker for answers not yet saved, ONE save through
// the patch seam with Contest mode on, and the check that the engine really entered the contest —
// with Contest mode turned off again, and the reason said, when it did not. Plus the quiet "this
// weekend" rule and Field Day's one power question.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor, within } from '@testing-library/react'
import { ContestSetupWizard, contestSoon, enteredContest, fdPowerAnswer, fdPowerFrom, SOON_SECS } from './ContestSetupWizard'
import type { AppSnapshot, Settings } from '../types'
import type { ContestWindow, FdRulesetDto } from '../api'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

/** The ARRL RTTY Roundup's banned modes, as the rules seed ships them. */
const RRU_BANNED: string[] = (
  JSON.parse(readFileSync(resolve(process.cwd(), '../crates/tempo-core/src/fd_rules.seed.json'), 'utf8')) as {
    rulesets: { event: string; banned_modes: string[] }[]
  }
).rulesets.find((r) => r.event === 'arrlrtty')!.banned_modes

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})
vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const mod: Record<string, unknown> = {}
  for (const name of Object.keys(actual)) {
    mod[name] = typeof actual[name] === 'function' ? api.get(name) : actual[name]
  }
  return mod
})

const NOW = Date.UTC(2026, 9, 21, 12) / 1000 // Wednesday 2026-10-21 1200Z
const ILQP: ContestWindow = { id: 'ilqp', startUnix: NOW + 3 * 86_400, endUnix: NOW + 4 * 86_400 }
const WINDOWS: ContestWindow[] = [
  { id: 'arrlfd', startUnix: NOW + 240 * 86_400, endUnix: NOW + 241 * 86_400 },
  ILQP,
  { id: 'cqww_cw', startUnix: NOW + 38 * 86_400, endUnix: NOW + 40 * 86_400 },
]

let station: Partial<Settings> = {}
let snapAfterSave: Partial<AppSnapshot> = {}

/** The engine's preview, as the shack answers it: the Illinois QSO Party refuses a blank county and
 *  sends the county once typed; ARRL Field Day sends class and section. */
function preview(patch: Partial<Settings>): FdRulesetDto {
  const s = { ...station, ...patch }
  const event = s.fdEvent || 'arrlfd'
  if (event === 'ilqp') {
    const county = (s.contestQthCounty ?? '').trim()
    // A station outside Illinois sends its state.
    const state = (s.contestQthState ?? '').trim()
    if (state && state !== 'IL') {
      return { ...preview({ ...patch, contestQthState: 'IL', contestQthCounty: 'COOK' }), role: 'out_of_state', exchange: ['599', state] }
    }
    return {
      event,
      rulesYear: 2026,
      bannedModes: ['FT8', 'FT4'],
      spottingAllowed: true,
      clusterAllowed: true,
      enforcement: 'warn',
      role: 'in_state',
      exchange: county ? ['599', county] : [],
      problem: county ? '' : 'Your QTH is empty — it is part of the exchange you transmit.',
      sentSettings: ['contest_qth_county', 'contest_qth_state'],
      cabrilloHeaders: ['CLAIMED-SCORE', 'EMAIL', 'IL-COUNTY', 'NAME', 'CLUB', 'ENTRY-CLASS', 'OPERATORS'],
      entryClasses: ['ILLINOIS FIXED HIGH POWER', 'UNLIMITED'],
    }
  }
  // The ARRL RTTY Roundup, as the engine previews it (src-tauri's
  // `the_preview_offers_the_declarations_the_sponsor_names`): a W/VE station sends its state, heads its
  // file with its ARRL section (LOCATION), and the contest takes RTTY alone.
  if (event === 'arrlrtty') {
    const state = (s.contestQthState ?? '').trim()
    return {
      event,
      rulesYear: 2026,
      bannedModes: RRU_BANNED,
      spottingAllowed: true,
      clusterAllowed: true,
      enforcement: 'warn',
      role: 'w_ve',
      exchange: ['599', state],
      problem: '',
      sentSettings: ['contest_qth_state', 'fd_section'],
      cabrilloHeaders: ['CATEGORY-ASSISTED', 'CATEGORY-MODE', 'CATEGORY-POWER', 'CLAIMED-SCORE', 'CLUB', 'OPERATORS'],
      transmitterCategories: ['ONE', 'TWO', 'UNLIMITED'],
    }
  }
  // The ARRL VHF contests send the grid, from the station's own grid: not a location "I moved" changes.
  // They head the file with the ARRL section (LOCATION), as the engine previews it.
  if (event.startsWith('arrlvhf')) {
    return {
      event,
      rulesYear: 2026,
      bannedModes: [],
      spottingAllowed: true,
      clusterAllowed: true,
      enforcement: 'warn',
      role: '',
      exchange: [s.mygrid ?? ''],
      problem: '',
      sentSettings: ['mygrid', 'fd_section'],
    }
  }
  return {
    event,
    rulesYear: 2026,
    bannedModes: [],
    spottingAllowed: true,
    clusterAllowed: true,
    enforcement: 'warn',
    role: '',
    exchange: [s.fdClass ?? '', s.fdSection ?? ''],
    problem: '',
    sentSettings: ['fd_class', 'fd_section'],
  }
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'], now: NOW * 1000 })
  for (const spy of Object.values(api.spies)) {
    spy.mockReset()
    spy.mockImplementation(() => Promise.resolve(null))
  }
  station = {
    mycall: 'K9ABC',
    fdActive: false,
    fdEvent: 'ilqp',
    contestQthState: 'IL',
    contestQthCounty: '',
    fdClass: '',
    fdSection: '',
    fdPowerMult: 2,
  }
  snapAfterSave = { mode: 'fieldDay', fieldDay: { event: 'ilqp', running: false } as AppSnapshot['fieldDay'] }
  api.get('getSettings').mockImplementation(() => Promise.resolve(structuredClone(station)))
  api.get('setSettings').mockImplementation((s: Partial<Settings>) => {
    station = structuredClone(s)
    return Promise.resolve(snapAfterSave)
  })
  api.get('getSnapshot').mockImplementation(() =>
    Promise.resolve({ fieldDay: null, mode: 'chat', radio: { catOk: true, operatingMode: 'digital' }, link: { tier: 'FT8' } }),
  )
  api.get('contestWindows').mockImplementation(() => Promise.resolve(WINDOWS))
  api.get('previewFdRuleset').mockImplementation((patch: Partial<Settings>) => Promise.resolve(preview(patch)))
  api.get('getFdRuleset').mockImplementation(() => Promise.resolve(preview({})))
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

const writes = () => api.get('setSettings').mock.calls.map((c) => c[0] as Partial<Settings>)
const next = () => fireEvent.click(screen.getByRole('button', { name: 'Next' }))
/** Next, once the preview of the answers so far has come back (Next waits for it). */
const nextWhenChecked = async () => {
  await waitFor(() => expect(screen.getByRole('button', { name: 'Next' })).toHaveProperty('disabled', false))
  next()
}

async function open(props: Partial<Parameters<typeof ContestSetupWizard>[0]> = {}) {
  const onClose = vi.fn()
  render(<ContestSetupWizard onClose={onClose} {...props} />)
  await screen.findByRole('dialog', { name: 'Set up a contest' })
  // The pick list once the windows have arrived.
  await screen.findByText('Running now or this week')
  return { onClose }
}

describe('the quiet "this weekend" line', () => {
  const at = (secs: number): ContestWindow => ({ id: 'ilqp', startUnix: NOW + secs, endUnix: NOW + secs + 86_400 })
  it('names a contest that starts within seven days, and only then', () => {
    expect(contestSoon([at(SOON_SECS)], NOW)?.id).toBe('ilqp')
    expect(contestSoon([at(SOON_SECS + 1)], NOW)).toBeNull()
    // Running already, or not one the picker offers: no line.
    expect(contestSoon([at(-60)], NOW)).toBeNull()
    expect(contestSoon([{ id: 'iaru', startUnix: NOW + 60, endUnix: NOW + 3600 }], NOW)).toBeNull()
    // The soonest wins.
    expect(contestSoon([{ ...at(5 * 86_400), id: 'nyqp' }, at(2 * 86_400)], NOW)?.id).toBe('ilqp')
  })
})

describe("ARRL Field Day's power: one question sets both stored fields", () => {
  it('maps QRP / LOW / HIGH and the battery answer to the category and the multiplier', () => {
    expect(fdPowerAnswer('QRP', true)).toEqual({ contestCategoryPower: 'QRP', fdPowerMult: 5 })
    expect(fdPowerAnswer('QRP', false)).toEqual({ contestCategoryPower: 'QRP', fdPowerMult: 2 })
    expect(fdPowerAnswer('LOW', true)).toEqual({ contestCategoryPower: 'LOW', fdPowerMult: 2 })
    expect(fdPowerAnswer('HIGH', true)).toEqual({ contestCategoryPower: 'HIGH', fdPowerMult: 1 })
    // Read back from what is stored: the declaration first, else the multiplier's.
    expect(fdPowerFrom({ contestCategoryPower: 'LOW', fdPowerMult: 1 })).toEqual({ power: 'LOW', battery: false })
    expect(fdPowerFrom({ contestCategoryPower: '', fdPowerMult: 5 })).toEqual({ power: 'QRP', battery: true })
    expect(fdPowerFrom({ fdPowerMult: 1 })).toEqual({ power: 'HIGH', battery: false })
  })

  it('on the entry step, QRP with battery saves ×5 and QRP; taking the battery away saves ×2', async () => {
    station = { ...station, fdEvent: 'arrlfd', fdClass: '1D', fdSection: 'WI' }
    snapAfterSave = { mode: 'fieldDay', fieldDay: { event: 'arrlfd', running: false } as AppSnapshot['fieldDay'] }
    await open()
    next()
    await screen.findByText('You send: 1D WI')
    next()
    fireEvent.click(screen.getByRole('radio', { name: 'QRP: 5 W or less (×5)' }))
    fireEvent.click(screen.getByRole('checkbox', { name: /Battery or other natural power/ }))
    expect(screen.queryByText('QRP on mains or generator power scores ×2.')).toBeNull()
    fireEvent.click(screen.getByRole('checkbox', { name: /Battery or other natural power/ }))
    expect(screen.getByText('QRP on mains or generator power scores ×2.')).toBeTruthy()
    fireEvent.click(screen.getByRole('checkbox', { name: /Battery or other natural power/ }))
    await nextWhenChecked()
    next()
    fireEvent.click(await screen.findByRole('button', { name: 'Start ARRL Field Day' }))
    await screen.findByText('ARRL Field Day is on · S&P')
    expect(writes()[0]).toMatchObject({ fdActive: true, contestCategoryPower: 'QRP', fdPowerMult: 5 })
  })
})

describe('the steps', () => {
  it('pick: this week first, in UTC', async () => {
    await open()
    const soon = screen.getByRole('radiogroup', { name: 'Running now or this week' })
    expect(within(soon).getAllByRole('radio').map((r) => r.textContent)).toEqual([
      'Illinois QSO PartyOct 24 1200Z to Oct 25 1200Z',
    ])
    expect(within(soon).getByRole('radio').getAttribute('aria-checked')).toBe('true')
    // Later ones follow, soonest first.
    const later = within(screen.getByRole('radiogroup', { name: 'Later' })).getAllByRole('radio')
    expect(later.slice(0, 2).map((r) => r.querySelector('.contest-setup-pick-name')!.textContent)).toEqual([
      'CQ World-Wide DX Contest (CW)',
      'ARRL Field Day',
    ])
  })

  it('exchange: only what the contest sends, and the engine\'s blocker for the unsaved answers holds Next', async () => {
    await open()
    next()
    expect(await screen.findByRole('alert')).toHaveProperty(
      'textContent',
      'Your QTH is empty — it is part of the exchange you transmit.',
    )
    expect(screen.getByLabelText('County')).toBeTruthy()
    expect(screen.getByLabelText('State or province')).toBeTruthy()
    expect(screen.queryByLabelText('FD Class')).toBeNull()
    expect(screen.getByRole('button', { name: 'Next' })).toHaveProperty('disabled', true)
    // The preview is asked for the DRAFT, and nothing is saved while it is.
    expect(api.get('previewFdRuleset')).toHaveBeenCalledWith({})
    fireEvent.change(screen.getByLabelText('County'), { target: { value: 'cook' } })
    await screen.findByText('You send: 599 COOK')
    expect(api.get('previewFdRuleset')).toHaveBeenLastCalledWith({ contestQthCounty: 'COOK' })
    expect(screen.queryByRole('alert')).toBeNull()
    expect(screen.getByRole('button', { name: 'Next' })).toHaveProperty('disabled', false)
    expect(writes()).toEqual([])
  })

  it('entry: the lines this contest writes — its entry classes, club, operators, email — and no band', async () => {
    station = { ...station, contestQthCounty: 'COOK' }
    await open()
    next()
    await screen.findByText('You send: 599 COOK')
    next()
    const cls = screen.getByLabelText('Entry class') as HTMLSelectElement
    expect([...cls.options].map((o) => o.value)).toEqual(['', 'ILLINOIS FIXED HIGH POWER', 'UNLIMITED'])
    expect(screen.getByLabelText('Club')).toBeTruthy()
    expect(screen.getByLabelText('Other operators')).toBeTruthy()
    expect(screen.getByLabelText('Email for contest logs')).toBeTruthy()
    expect(screen.queryByLabelText('Band category')).toBeNull()
    expect(screen.queryByLabelText('Power category')).toBeNull()
  })

  it('radio: a banned mode is a warning with a way to CW or Phone, never a refusal', async () => {
    station = { ...station, contestQthCounty: 'COOK' }
    await open()
    next()
    await screen.findByText('You send: 599 COOK')
    next()
    next()
    const warn = document.querySelector('[data-banned]')!
    expect(warn.textContent).toContain('Illinois QSO Party does not allow FT8: contacts in it are logged but score nothing.')
    expect(within(warn as HTMLElement).getByRole('button', { name: 'Work this contest in CW' })).toBeTruthy()
    expect(within(warn as HTMLElement).getByRole('button', { name: 'Work this contest in Phone' })).toBeTruthy()
    expect(screen.getByText('The radio is connected.')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Next' })).toHaveProperty('disabled', false)
    next()
    expect(screen.getByRole('button', { name: 'Start Illinois QSO Party' })).toHaveProperty('disabled', false)
  })

  it('the ARRL RTTY Roundup: the state alone, its entry lines, and FT8 or CW a warning with a way to RTTY, never a refusal', async () => {
    await open({ initial: { fdEvent: 'arrlrtty' } })
    next()
    await screen.findByText('You send: 599 IL')
    // Only what a W/VE station sends (Special Rules 4.2.1): no county, no zone, no class.
    expect(screen.getByLabelText('State or province')).toBeTruthy()
    // …and the ARRL section its log is filed under (the Cabrillo LOCATION ARRL's upload asks for).
    expect(screen.getByLabelText('ARRL Section')).toBeTruthy()
    expect(screen.queryByLabelText('County')).toBeNull()
    expect(screen.queryByLabelText('FD Class')).toBeNull()
    next()
    expect(screen.getByLabelText('Power category')).toBeTruthy()
    expect(screen.getByLabelText('Club')).toBeTruthy()
    expect(screen.getByLabelText('Other operators')).toBeTruthy()
    expect(screen.queryByLabelText('Band category')).toBeNull()
    expect(screen.queryByLabelText('Entry class')).toBeNull()
    next()
    // The station is on FT8 (the snapshot's digital tier).
    const warn = document.querySelector('[data-banned]')!
    expect(warn.textContent).toContain(
      'ARRL RTTY Roundup does not allow FT8: contacts in it are logged but score nothing. Work it in RTTY instead.',
    )
    const ways = within(warn as HTMLElement).getAllByRole('button').map((b) => b.textContent)
    expect(ways).toEqual(['Work this contest in RTTY'])
    expect(screen.getByRole('button', { name: 'Next' })).toHaveProperty('disabled', false)
    next()
    expect(screen.getByRole('button', { name: 'Start ARRL RTTY Roundup' })).toHaveProperty('disabled', false)
  })

  it('the ARRL RTTY Roundup on CW and on PSK31 warns too, and on RTTY it does not', async () => {
    for (const [operatingMode, banned, keyboardMode] of [
      ['cw', 'CW', undefined],
      ['keyboard', 'PSK31', 'PSK31'],
      // PSK and JTTY are one section: on the JTTY screen the warning names JTTY, never PSK31.
      ['keyboard', 'JTTY', 'JTTY'],
      ['keyboard', 'QPSK31', 'QPSK31'],
      ['rtty', null, undefined],
    ] as const) {
      api.get('getSnapshot').mockImplementation(() =>
        Promise.resolve({ fieldDay: null, mode: 'chat', radio: { catOk: true, operatingMode, keyboardMode }, link: { tier: 'FT8' } }),
      )
      await open({ initial: { fdEvent: 'arrlrtty' } })
      next()
      await screen.findByText('You send: 599 IL')
      next()
      next()
      const warn = document.querySelector('[data-banned]')
      if (banned) expect(warn?.textContent, operatingMode).toContain(`does not allow ${banned}`)
      else expect(warn, operatingMode).toBeNull()
      expect(screen.getByRole('button', { name: 'Next' })).toHaveProperty('disabled', false)
      cleanup()
    }
  })

  it('Cancel saves nothing', async () => {
    const { onClose } = await open()
    fireEvent.click(screen.getByRole('radio', { name: /ARRL Field Day/ }))
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    expect(onClose).toHaveBeenCalled()
    expect(writes()).toEqual([])
  })
})

describe('Start', () => {
  async function toStart() {
    await open()
    next()
    fireEvent.change(await screen.findByLabelText('County'), { target: { value: 'COOK' } })
    await screen.findByText('You send: 599 COOK')
    next()
    next()
    next()
    fireEvent.click(screen.getByRole('button', { name: 'Start Illinois QSO Party' }))
  }

  it('is ONE save through the patch seam, with Contest mode on, and says the contest is on in S&P once the engine entered it', async () => {
    await toStart()
    expect(await screen.findByText('Illinois QSO Party is on · S&P')).toBeTruthy()
    expect(writes()).toHaveLength(1)
    // Over the settings the backend held at save time (the patch seam), with the answers and the mode.
    expect(writes()[0]).toMatchObject({ mycall: 'K9ABC', fdEvent: 'ilqp', contestQthCounty: 'COOK', fdActive: true })
    expect(api.get('getSettings').mock.calls.length).toBeGreaterThanOrEqual(2)
  })

  it('when the engine did not enter, says why in its own words and turns Contest mode off again', async () => {
    snapAfterSave = { mode: 'chat', fieldDay: null }
    api.get('getFdRuleset').mockImplementation(() =>
      Promise.resolve({ ...preview({}), problem: 'Your COUNTY is not one the Illinois QSO Party lists.' }),
    )
    const before = structuredClone(station)
    await toStart()
    expect(await screen.findByRole('alert')).toHaveProperty(
      'textContent',
      'The station did not enter Illinois QSO Party: Your COUNTY is not one the Illinois QSO Party lists. Contest mode is off again.',
    )
    await waitFor(() => expect(writes()).toHaveLength(2))
    expect(writes()[1].fdActive).toBe(false)
    // Everything the Start saved is put back, by value: the county it typed too.
    expect(station).toEqual(before)
    expect(screen.queryByText('Illinois QSO Party is on · S&P')).toBeNull()
    // Back on the exchange, where it can be fixed.
    expect(screen.getByRole('heading', { name: 'What Illinois QSO Party sends' })).toBeTruthy()
  })

  it('entered the WRONG contest is not entered', () => {
    expect(enteredContest({ mode: 'fieldDay', fieldDay: { event: 'ilqp' } } as AppSnapshot, 'ilqp')).toBe(true)
    expect(enteredContest({ mode: 'fieldDay', fieldDay: { event: 'nyqp' } } as AppSnapshot, 'ilqp')).toBe(false)
    expect(enteredContest({ mode: 'chat', fieldDay: null } as unknown as AppSnapshot, 'ilqp')).toBe(false)
    expect(enteredContest({ mode: 'fieldDay', fieldDay: { event: '' } } as AppSnapshot, 'arrlfd')).toBe(true)
  })

  it('a contest already running: a different pick asks for a tick before it goes on, and Start never runs set_mode', async () => {
    api.get('getSnapshot').mockImplementation(() =>
      Promise.resolve({ fieldDay: { event: 'nyqp' }, mode: 'fieldDay', radio: { catOk: true }, link: { tier: 'FT8' } }),
    )
    await open()
    await screen.findByText(/New York QSO Party is running now/)
    expect(screen.getByRole('button', { name: 'Next' })).toHaveProperty('disabled', true)
    fireEvent.click(screen.getByRole('checkbox', { name: 'End New York QSO Party when I start Illinois QSO Party' }))
    expect(screen.getByRole('button', { name: 'Next' })).toHaveProperty('disabled', false)
    expect(api.get('setMode')).not.toHaveBeenCalled()
  })
})

describe('Start saves only what it showed, and a refusal puts all of it back', () => {
  it('answers handed over but never shown for the picked contest stay unsaved', async () => {
    // Settings' form held a call and a grid it had not saved; the Illinois QSO Party shows neither.
    await open({ initial: { fdEvent: 'ilqp', mycall: 'N0NEW', mygrid: 'EN99', contestQthCounty: 'COOK' } })
    const before = structuredClone(station)
    next()
    await screen.findByText('You send: 599 COOK')
    next()
    next()
    next()
    fireEvent.click(screen.getByRole('button', { name: 'Start Illinois QSO Party' }))
    await screen.findByText('Illinois QSO Party is on · S&P')
    expect(writes()).toHaveLength(1)
    // Exactly the shown answers and the mode over what the station held: the call and grid are not.
    expect(writes()[0]).toEqual({ ...before, fdEvent: 'ilqp', contestQthCounty: 'COOK', fdActive: true })
  })

  it('a refused switch puts back the answers, the contest it ended and the mode, exactly', async () => {
    station = { ...station, fdActive: true, fdEvent: 'nyqp', contestQthCounty: '' }
    const before = structuredClone(station)
    api.get('getSnapshot').mockImplementation(() =>
      Promise.resolve({ fieldDay: { event: 'nyqp' }, mode: 'fieldDay', radio: { catOk: true }, link: { tier: 'FT8' } }),
    )
    // The Start's save: nothing entered. The put-back's: New York QSO Party again.
    api.get('setSettings')
      .mockImplementationOnce((s: Partial<Settings>) => {
        station = structuredClone(s)
        return Promise.resolve({ mode: 'chat', fieldDay: null })
      })
      .mockImplementationOnce((s: Partial<Settings>) => {
        station = structuredClone(s)
        return Promise.resolve({ mode: 'fieldDay', fieldDay: { event: 'nyqp' } })
      })
    api.get('getFdRuleset').mockImplementation(() =>
      Promise.resolve({ ...preview({}), problem: 'Your COUNTY is not one the Illinois QSO Party lists.' }),
    )
    await open()
    fireEvent.click(screen.getByRole('radio', { name: /Illinois QSO Party/ }))
    fireEvent.click(screen.getByRole('checkbox', { name: 'End New York QSO Party when I start Illinois QSO Party' }))
    next()
    fireEvent.change(await screen.findByLabelText('County'), { target: { value: 'COOK' } })
    await screen.findByText('You send: 599 COOK')
    next()
    next()
    next()
    fireEvent.click(screen.getByRole('button', { name: 'Start Illinois QSO Party' }))
    expect(await screen.findByRole('alert')).toHaveProperty(
      'textContent',
      'The station did not enter Illinois QSO Party: Your COUNTY is not one the Illinois QSO Party lists. Your answers were put back, and New York QSO Party is on again.',
    )
    expect(writes()).toHaveLength(2)
    expect(writes()[1]).toEqual(before)
    expect(station).toEqual(before)
  })

  it('a put-back that fails says what may still be saved', async () => {
    snapAfterSave = { mode: 'chat', fieldDay: null }
    api.get('setSettings')
      .mockImplementationOnce((s: Partial<Settings>) => {
        station = structuredClone(s)
        return Promise.resolve(snapAfterSave)
      })
      .mockImplementationOnce(() => Promise.reject('disk full'))
    await open()
    next()
    fireEvent.change(await screen.findByLabelText('County'), { target: { value: 'COOK' } })
    await screen.findByText('You send: 599 COOK')
    next()
    next()
    next()
    fireEvent.click(screen.getByRole('button', { name: 'Start Illinois QSO Party' }))
    expect(await screen.findByRole('alert')).toHaveProperty(
      'textContent',
      'The station did not enter Illinois QSO Party, and putting the earlier settings back failed: disk full. Contest mode may still be saved on: check Settings ▸ Contesting.',
    )
  })
})

describe('Change setup… on a running contest', () => {
  /** The Illinois QSO Party running in S&P from Cook County, an in-state station whose rules let it move. */
  const RUNNING = {
    event: 'ilqp',
    running: false,
    role: 'in_state',
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'QTH', raw: 'COOK', domain: 'il_counties' },
    ],
    composingText: '599 COOK',
    dupeRule: { byCall: true, byBand: true, byModeClass: true, byFields: [], bySentFields: ['QTH'], modeClassGroups: [] },
  }
  beforeEach(() => {
    station = { ...station, fdActive: true, contestQthCounty: 'COOK' }
    api.get('getSnapshot').mockImplementation(() =>
      Promise.resolve({ fieldDay: RUNNING, mode: 'fieldDay', radio: { catOk: true, operatingMode: 'digital' }, link: { tier: 'FT8' } }),
    )
    api.get('contestIMoved').mockImplementation(() =>
      Promise.resolve({ mode: 'fieldDay', fieldDay: { ...RUNNING, composingText: '599 WILL' } }),
    )
  })
  async function openChange() {
    render(<ContestSetupWizard onClose={vi.fn()} startAt="sends" />)
    await screen.findByRole('dialog', { name: 'Change contest setup' })
  }

  it('a new county goes on the air through I moved, and You send is what the engine sends', async () => {
    await openChange()
    const county = (await screen.findByLabelText('County')) as HTMLInputElement
    await screen.findByText('You send: 599 COOK')
    fireEvent.change(county, { target: { value: 'will' } })
    await waitFor(() => expect(api.get('previewFdRuleset')).toHaveBeenLastCalledWith({ contestQthCounty: 'WILL' }))
    await nextWhenChecked()
    // Not sent yet, so not shown as sent.
    expect(screen.queryByText('You send: 599 WILL')).toBeNull()
    next()
    next()
    expect(screen.getByText('You send: 599 COOK')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }))
    // The engine's own answer, from the snapshot the move returned.
    await screen.findByText('You send: 599 WILL')
    expect(api.get('contestIMoved')).toHaveBeenCalledWith([['QTH', 'WILL']])
    // The move writes the setting itself: no settings save beside it.
    expect(writes()).toEqual([])
    expect(api.get('setMode')).not.toHaveBeenCalled()
  })

  it('a field read at start is shown read-only with the live exchange, and nothing saves it', async () => {
    await openChange()
    await screen.findByText('You send: 599 COOK')
    // An in-state station moves by county; the state is not a box here.
    expect(screen.queryByLabelText('State or province')).toBeNull()
    expect(
      screen.getByText('Read when the contest starts: State or province. To change it, stop the contest, then start it again.'),
    ).toBeTruthy()
    await nextWhenChecked()
    next()
    next()
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }))
    await screen.findByText('Illinois QSO Party is on · S&P')
    // Nothing changed, so nothing was written and nothing moved.
    expect(writes()).toEqual([])
    expect(api.get('contestIMoved')).not.toHaveBeenCalled()
  })

  it('a move the engine refuses saves nothing and says why in its words', async () => {
    api.get('contestIMoved').mockImplementation(() => Promise.reject('"XXXX" is not a il_counties value'))
    await openChange()
    fireEvent.change(await screen.findByLabelText('County'), { target: { value: 'XXXX' } })
    await waitFor(() => expect(api.get('previewFdRuleset')).toHaveBeenLastCalledWith({ contestQthCounty: 'XXXX' }))
    await nextWhenChecked()
    next()
    next()
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }))
    expect(await screen.findByRole('alert')).toHaveProperty('textContent', '"XXXX" is not a il_counties value')
    expect(writes()).toEqual([])
  })
})

describe('the station changed while the setup was open', () => {
  const RUNNING = {
    event: 'ilqp',
    running: false,
    role: 'in_state',
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'QTH', raw: 'COOK', domain: 'il_counties' },
    ],
    composingText: '599 COOK',
    dupeRule: { byCall: true, byBand: true, byModeClass: true, byFields: [], bySentFields: ['QTH'], modeClassGroups: [] },
  }
  const snapWith = (fieldDay: unknown) => ({
    fieldDay,
    mode: fieldDay ? 'fieldDay' : 'chat',
    radio: { catOk: true, operatingMode: 'digital' },
    link: { tier: 'FT8' },
  })

  it('Save changes after the contest was stopped elsewhere says it is not running, and shows no exchange', async () => {
    station = { ...station, fdActive: true, contestQthCounty: 'COOK' }
    api.get('getSnapshot').mockImplementation(() => Promise.resolve(snapWith(RUNNING)))
    // The entry line's save: the contest was stopped from another surface in the meantime.
    api.get('setSettings').mockImplementation((s: Partial<Settings>) => {
      station = structuredClone(s)
      return Promise.resolve({ mode: 'chat', fieldDay: null })
    })
    render(<ContestSetupWizard onClose={vi.fn()} startAt="sends" />)
    await screen.findByText('You send: 599 COOK')
    await nextWhenChecked()
    fireEvent.change(screen.getByLabelText('Club'), { target: { value: 'Rock River ARC' } })
    await nextWhenChecked()
    next()
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }))
    expect(await screen.findByText('Illinois QSO Party is not running now. It was stopped while this setup was open.')).toBeTruthy()
    expect(writes()).toEqual([{ ...station, contestClub: 'Rock River ARC' }])
    expect(screen.queryByText(/You send:/)).toBeNull()
    expect(screen.queryByText('Illinois QSO Party is on · S&P')).toBeNull()
  })

  it('Start never ends a contest started elsewhere: it stops, and shows the End choice', async () => {
    station = { ...station, contestQthCounty: 'COOK' }
    api.get('getSnapshot')
      .mockImplementationOnce(() => Promise.resolve(snapWith(null)))
      .mockImplementation(() => Promise.resolve(snapWith({ event: 'nyqp' })))
    await open()
    next()
    await screen.findByText('You send: 599 COOK')
    next()
    next()
    next()
    fireEvent.click(screen.getByRole('button', { name: 'Start Illinois QSO Party' }))
    expect(await screen.findByRole('alert')).toHaveProperty(
      'textContent',
      'The running contest changed while this setup was open. Check it here, then go on.',
    )
    // Nothing saved, nothing ended: back on the pick, with the tick unticked.
    expect(writes()).toEqual([])
    const tick = screen.getByRole('checkbox', { name: 'End New York QSO Party when I start Illinois QSO Party' }) as HTMLInputElement
    expect(tick.checked).toBe(false)
    expect(screen.getByRole('button', { name: 'Next' })).toHaveProperty('disabled', true)
    // CONTROL: with the tick, the same Start goes ahead.
    fireEvent.click(tick)
    next()
    await screen.findByText('You send: 599 COOK')
    next()
    next()
    next()
    fireEvent.click(screen.getByRole('button', { name: 'Start Illinois QSO Party' }))
    await screen.findByText('Illinois QSO Party is on · S&P')
    expect(writes()).toEqual([{ ...station }])
    expect(writes()[0]).toMatchObject({ fdEvent: 'ilqp', contestQthCounty: 'COOK', fdActive: true })
    expect(api.get('setMode')).not.toHaveBeenCalled()
  })
})

describe('Change setup… on other contests', () => {
  const snapWith = (fieldDay: unknown) => ({
    fieldDay,
    mode: 'fieldDay',
    radio: { catOk: true, operatingMode: 'digital' },
    link: { tier: 'FT8' },
  })

  it('Field Day: a new class goes through I moved as CLASS, and nothing else is written', async () => {
    station = { ...station, fdActive: true, fdEvent: 'arrlfd', fdClass: '1D', fdSection: 'WI' }
    const FD = {
      event: 'arrlfd',
      running: false,
      composing: [
        { key: 'CLASS', raw: '1D' },
        { key: 'SECTION', raw: 'WI' },
      ],
      composingText: '1D WI',
    }
    api.get('getSnapshot').mockImplementation(() => Promise.resolve(snapWith(FD)))
    api.get('contestIMoved').mockImplementation(() => Promise.resolve({ mode: 'fieldDay', fieldDay: { ...FD, composingText: '2A WI' } }))
    render(<ContestSetupWizard onClose={vi.fn()} startAt="sends" />)
    await screen.findByText('You send: 1D WI')
    expect(screen.getByText('A change here is "I moved": it takes effect on your next contact.')).toBeTruthy()
    fireEvent.change(screen.getByLabelText('FD Class'), { target: { value: '2a' } })
    await nextWhenChecked()
    next()
    next()
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }))
    await screen.findByText('You send: 2A WI')
    expect(api.get('contestIMoved')).toHaveBeenCalledWith([['CLASS', '2A']])
    expect(writes()).toEqual([])
  })

  it('an out-of-state station moves by state, and County is not named as read at start', async () => {
    station = { ...station, fdActive: true, contestQthState: 'IN', contestQthCounty: '' }
    const OUT = {
      event: 'ilqp',
      running: false,
      role: 'out_of_state',
      composing: [
        { key: 'RST', raw: '599' },
        { key: 'QTH', raw: 'IN' },
      ],
      composingText: '599 IN',
      dupeRule: { byCall: true, byBand: true, byModeClass: true, byFields: [], bySentFields: ['QTH'], modeClassGroups: [] },
    }
    api.get('getSnapshot').mockImplementation(() => Promise.resolve(snapWith(OUT)))
    api.get('contestIMoved').mockImplementation(() => Promise.resolve({ mode: 'fieldDay', fieldDay: { ...OUT, composingText: '599 WI' } }))
    render(<ContestSetupWizard onClose={vi.fn()} startAt="sends" />)
    await screen.findByText('You send: 599 IN')
    expect(screen.queryByLabelText('County')).toBeNull()
    expect(document.querySelector('[data-read-at-start]')).toBeNull()
    fireEvent.change(screen.getByLabelText('State or province'), { target: { value: 'wi' } })
    await nextWhenChecked()
    next()
    next()
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }))
    await screen.findByText('You send: 599 WI')
    expect(api.get('contestIMoved')).toHaveBeenCalledWith([['QTH', 'WI']])
    expect(writes()).toEqual([])
  })

  it('VHF: the grid is read at start, so no "I moved" hint and no move, even for an answer a host handed over', async () => {
    station = { ...station, fdActive: true, fdEvent: 'arrlvhf_jan', mygrid: 'EN52' }
    const VHF = {
      event: 'arrlvhf_jan',
      running: false,
      role: '',
      composing: [{ key: 'GRID', raw: 'EN52' }],
      composingText: 'EN52',
      dupeRule: { byCall: true, byBand: true, byModeClass: true, byFields: [], bySentFields: ['GRID'], modeClassGroups: [] },
    }
    api.get('getSnapshot').mockImplementation(() => Promise.resolve(snapWith(VHF)))
    render(<ContestSetupWizard onClose={vi.fn()} startAt="sends" initial={{ contestQthState: 'WI' }} />)
    await screen.findByText('You send: EN52')
    expect(screen.queryByText('A change here is "I moved": it takes effect on your next contact.')).toBeNull()
    expect(screen.queryByRole('textbox')).toBeNull()
    // …and the ARRL section the file is headed with (LOCATION), read at start as the grid is.
    expect(document.querySelector('[data-read-at-start]')!.textContent).toBe(
      'Read when the contest starts: Grid square, ARRL Section. To change them, stop the contest, then start it again.',
    )
    await nextWhenChecked()
    next()
    next()
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }))
    await screen.findByText('ARRL January VHF Contest is on · S&P')
    expect(api.get('contestIMoved')).not.toHaveBeenCalled()
    expect(writes()).toEqual([])
  })

  it('a move that took effect, then a failed save of the entry lines, says the move is on and hands the move to the host', async () => {
    station = { ...station, fdActive: true, contestQthCounty: 'COOK' }
    const RUNNING = {
      event: 'ilqp',
      running: false,
      role: 'in_state',
      composing: [
        { key: 'RST', raw: '599' },
        { key: 'QTH', raw: 'COOK' },
      ],
      composingText: '599 COOK',
      dupeRule: { byCall: true, byBand: true, byModeClass: true, byFields: [], bySentFields: ['QTH'], modeClassGroups: [] },
    }
    api.get('getSnapshot').mockImplementation(() => Promise.resolve(snapWith(RUNNING)))
    api.get('contestIMoved').mockImplementation(() => Promise.resolve({ mode: 'fieldDay', fieldDay: { ...RUNNING, composingText: '599 WILL' } }))
    api.get('setSettings').mockImplementation(() => Promise.reject('disk full'))
    const onSaved = vi.fn()
    render(<ContestSetupWizard onClose={vi.fn()} startAt="sends" onSaved={onSaved} />)
    await screen.findByText('You send: 599 COOK')
    fireEvent.change(screen.getByLabelText('County'), { target: { value: 'will' } })
    await nextWhenChecked()
    fireEvent.change(screen.getByLabelText('Club'), { target: { value: 'Rock River ARC' } })
    await nextWhenChecked()
    next()
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }))
    expect(await screen.findByRole('alert')).toHaveProperty(
      'textContent',
      'Your move took effect: you send 599 WILL. Saving the other changes failed: disk full.',
    )
    expect(onSaved.mock.calls).toEqual([[{ contestQthCounty: 'WILL' }]])
  })
})

describe('the smaller cases', () => {
  it('a refused Start when Contest mode was already saved on does not say it is off again', async () => {
    station = { ...station, fdActive: true }
    snapAfterSave = { mode: 'chat', fieldDay: null }
    api.get('getFdRuleset').mockImplementation(() =>
      Promise.resolve({ ...preview({}), problem: 'Your COUNTY is not one the Illinois QSO Party lists.' }),
    )
    const before = structuredClone(station)
    await open()
    next()
    fireEvent.change(await screen.findByLabelText('County'), { target: { value: 'COOK' } })
    await screen.findByText('You send: 599 COOK')
    next()
    next()
    next()
    fireEvent.click(screen.getByRole('button', { name: 'Start Illinois QSO Party' }))
    expect(await screen.findByRole('alert')).toHaveProperty(
      'textContent',
      'The station did not enter Illinois QSO Party: Your COUNTY is not one the Illinois QSO Party lists. Your answers were put back. Contest mode was already saved on, so the station tries again at the next save.',
    )
    expect(writes()[1]).toEqual(before)
  })

  it('a preview that failed holds Next, and says so, rather than dropping the typed class', async () => {
    station = { ...station, fdEvent: 'arrlfd', fdClass: '1D', fdSection: 'WI' }
    await open()
    next()
    await screen.findByText('You send: 1D WI')
    api.get('previewFdRuleset').mockImplementation(() => Promise.reject('no engine'))
    fireEvent.change(screen.getByLabelText('FD Class'), { target: { value: '2A' } })
    expect(await screen.findByText('The station could not check this exchange. Change a field to try again.')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Next' })).toHaveProperty('disabled', true)
    expect(writes()).toEqual([])
  })
})

describe('Start fails closed', () => {
  it('when the station cannot say what is running, Start saves nothing, says why, and can be pressed again', async () => {
    station = { ...station, contestQthCounty: 'COOK' }
    const snap = { fieldDay: null, mode: 'chat', radio: { catOk: true, operatingMode: 'digital' }, link: { tier: 'FT8' } }
    api.get('getSnapshot')
      .mockImplementationOnce(() => Promise.resolve(snap))
      .mockImplementationOnce(() => Promise.reject('no engine'))
      .mockImplementation(() => Promise.resolve(snap))
    await open()
    next()
    await screen.findByText('You send: 599 COOK')
    next()
    next()
    next()
    fireEvent.click(screen.getByRole('button', { name: 'Start Illinois QSO Party' }))
    expect(await screen.findByRole('alert')).toHaveProperty(
      'textContent',
      'Nothing was saved: the station could not say whether a contest is running now. Try again.',
    )
    expect(writes()).toEqual([])
    // Still on Start, and Start can be pressed again.
    const again = screen.getByRole('button', { name: 'Start Illinois QSO Party' })
    expect(again).toHaveProperty('disabled', false)
    // CONTROL: once the station answers, the same Start goes ahead.
    fireEvent.click(again)
    await screen.findByText('Illinois QSO Party is on · S&P')
    expect(writes()).toEqual([{ ...station }])
    expect(writes()[0]).toMatchObject({ fdEvent: 'ilqp', contestQthCounty: 'COOK', fdActive: true })
  })
})
