// @vitest-environment jsdom
//
// ⭐ BUILD YOUR OWN CONTEST, in the contest setup: the builder's steps (a required box empty refuses
// Next and says which), Save installs it and comes back to the pick with it picked, Edit / Export… /
// Delete on the operator's own contests, Import… with the Replace-or-Keep-both choice, the FT8/FT4
// line saying what this build does (standard messages), the RTTY Roundup question defaulting to No,
// and the typed values (an age) asked on the exchange step into `contestEntered`.
//
// The station's commands are mocked as the station answers them (src-tauri's
// `preview_custom_contest`, `save_custom_contest`, …); the rules themselves are Rust's and pinned
// there. These tests pin what the screen does with the answers.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor, within } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { ContestSetupWizard } from './ContestSetupWizard'
import type { AppSnapshot, Settings } from '../types'
import type { CustomContest, CustomContestDef, CustomContestPreview, FdRulesetDto } from '../api'
import { enteredTyped, setCustomContests } from '../features/customContests'
import { contestName, contestClubRefusal, clubSyncRefusalText } from '../fdEvent'
import { chainOf, parseRules, winnerAt } from '../cssCascade'

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

const NOW = Date.UTC(2026, 9, 21, 12) / 1000

/** A club sprint with an age in its exchange, as the station lists it. */
const SPRINT: CustomContestDef = {
  format: 'nexus-custom-contest',
  version: 1,
  id: 'custom_club_sprint',
  name: 'Club Sprint',
  short: 'Sprint',
  cabrillo_contest: 'CLUB-SPRINT',
  modes: ['CW'],
  window: { kind: 'weekend', month: 3, weekend: 'nth_full', n: 1, start_hour_utc: 14, duration_hours: 4 },
  exchange: [{ field: 'rst' }, { field: 'age' }],
  dupe: 'band',
  points: { CW: 1, PH: 1, DIG: 1 },
}
const listed = (def: CustomContestDef, exchangeLocked = false): CustomContest => ({
  def,
  enteredSlots: def.exchange.some((f) => f.field === 'age') ? ['AGE'] : [],
  sentSettings: [],
  exchangeLocked,
})

let station: Partial<Settings> = {}
/** The contests installed on the station. */
let installed: CustomContest[] = []

/** The station's preview of a definition: its refusal verbatim, else what it would send, a sample
 *  line, and the FT8/FT4 answer — standard messages for every custom contest in this build. */
function previewOf(def: CustomContestDef): CustomContestPreview {
  const blank = { youSend: '', startProblem: '', sampleQsoLine: '', ftShape: null, ftActivity: null, ftMessages: '', enteredSlots: [] }
  if (def.exchange.filter((f) => f.field === 'rst').length > 1) return { ...blank, refusal: 'the exchange carries rst twice' }
  const ft = def.modes.includes('FT8') || def.modes.includes('FT4')
  const roundup = ft && def.exchange.map((f) => f.field).join() === 'rst,serial'
  return {
    ...blank,
    refusal: '',
    youSend: def.exchange.map((f) => (f.field === 'rst' ? '599' : f.field === 'serial' ? '1' : '?')).join(' '),
    sampleQsoLine: `QSO:  7025 CW 2026-03-07 1400 K9ABC 599 1 W1AW 599 1`,
    ftShape: roundup ? 'rtty_roundup' : null,
    ftActivity: roundup && def.ft_roundup ? 'rtty_roundup' : null,
    ftMessages: 'standard',
  }
}

/** The contest preview for the pick: a custom contest sends RST and the typed age. */
function fdPreview(patch: Partial<Settings>): FdRulesetDto {
  const s = { ...station, ...patch }
  const event = s.fdEvent || 'arrlfd'
  const base: FdRulesetDto = { event, rulesYear: 2026, bannedModes: [], spottingAllowed: true, clusterAllowed: true, enforcement: 'warn', role: '' }
  if (event === SPRINT.id) {
    const age = s.contestEntered?.[event]?.AGE ?? ''
    return {
      ...base,
      exchange: age ? ['599', age] : [],
      problem: age ? '' : 'Your AGE is empty — it is part of the exchange you transmit. Fill it in when you set up the contest.',
      sentSettings: [],
      enteredSlots: ['AGE'],
    }
  }
  return { ...base, exchange: [s.fdClass ?? '', s.fdSection ?? ''], problem: '', sentSettings: ['fd_class', 'fd_section'] }
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'], now: NOW * 1000 })
  for (const spy of Object.values(api.spies)) {
    spy.mockReset()
    spy.mockImplementation(() => Promise.resolve(null))
  }
  setCustomContests([])
  installed = []
  station = { mycall: 'K9ABC', fdActive: false, fdEvent: 'arrlfd', fdClass: '1D', fdSection: 'WI', contestEntered: {} }
  api.get('getSettings').mockImplementation(() => Promise.resolve(structuredClone(station)))
  api.get('setSettings').mockImplementation((s: Partial<Settings>) => {
    station = structuredClone(s)
    return Promise.resolve({ mode: 'fieldDay', fieldDay: { event: s.fdEvent, running: false } } as unknown as AppSnapshot)
  })
  api.get('getSnapshot').mockImplementation(() => Promise.resolve({ fieldDay: null, mode: 'chat', radio: { catOk: true } }))
  api.get('contestWindows').mockImplementation(() => Promise.resolve([]))
  api.get('previewFdRuleset').mockImplementation((patch: Partial<Settings>) => Promise.resolve(fdPreview(patch)))
  api.get('getFdRuleset').mockImplementation(() => Promise.resolve(fdPreview({})))
  api.get('listCustomContests').mockImplementation(() => Promise.resolve(structuredClone(installed)))
  api.get('previewCustomContest').mockImplementation((def: CustomContestDef) => Promise.resolve(previewOf(def)))
  api.get('saveCustomContest').mockImplementation((def: CustomContestDef) => {
    installed = [...installed.filter((c) => c.def.id !== def.id), listed(def)]
    return Promise.resolve()
  })
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

async function open(props: Partial<Parameters<typeof ContestSetupWizard>[0]> = {}) {
  render(<ContestSetupWizard onClose={() => {}} {...props} />)
  await screen.findByRole('dialog', { name: 'Set up a contest' })
  return screen.findByRole('radiogroup', { name: 'Your contests' })
}
const button = (name: string | RegExp) => screen.getByRole('button', { name })
const nextButton = () => button('Next') as HTMLButtonElement
const typeIn = (label: string, value: string) => fireEvent.change(screen.getByLabelText(label), { target: { value } })
const saved = () => api.get('saveCustomContest').mock.calls.map((c) => c[0] as CustomContestDef)

/** Build your own…, the Basics filled in. */
async function startBuilding(modes: string[] = ['CW']) {
  await open()
  fireEvent.click(button('Build your own…'))
  await screen.findByRole('dialog', { name: 'Build your own contest' })
  typeIn('Contest name', 'Club Sprint')
  typeIn('Cabrillo contest name', 'club-sprint')
  for (const m of ['CW', 'SSB', 'FM', 'RTTY', 'PSK', 'FT8', 'FT4']) {
    const box = screen.getByRole('checkbox', { name: m }) as HTMLInputElement
    if (box.checked !== modes.includes(m)) fireEvent.click(box)
  }
}

describe('build your own contest', () => {
  it('⭐ a contest built step by step is saved, then picked in Your contests', async () => {
    await startBuilding()
    fireEvent.click(nextButton()) // → Exchange: RST and serial to start from
    expect(screen.getAllByRole('combobox', { name: /^Field \d$/ }).map((s) => (s as HTMLSelectElement).value)).toEqual(['rst', 'serial'])
    fireEvent.click(nextButton()) // → Rules
    fireEvent.click(screen.getByRole('radio', { name: 'Once per band and mode' }))
    fireEvent.change(screen.getByLabelText('Phone'), { target: { value: '2' } })
    fireEvent.click(nextButton()) // → Check: the station's preview
    expect(await screen.findByText('You send: 599 1')).toBeTruthy()
    expect(screen.getByText(/^QSO: 7025 CW 2026-03-07/)).toBeTruthy()
    fireEvent.click(button('Save contest'))

    const group = await screen.findByRole('radiogroup', { name: 'Your contests' })
    const pick = within(group).getByRole('radio', { name: 'Club Sprint' })
    expect(pick.getAttribute('aria-checked')).toBe('true')
    expect(saved()).toEqual([
      {
        ...SPRINT,
        exchange: [{ field: 'rst' }, { field: 'serial' }],
        window: { kind: 'weekend', month: 1, weekend: 'nth_full', n: 1, start_hour_utc: 0, duration_hours: 24 },
        short: '',
        dupe: 'band_mode',
        points: { CW: 1, PH: 2, DIG: 1 },
      },
    ])
    // Its name is the operator's own wherever a contest is named.
    expect(contestName('custom_club_sprint')).toBe('Club Sprint')
  })

  it('refuses Next with the reason while a required box is empty', async () => {
    await open()
    fireEvent.click(button('Build your own…'))
    await screen.findByRole('dialog', { name: 'Build your own contest' })
    expect(nextButton().disabled).toBe(true)
    expect(screen.getByRole('alert').textContent).toBe('Give the contest a name.')
    typeIn('Contest name', 'Club Sprint')
    expect(nextButton().disabled).toBe(true)
    expect(screen.getByRole('alert').textContent).toBe('Give the Cabrillo contest name: 1 to 32 capital letters, digits or -.')
    typeIn('Cabrillo contest name', 'CLUB SPRINT')
    expect(nextButton().disabled, 'a space is not in a Cabrillo name').toBe(true)
    typeIn('Cabrillo contest name', 'CLUB-SPRINT')
    fireEvent.click(screen.getByRole('checkbox', { name: 'CW' }))
    expect(screen.getByRole('alert').textContent).toBe('Tick at least one mode.')
    fireEvent.click(screen.getByRole('checkbox', { name: 'SSB' }))
    // A one-date contest needs its date.
    fireEvent.click(screen.getByRole('radio', { name: 'One date' }))
    expect(screen.getByRole('alert').textContent).toBe('Pick the date and the start time.')
    typeIn('Date (UTC)', '2026-12-05')
    typeIn('Start time (UTC)', '14:00')
    expect(screen.queryByRole('alert')).toBeNull()
    expect(nextButton().disabled).toBe(false)
    // The exchange step refuses an empty exchange.
    fireEvent.click(nextButton())
    fireEvent.click(screen.getAllByRole('button', { name: 'Remove' })[0])
    fireEvent.click(screen.getAllByRole('button', { name: 'Remove' })[0])
    expect(nextButton().disabled).toBe(true)
    expect(screen.getByRole('alert').textContent).toBe('Add at least one field to the exchange.')
  })

  it('caps the exchange at five fields', async () => {
    await startBuilding()
    fireEvent.click(nextButton())
    const add = button('Add a field') as HTMLButtonElement
    fireEvent.click(add)
    fireEvent.click(add)
    fireEvent.click(add)
    expect(screen.getAllByRole('combobox', { name: /^Field \d$/ })).toHaveLength(5)
    expect(add.disabled).toBe(true)
    expect(screen.getByText('An exchange has at most 5 fields.')).toBeTruthy()
  })

  it("shows the station's refusal verbatim, and Save waits for a definition the station takes", async () => {
    await startBuilding()
    fireEvent.click(nextButton())
    fireEvent.change(screen.getAllByRole('combobox', { name: /^Field \d$/ })[1], { target: { value: 'rst' } })
    fireEvent.click(nextButton())
    fireEvent.click(nextButton())
    const refusal = await screen.findByText('the exchange carries rst twice')
    expect(refusal.closest('[data-refusal]')).toBeTruthy()
    expect((button('Save contest') as HTMLButtonElement).disabled).toBe(true)
  })

  it('⭐ asks whether to use RTTY Roundup messages on FT8 for RST + serial, No by default, and says FT8 sends standard messages', async () => {
    await startBuilding(['CW', 'FT8'])
    fireEvent.click(nextButton())
    const ask = screen.getByRole('radiogroup', { name: 'Use RTTY Roundup messages on FT8 (CQ RU)?' })
    expect((within(ask).getByRole('radio', { name: 'No: standard messages' }) as HTMLInputElement).checked).toBe(true)
    expect((within(ask).getByRole('radio', { name: 'Yes: RTTY Roundup messages' }) as HTMLInputElement).checked).toBe(false)
    // What this build sends: standard messages, the exchange not sent.
    await waitFor(() =>
      expect(document.querySelector('[data-ft-line]')?.textContent).toBe(
        'FT8/FT4: Nexus sends standard messages (grid and signal report); your exchange is not sent.',
      ),
    )
    // Said Yes: the message is named, and still not sent by this build.
    fireEvent.click(within(ask).getByRole('radio', { name: 'Yes: RTTY Roundup messages' }))
    await waitFor(() => expect(document.querySelector('[data-ft-line]')?.textContent).toMatch(/matches WSJT-X's ARRL RTTY Roundup message, which this version of Nexus does not send yet/))
    fireEvent.click(within(ask).getByRole('radio', { name: 'No: standard messages' }))
    fireEvent.click(nextButton())
    fireEvent.click(nextButton())
    await screen.findByText('You send: 599 1')
    fireEvent.click(button('Save contest'))
    await screen.findByRole('radiogroup', { name: 'Your contests' })
    expect('ft_roundup' in saved()[0], 'No is not written').toBe(false)
  })

  it('a contest with no FT8 or FT4 asks nothing about FT8', async () => {
    await startBuilding(['CW'])
    fireEvent.click(nextButton())
    expect(screen.queryByRole('radiogroup', { name: /RTTY Roundup/ })).toBeNull()
    expect(document.querySelector('[data-ft-line]')).toBeNull()
  })
})

describe('your contests: edit, export, delete, import', () => {
  beforeEach(() => {
    installed = [listed(SPRINT)]
  })

  it('Edit opens the builder on it, and keeps its id', async () => {
    const group = await open()
    await within(group).findByRole('radio', { name: 'Club Sprint' })
    fireEvent.click(within(group).getByRole('button', { name: 'Edit…' }))
    await screen.findByRole('dialog', { name: 'Edit Club Sprint' })
    expect((screen.getByLabelText('Contest name') as HTMLInputElement).value).toBe('Club Sprint')
    typeIn('Contest name', 'Club Sprint Spring')
    fireEvent.click(nextButton())
    fireEvent.click(nextButton())
    fireEvent.click(nextButton())
    await screen.findByText(/^You send:/)
    fireEvent.click(button('Save contest'))
    await screen.findByRole('radiogroup', { name: 'Your contests' })
    expect(saved()[0]).toMatchObject({ id: 'custom_club_sprint', name: 'Club Sprint Spring' })
  })

  it('a contest with logged contacts keeps its exchange: the boxes are locked, and Save as a new one is offered', async () => {
    installed = [listed(SPRINT, true)]
    const group = await open()
    await within(group).findByRole('radio', { name: 'Club Sprint' })
    fireEvent.click(within(group).getByRole('button', { name: 'Edit…' }))
    await screen.findByRole('dialog', { name: 'Edit Club Sprint' })
    expect((screen.getByLabelText('Cabrillo contest name') as HTMLInputElement).disabled).toBe(true)
    fireEvent.click(button('Save as a new contest instead'))
    expect((screen.getByLabelText('Cabrillo contest name') as HTMLInputElement).disabled).toBe(false)
    // No longer an edit: the dialog says so.
    expect(screen.getByRole('dialog', { name: 'Build your own contest' })).toBeTruthy()
    typeIn('Cabrillo contest name', 'CLUB-SPRINT-2')
    fireEvent.click(nextButton())
    fireEvent.click(nextButton())
    fireEvent.click(nextButton())
    await screen.findByText(/^You send:/)
    fireEvent.click(button('Save contest'))
    await screen.findByRole('radiogroup', { name: 'Your contests' })
    // A new id beside the old one, which keeps its logged contacts.
    expect(saved()[0]).toMatchObject({ id: 'custom_club_sprint_2', cabrillo_contest: 'CLUB-SPRINT-2' })
  })

  it('⭐ Export saves the definition text where the operator picks, and it holds no typed value', async () => {
    station = { ...station, contestEntered: { custom_club_sprint: { AGE: '42' } } }
    const text = JSON.stringify(SPRINT, null, 2)
    api.get('exportCustomContest').mockImplementation(() => Promise.resolve(text))
    api.get('saveTextAs').mockImplementation(() => Promise.resolve('/home/op/Downloads/club_sprint.nexus-contest.json'))
    const group = await open()
    await within(group).findByRole('radio', { name: 'Club Sprint' })
    fireEvent.click(within(group).getByRole('button', { name: 'Export…' }))
    await screen.findByText('Saved Club Sprint to /home/op/Downloads/club_sprint.nexus-contest.json')
    expect(api.get('exportCustomContest')).toHaveBeenCalledWith('custom_club_sprint')
    const [name, written] = api.get('saveTextAs').mock.calls[0] as [string, string]
    expect(name).toBe('club_sprint.nexus-contest.json')
    expect(written).toBe(text)
    expect(written).not.toMatch(/42|contestEntered|contest_entered/)
  })

  it('Delete asks first, and says why the station refused it', async () => {
    api.get('deleteCustomContest').mockImplementation(() =>
      Promise.reject('Club Sprint is the picked contest. Pick another contest before deleting it.'),
    )
    const group = await open()
    await within(group).findByRole('radio', { name: 'Club Sprint' })
    fireEvent.click(within(group).getByRole('button', { name: 'Delete' }))
    expect(api.get('deleteCustomContest')).not.toHaveBeenCalled()
    expect(screen.getByText('Delete Club Sprint? Contacts already logged in it stay on this computer.')).toBeTruthy()
    fireEvent.click(button('Delete it'))
    expect(
      (await screen.findByText(/^Club Sprint was not deleted:/)).textContent,
    ).toBe('Club Sprint was not deleted: Club Sprint is the picked contest. Pick another contest before deleting it.')
    // Kept: it is still listed.
    expect(within(group).getByRole('radio', { name: 'Club Sprint' })).toBeTruthy()
  })

  it('⭐ an import of a contest already here asks: Replace it or Keep both', async () => {
    api.get('importCustomContest').mockImplementation((_text: string, how: string) => {
      if (how === 'ask') return Promise.reject('collision:Club Sprint')
      const def = { ...SPRINT, id: 'custom_club_sprint_2', cabrillo_contest: 'CLUB-SPRINT-2' }
      installed = [...installed, listed(def)]
      return Promise.resolve(def)
    })
    const group = await open()
    await within(group).findByRole('radio', { name: 'Club Sprint' })
    const file = new File([JSON.stringify(SPRINT)], 'club_sprint.json', { type: 'application/json' })
    fireEvent.change(document.querySelector('[data-import-file]')!, { target: { files: [file] } })
    const ask = await screen.findByText('You already have this contest: Club Sprint. Replace it with the file, or keep both?')
    const choice = ask.closest('[data-collision]') as HTMLElement
    expect(within(choice).getByRole('button', { name: 'Replace it' })).toBeTruthy()
    fireEvent.click(within(choice).getByRole('button', { name: 'Keep both' }))
    await screen.findByText('Imported Club Sprint, and picked it.')
    expect(api.get('importCustomContest').mock.calls.map((c) => c[1])).toEqual(['ask', 'keep_both'])
    expect(api.get('importCustomContest').mock.calls[1][0]).toBe(JSON.stringify(SPRINT))
    const picks = within(group).getAllByRole('radio', { name: 'Club Sprint' })
    expect(picks).toHaveLength(2)
    expect(picks[1].getAttribute('aria-checked')).toBe('true')
  })

  it('⭐ the exchange step asks for the typed age, and the age reaches the preview and the save', async () => {
    const group = await open()
    fireEvent.click(await within(group).findByRole('radio', { name: 'Club Sprint' }))
    await waitFor(() => expect(nextButton().disabled).toBe(false))
    fireEvent.click(nextButton())
    // Blank: the station's own blocker holds Next.
    await screen.findByText(/^Your AGE is empty/)
    fireEvent.change(screen.getByLabelText('Your age'), { target: { value: '42' } })
    await screen.findByText('You send: 599 42')
    const asked = api.get('previewFdRuleset').mock.calls.map((c) => c[0] as Partial<Settings>)
    expect(asked[asked.length - 1].contestEntered).toEqual({ custom_club_sprint: { AGE: '42' } })
    await waitFor(() => expect(nextButton().disabled).toBe(false))
    fireEvent.click(nextButton())
    fireEvent.click(nextButton())
    fireEvent.click(nextButton())
    fireEvent.click(await screen.findByRole('button', { name: 'Start Club Sprint' }))
    await screen.findByText('Club Sprint is on · S&P')
    expect(station.contestEntered).toEqual({ custom_club_sprint: { AGE: '42' } })
    expect(station.fdEvent).toBe('custom_club_sprint')
  })
})

describe('what the contest file holds that this version cannot use', () => {
  it('is named with why, and Delete asks first, then removes it', async () => {
    let left = [{ id: 'custom_future', name: 'Future Sprint', why: 'custom contest file version 2 (this build reads version 1)' }]
    api.get('listUnusableCustomContests').mockImplementation(() => Promise.resolve(structuredClone(left)))
    api.get('deleteCustomContest').mockImplementation(() => {
      left = []
      return Promise.resolve()
    })
    await open()
    const row = (await screen.findByText(/^Future Sprint can't be used by this version of Nexus/)).closest('[data-unusable]') as HTMLElement
    expect(row.textContent).toContain('custom contest file version 2 (this build reads version 1)')
    fireEvent.click(within(row).getByRole('button', { name: 'Delete' }))
    expect(api.get('deleteCustomContest')).not.toHaveBeenCalled()
    fireEvent.click(within(row).getByRole('button', { name: 'Delete it' }))
    await screen.findByText('Deleted Future Sprint.')
    expect(api.get('deleteCustomContest')).toHaveBeenCalledWith('custom_future')
    await waitFor(() => expect(document.querySelector('[data-unusable]')).toBeNull())
  })
})

describe('typed values keep what the station sends', () => {
  it('an age is digits, a name one word of capitals, digits and /', () => {
    const box = document.createElement('input')
    box.value = 'al ex-2/p!'
    expect(enteredTyped('NAME', box)).toBe('ALEX2/P')
    box.value = '4a2'
    expect(enteredTyped('AGE', box)).toBe('42')
    // Upper-casing can lengthen a word (ß → SS): the box still holds no more than the station sends.
    box.value = 'straße'.repeat(2)
    expect(enteredTyped('NAME', box)).toBe('STRASSESTRAS')
    box.value = '1234'
    expect(enteredTyped('AGE', box)).toBe('12')
  })
})

describe('the operator\'s own contests elsewhere', () => {
  it('club sync names its own reason for a contest the operator built', () => {
    expect(contestClubRefusal('custom_club_sprint')).toBe('custom')
    expect(clubSyncRefusalText('custom', 'Club Sprint')).toMatch(/^Club sync does not run Club Sprint: it is a contest you built/)
  })
})

describe('layout: the step buttons stay reachable', () => {
  it('⭐ the setup\'s buttons are pinned to the bottom of its scrolling box, on an opaque ground', async () => {
    await open()
    const read = (name: string) => readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '')
    const rules = parseRules(read('styles.css'))
    const actions = document.querySelector('.contest-setup .wizard-actions')!
    const chain = chainOf(actions)
    expect(winnerAt(rules, 'dark', chain, 'position')?.value).toBe('sticky')
    expect(winnerAt(rules, 'dark', chain, 'bottom')?.value).toBe('0')
    expect(winnerAt(rules, 'dark', chain, 'background', 'background-color')?.value).toBe('var(--panel)')
    // …inside the dialog box, which is the one that scrolls.
    const box = chainOf(document.querySelector('.ui-dialog.contest-setup')!)
    expect(winnerAt(rules, 'dark', box, 'overflow-y')?.value).toBe('auto')
    // A focused control stops above the pinned buttons, not under them (the probe measures 5em).
    expect(winnerAt(rules, 'dark', box, 'scroll-padding-bottom', 'scroll-padding')?.value).toBe('5em')
  })
})
