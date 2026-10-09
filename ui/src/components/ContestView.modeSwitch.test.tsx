// @vitest-environment jsdom
//
// THE FIELD DAY MODE SWITCH ON THE CONTEST SCREEN — one click on or off, beside the picked
// contest and its window, writing the SAME `fdActive` Settings writes.
//
// The backend here is a live one: a read returns what it holds NOW and a save replaces it, so
// "the same path" is checked by value — the struct saved is the backend's at save time with
// `fdActive` changed and nothing else, a QSY made after the screen opened included. Every
// sentence is compared whole, because each one is a claim the operator acts on.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react'
import { ContestView } from './ContestView'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FieldDayStatus, Settings } from '../types'
import type { FdRulesetDto } from '../api'
import { setSettings } from '../api'

let station: Record<string, unknown> = {}
let preview: FdRulesetDto | null = null

vi.mock('../api', () => ({
  // The contest screen's Removed list, read on mount: none removed.
  contestRemoved: vi.fn(async () => []),
  getSettings: vi.fn(async () => ({ ...station })),
  setSettings: vi.fn(async (s: Record<string, unknown>) => {
    station = { ...s }
    return {}
  }),
  getFdRuleset: vi.fn(async () => preview),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  fdClubExport: vi.fn(async () => ''),
  fdSetUpload: vi.fn(async () => ({})),
  saveTextToDownloads: vi.fn(async () => ''),
  openPanelWindow: vi.fn(async () => {}),
}))

/** 1700Z Sunday 18 October 2026 to 0100Z Monday: the Illinois QSO Party's window. */
const ILQP_START = Date.UTC(2026, 9, 18, 17) / 1000
const ILQP_END = Date.UTC(2026, 9, 19, 1) / 1000

const rules = (over: Partial<FdRulesetDto> = {}): FdRulesetDto => ({
  event: 'ilqp',
  rulesYear: 2026,
  bannedModes: ['FT8', 'FT4'],
  spottingAllowed: true,
  clusterAllowed: true,
  enforcement: 'warn',
  role: 'in_state',
  exchange: ['599', 'COOK'],
  problem: '',
  eventStartUnix: ILQP_START,
  eventEndUnix: ILQP_END,
  ...over,
})

const session = (over: Partial<FieldDayStatus> = {}): FieldDayStatus =>
  ({
    running: false,
    state: 'Idle',
    qsoCount: 0,
    sections: 0,
    points: 0,
    log: [],
    event: 'ilqp',
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'QTH', raw: 'COOK' },
    ],
    sentExchange: '599 COOK',
    receives: [],
    eventStartUnix: ILQP_START,
    eventEndUnix: ILQP_END,
    rulesYear: 2026,
    ...over,
  }) as FieldDayStatus

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 12; i++) await Promise.resolve()
  })
}

interface Open {
  fieldDay?: FieldDayStatus | null
  fdActive?: boolean
  fdRuleset?: FdRulesetDto | null
  observed?: boolean
}

/** Open the screen with the backend holding `held`, and App's props as `o` says. */
async function openOn(held: Partial<Settings>, o: Open = {}) {
  station = { ...defaultSettings, ...held }
  const onOpenSettings = vi.fn()
  const onSettingsSaved = vi.fn()
  const onSetMode = vi.fn()
  render(
    <ContestView
      fieldDay={o.fieldDay ?? null}
      onSetMode={onSetMode}
      fdActive={o.fdActive ?? false}
      fdRuleset={o.fdRuleset === undefined ? preview : o.fdRuleset}
      observation={
        o.observed ? { fdOperator: '', fdPowerMult: 2, fdBonuses: [], fdBonusesPlanned: [] } : undefined
      }
      onOpenSettings={onOpenSettings}
      onSettingsSaved={onSettingsSaved}
    />,
  )
  await settle()
  return { onOpenSettings, onSettingsSaved, onSetMode }
}

const theSwitch = () => screen.getByRole('switch') as HTMLButtonElement
const press = async () => {
  fireEvent.click(theSwitch())
  await settle()
}
/** A line's own sentence, without the link button it may carry. */
const sentence = (el: Element) =>
  [...el.childNodes]
    .filter((n) => n.nodeType === Node.TEXT_NODE)
    .map((n) => n.textContent)
    .join('')
    .trim()
const lines = () => [...document.querySelectorAll('.fd-mode-status .fd-mode-line')].map(sentence)

const ILQP_STATION: Partial<Settings> = { fdEvent: 'ilqp', contestQthState: 'IL', contestQthCounty: 'COOK' }

beforeEach(() => {
  preview = rules()
  vi.mocked(setSettings).mockClear()
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

describe('the switch writes the same setting, through the same path', () => {
  it('on: the backend’s settings at save time, with fdActive true and nothing else changed', async () => {
    const { onSettingsSaved } = await openOn(ILQP_STATION)
    expect(theSwitch().getAttribute('aria-checked')).toBe('false')
    expect(theSwitch().getAttribute('aria-label')).toBe('Enable Field Day mode')
    // A QSY after the screen opened: a save of the screen's own copy would send it back.
    station = { ...station, dialMhz: 7.04, band: '40m' }
    const before = { ...station }
    await press()
    expect(vi.mocked(setSettings)).toHaveBeenCalledTimes(1)
    expect(vi.mocked(setSettings).mock.calls[0][0]).toEqual({ ...before, fdActive: true })
    expect(station.dialMhz).toBe(7.04)
    expect(theSwitch().getAttribute('aria-checked')).toBe('true')
    expect(theSwitch().getAttribute('aria-label')).toBe('Disable Field Day mode')
    // App is told, so the rail, Settings and the cockpits read the change at once.
    expect(onSettingsSaved).toHaveBeenCalledTimes(1)
  })

  it('off: fdActive false the same way, and no start check stands in its way', async () => {
    // A blank class would refuse a turn-on; it must never trap an operator in the mode.
    await openOn({ fdActive: true, fdClass: '', fdSection: '' }, {
      fdActive: true,
      fdRuleset: rules({ event: 'arrlfd', bannedModes: [] }),
    })
    const before = { ...station }
    await press()
    expect(vi.mocked(setSettings).mock.calls.map((c) => c[0])).toEqual([{ ...before, fdActive: false }])
    expect(theSwitch().getAttribute('aria-checked')).toBe('false')
  })

  it('shows what App says once App has re-read it, and the written value only until then', async () => {
    station = { ...defaultSettings, ...ILQP_STATION }
    const view = (fdActive: boolean) => (
      <ContestView fieldDay={null} onSetMode={() => {}} fdActive={fdActive} fdRuleset={preview} />
    )
    const r = render(view(false))
    await settle()
    await press()
    // Saved, and App has not re-read yet: the value just written, not a flick back to off.
    expect(theSwitch().getAttribute('aria-checked')).toBe('true')
    r.rerender(view(true))
    expect(theSwitch().getAttribute('aria-checked')).toBe('true')
    // Settings turns it off later: App's copy is the truth from then on.
    r.rerender(view(false))
    expect(theSwitch().getAttribute('aria-checked')).toBe('false')
  })
})

describe('it refuses to turn on, and names what is missing', () => {
  it('ARRL Field Day with no class: no save, the class named, and a way to Field Day Setup', async () => {
    preview = rules({ event: 'arrlfd', bannedModes: [], problem: '' })
    const { onOpenSettings } = await openOn({ fdEvent: '', fdClass: '', fdSection: 'WI' })
    await press()
    expect(vi.mocked(setSettings)).not.toHaveBeenCalled()
    expect(theSwitch().getAttribute('aria-checked')).toBe('false')
    const alert = screen.getByRole('alert')
    expect(sentence(alert)).toBe('The station cannot enter ARRL Field Day until your FD Class is set.')
    fireEvent.click(screen.getByRole('button', { name: 'Open in Settings' }))
    expect(onOpenSettings).toHaveBeenCalledWith('field-day')
  })

  it('both blank: both named, in one sentence', async () => {
    preview = rules({ event: 'arrlfd', bannedModes: [], problem: '' })
    await openOn({ fdEvent: 'arrlfd', fdClass: ' ', fdSection: '' })
    await press()
    expect(vi.mocked(setSettings)).not.toHaveBeenCalled()
    expect(sentence(screen.getByRole('alert'))).toBe(
      'The station cannot enter ARRL Field Day until your FD Class and ARRL Section are set.',
    )
  })

  it('Winter Field Day names the box as Settings does', async () => {
    preview = rules({ event: 'wfd', bannedModes: [], problem: '' })
    await openOn({ fdEvent: 'wfd', fdClass: '', fdSection: 'IL' })
    await press()
    expect(sentence(screen.getByRole('alert'))).toBe(
      'The station cannot enter Winter Field Day until your WFD Class and Category is set.',
    )
  })

  it('a contest missing a slot: the engine’s own sentence, and a way to the station data', async () => {
    const why = 'Your COUNTY is empty — it is part of the exchange you transmit. Fill it in on the Contesting tab in Settings.'
    // App's copy was read before the county was cleared: the click reads the backend again.
    const { onOpenSettings } = await openOn({ ...ILQP_STATION, contestQthCounty: '' }, { fdRuleset: rules() })
    preview = rules({ problem: why })
    await press()
    expect(vi.mocked(setSettings)).not.toHaveBeenCalled()
    expect(sentence(screen.getByRole('alert'))).toBe(why)
    fireEvent.click(screen.getByRole('button', { name: 'Open in Settings' }))
    expect(onOpenSettings).toHaveBeenCalledWith('contest-station')
  })

  it('says it before the click, too, while the mode is off', async () => {
    preview = rules({ event: 'arrlfd', bannedModes: [], problem: '' })
    await openOn({ fdEvent: '', fdClass: '', fdSection: '' })
    expect(screen.queryByRole('alert')).toBeNull()
    expect(lines()).toEqual([
      'The station cannot enter ARRL Field Day until your FD Class and ARRL Section are set.',
    ])
  })

  it('CONTROL: with the class and section set, the same click saves', async () => {
    preview = rules({ event: 'arrlfd', bannedModes: [], problem: '' })
    await openOn({ fdEvent: '', fdClass: '1D', fdSection: 'WI' })
    await press()
    expect(vi.mocked(setSettings)).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole('alert')).toBeNull()
  })
})

describe('the banner names the picked contest and its window before any session exists', () => {
  it('the Illinois QSO Party, its dates in UTC, and how far off it is', async () => {
    vi.useFakeTimers({ toFake: ['Date'] })
    vi.setSystemTime(new Date('2026-10-08T12:00:00Z'))
    await openOn(ILQP_STATION)
    expect(document.querySelector('.fd-event-name')!.textContent).toBe('Illinois QSO Party')
    expect(document.querySelector('.fd-event-subtitle')!.textContent).toBe(
      'Illinois QSO Party: Oct 18–19 · starts in 10 days',
    )
    // The switch sits in that banner, first.
    expect(document.querySelector('.fd-event-banner')!.firstElementChild!.contains(theSwitch())).toBe(true)
    expect(lines()).toEqual([
      'Off. When it is on, the log strips, this screen and the exports follow the Illinois QSO Party rules.',
    ])
  })
})

describe('with the mode on, the screen says which rules apply now', () => {
  it('the running contest’s rules, and the modes it does not permit', async () => {
    await openOn({ ...ILQP_STATION, fdActive: true }, { fieldDay: session(), fdActive: true })
    expect(lines()).toEqual([
      'The Illinois QSO Party rules apply now: the log strips, this screen and the exports follow them.',
      'Not permitted: FT8, FT4. Contacts in them are logged but do not count.',
    ])
  })

  it('Winter Field Day inside its window: no spots over the internet, said plainly', async () => {
    const start = Date.UTC(2027, 0, 23, 16) / 1000
    const wfd = session({
      event: 'wfd',
      eventStartUnix: start,
      eventEndUnix: start + 30 * 3600,
      composing: [
        { key: 'CLASS', raw: '2O' },
        { key: 'SECTION', raw: 'IL', domain: 'fd_sections' },
      ],
    })
    const wfdRules = rules({ event: 'wfd', bannedModes: ['FT8', 'FT4'], spotsRfOnly: true })
    const held = { fdEvent: 'wfd', fdClass: '2O', fdSection: 'IL', fdActive: true }
    vi.useFakeTimers({ toFake: ['Date'] })
    vi.setSystemTime(new Date('2027-01-23T20:00:00Z'))
    await openOn(held, { fieldDay: wfd, fdActive: true, fdRuleset: wfdRules })
    expect(lines()).toEqual([
      'The Winter Field Day rules apply now: the log strips, this screen and the exports follow them.',
      'Not permitted: FT8, FT4. Contacts in them are logged but do not count.',
      'No spots are posted over the internet during the event.',
    ])
    // CONTROL: the day before, the same rules and no such line — it is the window's.
    cleanup()
    vi.setSystemTime(new Date('2027-01-22T20:00:00Z'))
    await openOn(held, { fieldDay: wfd, fdActive: true, fdRuleset: wfdRules })
    expect(lines()).not.toContain('No spots are posted over the internet during the event.')
    expect(lines()).toContain('Not permitted: FT8, FT4. Contacts in them are logged but do not count.')
  })

  it('a contest picked since it started: the switch is the way over, and no rules from the wrong one', async () => {
    await openOn(
      { ...ILQP_STATION, fdEvent: 'cqww_cw', fdActive: true },
      { fieldDay: session(), fdActive: true, fdRuleset: rules({ event: 'cqww_cw', bannedModes: ['RTTY'] }) },
    )
    expect(document.querySelector('.fd-event-name')!.textContent).toBe('Illinois QSO Party')
    expect(lines()).toEqual([
      'The Illinois QSO Party rules apply now: the log strips, this screen and the exports follow them.',
      'Picked in Settings: CQ World-Wide DX Contest (CW). Turn Field Day mode off and on again to switch to it.',
    ])
  })

  it('a club: what turning the mode off does to it, said before the click', async () => {
    await openOn(
      { ...ILQP_STATION, fdActive: true, fdJoinAddr: '192.168.1.20:42073' },
      { fieldDay: session(), fdActive: true },
    )
    expect(lines()).toContain(
      'Club sync is on. With Field Day mode off, this station stays in the club event and sends no contacts until the mode is back on. Nothing in the log is lost.',
    )
    // CONTROL: no club, no such line.
    cleanup()
    await openOn({ ...ILQP_STATION, fdActive: true, fdJoinAddr: '' }, { fieldDay: session(), fdActive: true })
    expect(lines().some((l) => l.startsWith('Club sync'))).toBe(false)
  })
})

describe('what the switch must not do', () => {
  it('with the mode off and no session, Running and S&P cannot enter the contest behind it', async () => {
    const { onSetMode } = await openOn(ILQP_STATION)
    const running = screen.getByRole('button', { name: 'Running' }) as HTMLButtonElement
    const sp = screen.getByRole('button', { name: 'S&P' }) as HTMLButtonElement
    expect([running.disabled, sp.disabled]).toEqual([true, true])
    fireEvent.click(running)
    expect(onSetMode).not.toHaveBeenCalled()
    // CONTROL: with the mode on they work as they always have.
    cleanup()
    const on = await openOn({ ...ILQP_STATION, fdActive: true }, { fieldDay: session(), fdActive: true })
    fireEvent.click(screen.getByRole('button', { name: 'Running' }))
    expect(on.onSetMode).toHaveBeenCalledWith('fieldday-run')
  })

  it('a Remote observer sees the state and cannot change it', async () => {
    await openOn({ ...ILQP_STATION, fdActive: true }, { fieldDay: session(), fdActive: true, observed: true })
    expect(theSwitch().getAttribute('aria-checked')).toBe('true')
    expect(theSwitch().disabled).toBe(true)
    fireEvent.click(theSwitch())
    await settle()
    expect(vi.mocked(setSettings)).not.toHaveBeenCalled()
  })
})
