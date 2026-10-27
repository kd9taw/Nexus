// @vitest-environment jsdom
//
// ⭐ CONTEST MODE FROM SETTINGS: Stop contest… offers the contest-over checklist FIRST and saves
// nothing until it is answered (the merge and the export need the session the mode holds). "Not now"
// saves nothing. "Turn off" saves the mode off with the ticked settings put back, through the patch
// seam, and nothing else the form holds. Start contest… opens the contest setup; no control on the
// form saves the mode on, so it can no longer be saved on with nothing able to start. A Save that
// would switch a running contest to one the station cannot enter is refused, saying why.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor, within } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import type { FieldDayStatus, Settings } from '../types'
import defaultSettings from './__fixtures__/defaultSettings.json'

// The house budget for a Settings render under a loaded full suite (SettingsPanel.stalecopy.test.tsx).
vi.setConfig({ testTimeout: 15_000 })

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
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (fn: () => Promise<unknown>) => fn()),
}))

let station: Record<string, unknown> = {}

/** The snapshot's contest block for a contest the station is running now. */
const running = (event: string) =>
  ({ running: false, state: 'Idle', qsoCount: 0, sections: 0, points: 0, event, log: [] }) as unknown as FieldDayStatus

/** The end of a club weekend at the Illinois QSO Party: the club's call on the air, the mode on, and
 *  the engine's note of the station's own call. `fieldDay` is the snapshot's contest block, as App
 *  passes it. */
function openOn(held: Partial<Settings> = {}, live?: FieldDayStatus | null) {
  station = structuredClone({
    ...defaultSettings,
    mycall: 'W9CLB',
    mygrid: 'EN52',
    fdActive: true,
    fdEvent: 'ilqp',
    contestQthState: 'IL',
    contestQthCounty: 'COOK',
    contestBefore: {
      mycall: 'K9ABC',
      fdOperator: '',
      fdPositionName: '',
      contestCategoryOperator: '',
      contestEntryClass: '',
      contestCategoryPower: '',
      unassistedMode: false,
    },
    ...held,
  })
  // The snapshot follows what is saved unless a test says otherwise: on is a running session.
  const fieldDay = live !== undefined ? live : station.fdActive ? running(String(station.fdEvent)) : null
  const features = {
    enabled: () => true,
    setEnabled: vi.fn(),
    all: () => [],
    profile: 'full',
    setProfile: vi.fn(),
  } as unknown as FeaturesApi
  const panel = (fd: FieldDayStatus | null) => (
    <SettingsPanel
      activeRadioId={0}
      scale={1 as never}
      scaleMode={'auto' as never}
      scaleCap={1 as never}
      onScaleModeChange={() => {}}
      onScaleCapChange={() => {}}
      density={'comfortable' as never}
      onDensityChange={() => {}}
      onResetLayout={() => {}}
      features={features}
      fieldDay={fd}
    />
  )
  const { rerender } = render(panel(fieldDay))
  /** The next snapshot: the live contest block as the station now reports it. */
  return { snapshot: (fd: FieldDayStatus | null) => rerender(panel(fd)) }
}

beforeEach(() => {
  for (const spy of Object.values(api.spies)) {
    spy.mockClear()
    spy.mockImplementation(() => Promise.resolve(null))
  }
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([]))
  api.get('getConnectionLog').mockImplementation(() => Promise.resolve([]))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.6.1'))
  api.get('getAssistanceJournal').mockImplementation(() => Promise.resolve([]))
  api.get('getSettings').mockImplementation(() => Promise.resolve(structuredClone(station)))
  api.get('setSettings').mockImplementation((s: Record<string, unknown>) => {
    station = structuredClone(s)
    return Promise.resolve(null)
  })
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

const writes = () => api.get('setSettings').mock.calls.map((c) => c[0] as Record<string, unknown>)

/** The form has loaded, on the Contesting tab; then Stop contest…. */
async function pressStop() {
  await screen.findAllByDisplayValue('W9CLB')
  fireEvent.click(screen.getByRole('tab', { name: 'Contesting' }))
  const group = document.getElementById('settings-contest-pick')!
  fireEvent.click(within(group).getByRole('button', { name: 'Stop contest…' }))
}

/** The footer Save (`type="submit"`). */
const pressSave = () =>
  fireEvent.click(
    screen.getAllByRole('button', { name: 'Save' }).find((b) => (b as HTMLButtonElement).type === 'submit')!,
  )

describe('Stop contest…', () => {
  it('offers the checklist first and saves nothing until it is answered', async () => {
    openOn()
    await pressStop()
    expect(await screen.findByRole('dialog', { name: 'Contest over?' })).toBeTruthy()
    expect(writes()).toEqual([])
  })

  it('Not now saves nothing and leaves Contest mode on', async () => {
    openOn()
    await pressStop()
    fireEvent.click(await screen.findByRole('button', { name: 'Not now' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    expect(writes()).toEqual([])
    const group = document.getElementById('settings-contest-pick')!
    expect(within(group).getByRole('button', { name: 'Stop contest…' })).toBeTruthy()
  })

  it('Turn off saves the mode off with the ticked settings put back, and nothing else the form holds', async () => {
    const { snapshot } = openOn()
    await screen.findAllByDisplayValue('W9CLB')
    fireEvent.click(screen.getByRole('tab', { name: 'Contesting' }))
    // An edit the operator has not saved: Stop is not a Save of the form.
    fireEvent.change(screen.getByDisplayValue('COOK'), { target: { value: 'WILL' } })
    fireEvent.click(within(document.getElementById('settings-contest-pick')!).getByRole('button', { name: 'Stop contest…' }))
    fireEvent.click(await screen.findByRole('checkbox', { name: /Callsign on the air/ }))
    fireEvent.click(screen.getByRole('button', { name: 'Turn off' }))
    await waitFor(() => expect(writes()).toHaveLength(1))
    expect(writes()[0]).toMatchObject({ fdActive: false, mycall: 'K9ABC', contestQthCounty: 'COOK' })
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    // The station's next snapshot has no session.
    snapshot(null)
    // The form follows: Start contest… now, and the edit is still there, unsaved.
    expect(within(document.getElementById('settings-contest-pick')!).getByRole('button', { name: 'Start contest…' })).toBeTruthy()
    expect(screen.getByDisplayValue('WILL')).toBeTruthy()
    // …and a Save afterwards sends the edit, never the mode back on.
    pressSave()
    await waitFor(() => expect(writes()).toHaveLength(2))
    expect(writes()[1]).toMatchObject({ fdActive: false, contestQthCounty: 'WILL', mycall: 'K9ABC' })
  })

  it('with nothing to offer, the mode simply goes off', async () => {
    openOn({ contestBefore: undefined })
    await pressStop()
    await waitFor(() => expect(writes()).toHaveLength(1))
    expect(writes()[0]).toMatchObject({ fdActive: false })
    expect(screen.queryByRole('dialog')).toBeNull()
  })

  it('at a club, says how many of this laptop\'s contacts the club\'s log does not have yet', async () => {
    // The snapshot's club chip: the link down, three contacts the host never had.
    const fieldDay = {
      running: false,
      state: 'Idle',
      qsoCount: 3,
      sections: 0,
      points: 6,
      event: 'ilqp',
      log: [],
      club: {
        syncState: 'offline',
        queued: 3,
        offlineSinceUnix: 1,
        hosting: false,
        event: 'ILQP 2026',
        hostCall: 'W9CLB',
        score: 0,
        qsos: 0,
        sections: 0,
        skewSecs: 0,
        dupes: [],
        board: [],
      },
    } as FieldDayStatus
    openOn({ fdJoinAddr: '192.168.1.20:42073' }, fieldDay)
    await pressStop()
    await screen.findByRole('dialog', { name: 'Contest over?' })
    expect(document.querySelector('.contest-over-club')?.textContent).toBe(
      "3 contacts from this laptop are not in the club's log yet. They still go to the host when this laptop is back with it, Contest mode on or off. Leave Nexus open, with club sync on, until then.",
    )
    expect(writes()).toEqual([])
  })

  it('puts back a setting over a value that changed since Settings opened', async () => {
    // A seat swap in the pop-out scoreboard after Settings opened: the live value is put back.
    openOn({ fdOperator: '' })
    await screen.findAllByDisplayValue('W9CLB')
    station = { ...station, fdOperator: 'W9OP' }
    await pressStop()
    fireEvent.click(await screen.findByRole('checkbox', { name: /Operator at the key/ }))
    fireEvent.click(screen.getByRole('button', { name: 'Turn off' }))
    await waitFor(() => expect(writes()).toHaveLength(1))
    expect(writes()[0]).toMatchObject({ fdActive: false, fdOperator: '' })
  })
})

describe('Start contest…', () => {
  it('opens the contest setup, and the form has no control that saves Contest mode on', async () => {
    openOn({ fdActive: false, contestBefore: undefined })
    await screen.findAllByDisplayValue('W9CLB')
    fireEvent.click(screen.getByRole('tab', { name: 'Contesting' }))
    const group = document.getElementById('settings-contest-pick')!
    expect(within(group).queryByRole('switch', { name: /Contest mode/ })).toBeNull()
    fireEvent.click(within(group).getByRole('button', { name: 'Start contest…' }))
    expect(await screen.findByRole('dialog', { name: 'Set up a contest' })).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    // Cancel saved nothing, and the form's Save still sends the mode off.
    expect(writes()).toEqual([])
    pressSave()
    await waitFor(() => expect(writes()).toHaveLength(1))
    expect(writes()[0].fdActive).toBe(false)
  })
})

describe('a Save that would switch the running contest', () => {
  it('to one the station cannot enter is refused, naming what is missing, and saves nothing', async () => {
    openOn({ fdClass: '', fdSection: '' })
    api.get('previewFdRuleset').mockImplementation(() => Promise.resolve({ event: 'arrlfd', problem: '' }))
    await screen.findAllByDisplayValue('W9CLB')
    fireEvent.click(screen.getByRole('tab', { name: 'Contesting' }))
    fireEvent.click(screen.getByRole('button', { name: 'ARRL Field Day' }))
    pressSave()
    expect(
      await screen.findByText(
        'Saving would switch the running contest to ARRL Field Day, which cannot start yet: The station cannot enter ARRL Field Day until your FD Class and ARRL Section are set.',
      ),
    ).toBeTruthy()
    expect(writes()).toEqual([])
  })

  it('CONTROL: to one the station can enter saves at once', async () => {
    openOn({ fdClass: '1D', fdSection: 'WI' })
    api.get('previewFdRuleset').mockImplementation(() => Promise.resolve({ event: 'arrlfd', problem: '' }))
    await screen.findAllByDisplayValue('W9CLB')
    fireEvent.click(screen.getByRole('tab', { name: 'Contesting' }))
    fireEvent.click(screen.getByRole('button', { name: 'ARRL Field Day' }))
    pressSave()
    await waitFor(() => expect(writes()).toHaveLength(1))
    expect(writes()[0]).toMatchObject({ fdActive: true, fdEvent: 'arrlfd' })
  })
})

describe('any other Save', () => {
  it('saves at once, with no checklist, when the mode stays on', async () => {
    openOn()
    await screen.findAllByDisplayValue('W9CLB')
    fireEvent.click(screen.getByRole('tab', { name: 'Contesting' }))
    pressSave()
    await waitFor(() => expect(writes()).toHaveLength(1))
    expect(screen.queryByRole('dialog')).toBeNull()
  })
})

describe('Settings reads the contest state off the live station, not its own copy', () => {
  it('a contest started elsewhere after Settings opened shows Stop contest…, naming the running one', async () => {
    openOn({ fdActive: false, contestBefore: undefined }, running('nyqp'))
    await screen.findAllByDisplayValue('W9CLB')
    fireEvent.click(screen.getByRole('tab', { name: 'Contesting' }))
    const group = document.getElementById('settings-contest-pick')!
    expect(within(group).getByRole('button', { name: 'Stop contest…' })).toBeTruthy()
    expect(within(group).queryByRole('button', { name: 'Start contest…' })).toBeNull()
    expect(group.querySelector('[data-contest-mode] [role="status"]')!.textContent).toBe('On: New York QSO Party')
  })

  it('CONTROL: saved on with nothing running offers Start contest…', async () => {
    openOn({ fdActive: true }, null)
    await screen.findAllByDisplayValue('W9CLB')
    fireEvent.click(screen.getByRole('tab', { name: 'Contesting' }))
    const group = document.getElementById('settings-contest-pick')!
    expect(within(group).getByRole('button', { name: 'Start contest…' })).toBeTruthy()
    expect(group.querySelector('[data-contest-mode] [role="status"]')!.textContent).toBe('Off')
  })

  it('the switch guard holds for a contest started elsewhere: a pick that cannot start is refused, nothing saved', async () => {
    // Settings opened with the mode off; the Illinois QSO Party was started on the Contest screen since.
    openOn({ fdActive: false, fdClass: '', fdSection: '', contestBefore: undefined }, running('ilqp'))
    api.get('previewFdRuleset').mockImplementation(() => Promise.resolve({ event: 'arrlfd', problem: '' }))
    await screen.findAllByDisplayValue('W9CLB')
    fireEvent.click(screen.getByRole('tab', { name: 'Contesting' }))
    fireEvent.click(screen.getByRole('button', { name: 'ARRL Field Day' }))
    pressSave()
    expect(
      await screen.findByText(
        'Saving would switch the running contest to ARRL Field Day, which cannot start yet: The station cannot enter ARRL Field Day until your FD Class and ARRL Section are set.',
      ),
    ).toBeTruthy()
    expect(writes()).toEqual([])
  })

  it('the guard fails closed: when the preview cannot be asked, the switching Save is refused, nothing saved', async () => {
    openOn({ fdClass: '1D', fdSection: 'WI' })
    api.get('previewFdRuleset').mockImplementation(() => Promise.reject('unknown command preview_fd_ruleset'))
    await screen.findAllByDisplayValue('W9CLB')
    fireEvent.click(screen.getByRole('tab', { name: 'Contesting' }))
    fireEvent.click(screen.getByRole('button', { name: 'ARRL Field Day' }))
    pressSave()
    expect(
      await screen.findByText(
        'Not saved: picking ARRL Field Day would end the running contest, and the station could not check that ARRL Field Day can start.',
      ),
    ).toBeTruthy()
    expect(writes()).toEqual([])
  })

  it("the checklist's Summary scores from the live settings: a bonus ticked since Settings opened counts", async () => {
    // One contact in the session: the checklist offers its exports.
    const log = [{ call: 'W1AW', class: '2A', section: 'CT', band: '20m', mode: 'CW', whenUnix: 1 }]
    openOn({ fdEvent: 'arrlfd', fdClass: '1D', fdSection: 'WI', fdBonuses: [] }, { ...running('arrlfd'), log, qsoCount: 1 })
    api.get('saveTextToDownloads').mockImplementation((name: string) => Promise.resolve(`/dl/${name}`))
    await screen.findAllByDisplayValue('W9CLB')
    // Ticked on the Contest screen after Settings opened (bonus ticks save at once).
    station = { ...station, fdBonuses: ['emergency-power'] }
    await pressStop()
    fireEvent.click(await screen.findByRole('checkbox', { name: 'Export the Summary to Downloads' }))
    fireEvent.click(screen.getByRole('button', { name: 'Turn off' }))
    await waitFor(() => expect(api.get('saveTextToDownloads')).toHaveBeenCalled())
    const text = api.get('saveTextToDownloads').mock.calls[0][1] as string
    expect(text).toContain('Bonuses claimed (1, 100 pts):')
    expect(text).toContain('  Emergency Power — 100 pts')
  })

  it('Stop contest… says so when the station cannot be read', async () => {
    openOn()
    await screen.findAllByDisplayValue('W9CLB')
    api.get('getSettings').mockImplementation(() => Promise.reject('station unreachable'))
    await pressStop()
    expect(await screen.findByText('station unreachable')).toBeTruthy()
    expect(writes()).toEqual([])
  })
})

// A form describing a radio other than the one operated now goes whole (its flat rig fields are that
// radio's). The contest's master and pick are station-wide, not the radio's: they go as the station
// holds them, unless this form changed them.
describe('a Save after the active radio changed elsewhere', () => {
  const contestFields = (w: Record<string, unknown>) => ({
    fdActive: w.fdActive,
    fdEvent: w.fdEvent,
    contestQthCounty: w.contestQthCounty,
    activeRadio: w.activeRadio,
  })

  it('keeps a contest started elsewhere on, and on its own pick', async () => {
    // Settings opened with the mode off on the Illinois QSO Party; since then New York QSO Party was
    // started on the Contest screen and another radio was picked.
    openOn({ fdActive: false, contestBefore: undefined }, running('nyqp'))
    await screen.findAllByDisplayValue('W9CLB')
    station = { ...station, fdActive: true, fdEvent: 'nyqp', activeRadio: 1 }
    fireEvent.click(screen.getByRole('tab', { name: 'Contesting' }))
    fireEvent.change(screen.getByDisplayValue('COOK'), { target: { value: 'WILL' } })
    pressSave()
    await waitFor(() => expect(writes()).toHaveLength(1))
    expect(contestFields(writes()[0])).toEqual({ fdActive: true, fdEvent: 'nyqp', contestQthCounty: 'WILL', activeRadio: 0 })
  })

  it('keeps a contest stopped elsewhere off', async () => {
    openOn({}, running('ilqp'))
    await screen.findAllByDisplayValue('W9CLB')
    station = { ...station, fdActive: false, activeRadio: 1 }
    fireEvent.click(screen.getByRole('tab', { name: 'Contesting' }))
    fireEvent.change(screen.getByDisplayValue('COOK'), { target: { value: 'WILL' } })
    pressSave()
    await waitFor(() => expect(writes()).toHaveLength(1))
    expect(contestFields(writes()[0])).toEqual({ fdActive: false, fdEvent: 'ilqp', contestQthCounty: 'WILL', activeRadio: 0 })
  })

  it('runs the switch guard on a pick this form changed: one that cannot start is refused, nothing saved', async () => {
    openOn({ fdClass: '', fdSection: '' })
    api.get('previewFdRuleset').mockImplementation(() => Promise.resolve({ event: 'arrlfd', problem: '' }))
    await screen.findAllByDisplayValue('W9CLB')
    station = { ...station, activeRadio: 1 }
    fireEvent.click(screen.getByRole('tab', { name: 'Contesting' }))
    fireEvent.click(screen.getByRole('button', { name: 'ARRL Field Day' }))
    pressSave()
    expect(
      await screen.findByText(
        'Saving would switch the running contest to ARRL Field Day, which cannot start yet: The station cannot enter ARRL Field Day until your FD Class and ARRL Section are set.',
      ),
    ).toBeTruthy()
    expect(writes()).toEqual([])
  })

  it('CONTROL: a pick this form changed, that can start, is saved', async () => {
    openOn({ fdClass: '1D', fdSection: 'WI' })
    api.get('previewFdRuleset').mockImplementation(() => Promise.resolve({ event: 'arrlfd', problem: '' }))
    await screen.findAllByDisplayValue('W9CLB')
    station = { ...station, activeRadio: 1 }
    fireEvent.click(screen.getByRole('tab', { name: 'Contesting' }))
    fireEvent.click(screen.getByRole('button', { name: 'ARRL Field Day' }))
    pressSave()
    await waitFor(() => expect(writes()).toHaveLength(1))
    expect(contestFields(writes()[0])).toEqual({ fdActive: true, fdEvent: 'arrlfd', contestQthCounty: 'COOK', activeRadio: 0 })
  })
})
