// @vitest-environment jsdom
//
// ⭐ CLEAR TEST CONTACTS ON THE CONTEST SCREEN: every contact of this running out of the contest
// log at once, each the way Remove takes the newest, for the contacts logged as a test before the
// start. It asks first, on the button itself, naming how many (a county line is one contact), and
// the second click clears exactly the log it named; the answer, a toast and the Removed list's
// note, says where any of them had already gone, and at a club when every laptop drops them. Only a
// contest that sends a serial says its serials start again at 001 (the station's `sendsSerial`): the
// Illinois QSO Party here sends a county, CQ WPX a serial. Never offered when observing from afar, and
// refused through the Remote stream. The engine decides; this suite pins what the screen asks for and
// says.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, fireEvent, cleanup, act } from '@testing-library/react'
import { ContestView } from './ContestView'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FieldDayQso, FieldDayStatus } from '../types'
import { StreamInputDispatcher } from '../remote-native/stream-input'
import { pushToast } from '../toast'

// A computer driving a station (client mode): off unless a case turns it on.
const driving = vi.hoisted(() => ({ on: false }))
vi.mock('../features/durableStore', async (actual) => ({
  ...(await actual<typeof import('../features/durableStore')>()),
  drivesAnotherStation: () => driving.on,
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (run: () => Promise<unknown>) => run()),
}))
const toast = vi.mocked(pushToast)

vi.mock('../api', () => ({
  getSettings: vi.fn(async () => ({ ...defaultSettings, fdOperator: '' })),
  setSettings: vi.fn(async () => ({})),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  openPanelWindow: vi.fn(async () => {}),
  contestRemoved: vi.fn(async () => []),
  contestUnmerged: vi.fn(async () => []),
  contestRemoveLast: vi.fn(),
  contestRestore: vi.fn(),
  contestClear: vi.fn(),
}))
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>

const T0 = 1_792_342_800 // 2026-10-18 17:00Z

const row = (call: string, qth: string, whenUnix: number): FieldDayQso =>
  ({ call, class: '', section: '', band: '20m', mode: 'CW', submode: '', whenUnix, rcvd: ['599', qth] }) as FieldDayQso

const party = (log: FieldDayQso[]): FieldDayStatus =>
  ({
    running: true,
    state: 'sp',
    qsoCount: log.length,
    sections: 0,
    points: 0,
    log,
    event: 'ilqp',
    rulesYear: 2026,
    role: 'in_state',
    receives: [
      { key: 'RST', kind: 'rst', required: true },
      { key: 'QTH', kind: 'enum', required: true, domain: 'il_counties' },
    ],
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'QTH', raw: 'KANE', domain: 'il_counties' },
    ],
  }) as unknown as FieldDayStatus

/** CQ WPX: the report and a serial, which the station says it sends. */
const wpx = (log: FieldDayQso[]): FieldDayStatus =>
  ({
    ...party(log),
    event: 'cqwpx',
    role: '',
    receives: [
      { key: 'RST', kind: 'rst', required: true },
      { key: 'NR', kind: 'serial', required: true },
    ],
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'NR', raw: '0' },
    ],
    sendsSerial: true,
  }) as unknown as FieldDayStatus

// Three rows, two contacts: K9AAA, then K9NR on a county line (one contact, a row per county).
const LOG = [row('K9AAA', 'COOK', T0), row('K9NR', 'COOK', T0 + 60), row('K9NR', 'DUPG', T0 + 60)]
const NOWHERE = { wsjtx: false, logbook: false, uploaded: [] }

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

const view = (log: FieldDayQso[], observed = false, contest = party) => (
  <ContestView
    fieldDay={contest(log)}
    onSetMode={() => {}}
    fdActive
    tier="FT8"
    observation={observed ? { fdOperator: '', fdPowerMult: 2, fdBonuses: [], fdBonusesPlanned: [] } : undefined}
  />
)

async function show(log: FieldDayQso[], observed = false, contest = party) {
  const r = render(view(log, observed, contest))
  await settle()
  return r
}

const TITLE = 'Take every contact of this running out of the contest log, for contacts you logged as a test before the start.'
const clearButton = () =>
  [...document.querySelectorAll<HTMLButtonElement>('.fd-export button')].find((b) => b.title.startsWith(TITLE)) ?? null
/** What the screen said: the Removed list's note, and the toast raised with it. */
const said = (text: string, kind: 'info' | 'error') => {
  expect(note()).toBe(text)
  expect(toast).toHaveBeenLastCalledWith(text, kind, 12_000)
}
const note = () => document.querySelector('.fd-removed-note')?.textContent ?? null
const clickTwice = async () => {
  fireEvent.click(clearButton()!)
  await act(async () => {
    fireEvent.click(clearButton()!)
  })
  await settle()
}

afterEach(() => {
  cleanup()
  for (const f of Object.values(api)) f.mockClear()
  toast.mockClear()
})

describe('Clear test contacts asks first, then clears exactly the log it named', () => {
  it('names how many contacts on the first click, a county line as one, and clears them on the second', async () => {
    api.contestClear.mockResolvedValueOnce({ outcome: 'cleared', contacts: 2, sentTo: NOWHERE })
    await show(LOG)
    expect(clearButton()!.textContent).toBe('Clear test contacts')
    fireEvent.click(clearButton()!)
    expect(clearButton()!.getAttribute('aria-pressed')).toBe('true')
    expect(clearButton()!.textContent, 'the question is on the button').toBe('Clear all 2 contacts? Click again')
    expect(api.contestClear).not.toHaveBeenCalled()
    expect(toast).not.toHaveBeenCalled()
    await act(async () => {
      fireEvent.click(clearButton()!)
    })
    await settle()
    expect(api.contestClear).toHaveBeenCalledWith(3, 'K9NR', T0 + 60)
    expect(clearButton()!.textContent).toBe('Clear test contacts')
    said('Took 2 contacts out of the contest log. They are under Removed, where Restore puts each back.', 'info')
    expect(api.contestRemoved, 'the Removed list is read again after the clear').toHaveBeenCalledTimes(2)
  })

  it('says the serials start again at 001 only in a contest that sends one: its tooltip, its question and its answer', async () => {
    const told: (string | null | undefined)[][] = []
    for (const contest of [party, wpx]) {
      api.contestClear.mockResolvedValueOnce({ outcome: 'cleared', contacts: 2, sentTo: NOWHERE })
      await show(LOG, false, contest)
      const title = clearButton()!.title
      fireEvent.click(clearButton()!)
      const question = clearButton()!.textContent
      await act(async () => {
        fireEvent.click(clearButton()!)
      })
      await settle()
      told.push([contest(LOG).event, title, question, note()])
      cleanup()
    }
    expect(told).toEqual([
      [
        'ilqp',
        `${TITLE} Each is kept under Removed, where Restore puts it back. Click twice: the first click asks.`,
        'Clear all 2 contacts? Click again',
        'Took 2 contacts out of the contest log. They are under Removed, where Restore puts each back.',
      ],
      [
        'cqwpx',
        `${TITLE} Each is kept under Removed, where Restore puts it back. Serials restart at 001. Click twice: the first click asks.`,
        'Clear all 2 contacts? Serials restart at 001. Click again',
        'Took 2 contacts out of the contest log. They are under Removed, where Restore puts each back. Serials restart at 001.',
      ],
    ])
  })

  it('asks again when a contact is logged between the two clicks', async () => {
    const r = await show(LOG)
    fireEvent.click(clearButton()!)
    r.rerender(view([...LOG, row('W9BBB', 'LAKE', T0 + 120)]))
    await settle()
    fireEvent.click(clearButton()!)
    await settle()
    expect(api.contestClear).not.toHaveBeenCalled()
    expect(clearButton()!.textContent).toBe('Clear all 3 contacts? Click again')
  })

  it('says where any of them had already gone, and at a club when every laptop drops them', async () => {
    for (const [club, words] of [
      ['linked', 'The club log drops them too, on every laptop, as soon as the host has them.'],
      ['away', 'This laptop is not with the club host right now: the club log drops them when the link is back.'],
    ] as const) {
      api.contestClear.mockResolvedValueOnce({
        outcome: 'cleared',
        contacts: 2,
        club,
        sentTo: { n3fjp: '192.168.1.5', wsjtx: true, logbook: true, uploaded: ['lotw'] },
      })
      await show(LOG)
      await clickTwice()
      said(
        'Took 2 contacts out of the contest log. They are under Removed, where Restore puts each back. ' +
          `${words} Some were already sent to N3FJP (192.168.1.5), WSJT-X listeners: delete them there too. ` +
          'Some stay in your logbook, and LoTW have them: Nexus cannot take them back from there.',
        'info',
      )
      cleanup()
    }
  })

  it('says so when the log changed and nothing was cleared', async () => {
    api.contestClear.mockResolvedValueOnce({ outcome: 'refused', refusal: 'changed' })
    await show(LOG)
    await clickTwice()
    said('The contest log changed, so nothing was cleared.', 'error')
    expect(document.querySelector('.fd-removed-note')?.getAttribute('role')).toBe('alert')
  })

  it('is not offered on an empty log, nor when observing from afar', async () => {
    await show([])
    expect(clearButton()?.disabled).toBe(true)
    cleanup()
    await show(LOG, true)
    expect(clearButton()).toBeNull()
  })
})

// jsdom never lays out: `elementFromPoint` does not exist, so each streamed press says what is
// under it.
describe('a press through the Remote stream clears nothing', () => {
  let under: Element | null = null
  let stream: StreamInputDispatcher | null = null
  beforeEach(() => {
    under = null
    Object.defineProperty(document, 'elementFromPoint', { configurable: true, value: () => under })
    stream = new StreamInputDispatcher(window)
  })
  afterEach(() => {
    stream?.dispose()
    stream = null
    delete (document as { elementFromPoint?: unknown }).elementFromPoint
  })
  const pointer = (action: 'down' | 'up') =>
    ({ type: 'pointer', action, x: 0.5, y: 0.5, button: 0, buttons: action === 'down' ? 1 : 0,
      modifiers: 0, pointerType: 'mouse', clicks: 1 })

  it('refuses Clear test contacts clicked twice through the stream, and says why; CONTROL: at the station it clears', async () => {
    api.contestClear.mockResolvedValue({ outcome: 'cleared', contacts: 2, sentTo: NOWHERE })
    await show(LOG)
    for (let i = 0; i < 2; i++) {
      under = clearButton()
      await act(async () => { stream!.handle(pointer('down')); stream!.handle(pointer('up')) })
    }
    await settle()
    expect(api.contestClear).not.toHaveBeenCalled()
    said('Only at the station: a press through Remote clears no contact.', 'error')
    await clickTwice()
    expect(api.contestClear).toHaveBeenCalledWith(3, 'K9NR', T0 + 60)
  })
})

// A computer driving a station: Clear is the station's alone, as through the stream. It used to be
// sent, refused there as station-only, and said "Nexus could not clear the contacts", which named
// no reason. It now says the station-only line, and sends nothing.
describe('on a computer driving a station', () => {
  afterEach(() => {
    driving.on = false
  })
  it('Clear test contacts says it is only at the station, and sends nothing; CONTROL: at the station it clears', async () => {
    api.contestClear.mockResolvedValue({ outcome: 'cleared', contacts: 2, sentTo: NOWHERE })
    driving.on = true
    await show(LOG)
    await clickTwice()
    expect(api.contestClear).not.toHaveBeenCalled()
    said('Only at the station: a press through Remote clears no contact.', 'error')
    driving.on = false
    await clickTwice()
    expect(api.contestClear).toHaveBeenCalledWith(3, 'K9NR', T0 + 60)
  })
})
