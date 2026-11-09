// @vitest-environment jsdom
//
// THE CONTEST CONTACTS NO SESSION LOADS ANY MORE, AND THE LOGBOOK DOES NOT HOLD, ARE OFFERED.
//
// The contest journal keeps a contact past its four days until it is in the logbook (it used to
// drop it at the next contact, merged or not). The contest screen asks the station for those
// contacts when it opens, says how many, from which contest and when, and merges them with one
// press — the end-of-contest merge's own rule, so a press never writes a contact twice. Asked
// of the station only: a Remote observer is never asked, and a press through the Remote stream
// merges nothing, as the end-of-contest merge.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, act, cleanup, fireEvent } from '@testing-library/react'
import { ContestView } from './ContestView'
import { StreamInputDispatcher } from '../remote-native/stream-input'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { ContestUnmerged } from '../api'
import type { FieldDayStatus, FdMergeReport } from '../types'

const contestUnmerged = vi.fn<() => Promise<ContestUnmerged[]>>()
const contestMergeUnmerged = vi.fn<(event: string) => Promise<FdMergeReport>>()

vi.mock('../api', () => ({
  contestRemoved: vi.fn(async () => []),
  contestUnmerged: () => contestUnmerged(),
  contestMergeUnmerged: (event: string) => contestMergeUnmerged(event),
  getSettings: vi.fn(async () => ({ ...defaultSettings, fdOperator: '' })),
  setSettings: vi.fn(async () => ({})),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  fdClubExport: vi.fn(async () => ''),
  fdSetUpload: vi.fn(async () => ({})),
  fdMergeToGeneral: vi.fn(async () => ({ added: 0, already: 0, refused: 0, queued: false })),
  saveTextToDownloads: vi.fn(async () => {}),
  openPanelWindow: vi.fn(async () => {}),
}))

const FD = (over: Partial<FieldDayStatus> = {}): FieldDayStatus =>
  ({
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'QTH', raw: 'KANE', domain: 'il_counties' },
    ],
    running: false,
    state: 'sp',
    qsoCount: 1,
    sections: 1,
    points: 2,
    log: [{ call: 'N9CCC', class: '', section: '', band: '40m', mode: 'CW', whenUnix: 1_792_783_800, mex: '599 KANE' }],
    event: 'ilqp',
    upload: { enabled: false, destinations: [], available: ['wrl', 'qrz', 'clublog'], hint: '' },
    ...over,
  }) as unknown as FieldDayStatus

/** 2026-10-18T19:30:00Z and a minute on: the party's Sunday. */
const SUN = 1_792_351_800
const PARTY: ContestUnmerged = { event: 'ilqp', count: 2, firstUnix: SUN, lastUnix: SUN + 60 }

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

const show = async (fd: FieldDayStatus = FD(), observed = false) => {
  render(
    <ContestView
      fieldDay={fd}
      tier="FT8"
      onSetMode={() => {}}
      observation={observed ? { fdOperator: '', fdPowerMult: 2, fdBonuses: [], fdBonusesPlanned: [] } : undefined}
    />,
  )
  await settle()
}

beforeEach(() => {
  contestUnmerged.mockReset()
  contestMergeUnmerged.mockReset()
  // A dismissal is kept in storage: one test's must not hide another's offer.
  localStorage.clear()
})
afterEach(cleanup)

describe('the contacts the contest journal keeps that the logbook does not hold', () => {
  it('says how many, from which contest and when, and offers their merge', async () => {
    contestUnmerged.mockResolvedValue([PARTY])
    await show()
    expect(contestUnmerged).toHaveBeenCalledTimes(1)
    expect(screen.getByText(/^2 contacts from Illinois QSO Party, logged 2026-10-18 \(UTC\), are not in your logbook\./)).toBeTruthy()
    const merge = screen.getByRole('button', { name: 'Merge these 2 into my logbook' })
    expect(merge.getAttribute('title')).toContain('never duplicated')
  })

  it('gives the first and the last day when they were logged on more than one', async () => {
    contestUnmerged.mockResolvedValue([{ ...PARTY, count: 3, lastUnix: SUN + 2 * 86_400 }])
    await show()
    expect(screen.getByText(/^3 contacts from Illinois QSO Party, logged 2026-10-18 to 2026-10-20 \(UTC\)/)).toBeTruthy()
  })

  it('offers each contest its own merge, by the contest it names', async () => {
    contestUnmerged.mockResolvedValue([PARTY, { event: 'arrlfd', count: 1, firstUnix: SUN - 86_400 * 100, lastUnix: SUN - 86_400 * 100 }])
    contestMergeUnmerged.mockResolvedValue({ added: 1, already: 0, refused: 0, queued: false })
    await show()
    expect(screen.getByText(/^1 contact from ARRL Field Day, logged 2026-07-10 \(UTC\), is not in your logbook\./)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Merge it into my logbook' }))
    await settle()
    expect(contestMergeUnmerged).toHaveBeenCalledWith('arrlfd')
  })

  it('merges with one press, and its report stays when the offer it answered goes', async () => {
    contestUnmerged.mockResolvedValueOnce([PARTY]).mockResolvedValueOnce([])
    contestMergeUnmerged.mockResolvedValue({ added: 2, already: 0, refused: 0, queued: false })
    await show()
    fireEvent.click(screen.getByRole('button', { name: 'Merge these 2 into my logbook' }))
    await settle()
    expect(contestMergeUnmerged).toHaveBeenCalledTimes(1)
    expect(contestMergeUnmerged).toHaveBeenCalledWith('ilqp')
    expect(contestUnmerged, 'asked again after the merge').toHaveBeenCalledTimes(2)
    expect(screen.queryByRole('button', { name: /Merge these/ })).toBeNull()
    expect(screen.getByRole('status').textContent).toBe('Added 2 · 0 already in your logbook')
  })

  it("surfaces the engine's own refusal", async () => {
    contestUnmerged.mockResolvedValue([PARTY])
    contestMergeUnmerged.mockRejectedValue('Your QTH is empty — it is part of the exchange you transmit.')
    await show()
    fireEvent.click(screen.getByRole('button', { name: 'Merge these 2 into my logbook' }))
    await settle()
    expect(screen.getByRole('alert').textContent).toBe('Your QTH is empty — it is part of the exchange you transmit.')
    expect(screen.getByRole('button', { name: 'Merge these 2 into my logbook' }), 'still offered').toBeTruthy()
  })

  it('shows nothing when the journal keeps none, and nothing outside the contest', async () => {
    contestUnmerged.mockResolvedValue([])
    await show()
    expect(screen.queryByText(/not in your logbook/)).toBeNull()
    cleanup()
    contestUnmerged.mockClear()
    render(<ContestView fieldDay={null} tier="FT8" onSetMode={() => {}} />)
    await settle()
    expect(contestUnmerged, 'no contest, nothing to ask').not.toHaveBeenCalled()
  })

  // The Logbook copy of a merged contact was deleted: the journal keeps the contact and offers it
  // again, and the operator who deleted it on purpose says no once. A relaunch remounts the screen
  // and asks the station again, which still answers with the same contacts.
  it('Dismiss puts the offer away for good, a relaunch included, and merges nothing', async () => {
    contestUnmerged.mockResolvedValue([PARTY])
    await show()
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }))
    await settle()
    expect(screen.queryByText(/not in your logbook/)).toBeNull()
    cleanup()
    await show()
    expect(contestUnmerged, 'asked again on the relaunch').toHaveBeenCalledTimes(2)
    expect(screen.queryByText(/not in your logbook/)).toBeNull()
    expect(contestMergeUnmerged).not.toHaveBeenCalled()
    // CONTROL: a contact not offered before (a third one) is offered again, Dismiss and all.
    cleanup()
    contestUnmerged.mockResolvedValue([{ ...PARTY, count: 3 }])
    await show()
    expect(screen.getByRole('button', { name: 'Merge these 3 into my logbook' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Dismiss' })).toBeTruthy()
  })

  it('never asks the station for a Remote observer, and offers nothing there', async () => {
    contestUnmerged.mockResolvedValue([PARTY])
    await show(FD(), true)
    expect(contestUnmerged).not.toHaveBeenCalled()
    expect(screen.queryByText(/not in your logbook/)).toBeNull()
  })
})

// jsdom never lays out: `elementFromPoint` does not exist, so the streamed press says what is
// under it.
describe('a press through the Remote stream merges nothing', () => {
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

  it('refuses the merge through the stream, and says why; CONTROL: at the station it merges', async () => {
    contestUnmerged.mockResolvedValue([PARTY])
    contestMergeUnmerged.mockResolvedValue({ added: 2, already: 0, refused: 0, queued: false })
    await show()
    const merge = () => screen.getByRole('button', { name: 'Merge these 2 into my logbook' })
    under = merge()
    for (const action of ['down', 'up']) {
      await act(async () => {
        stream!.handle({ type: 'pointer', action, x: 0.5, y: 0.5, button: 0, buttons: action === 'down' ? 1 : 0,
          modifiers: 0, pointerType: 'mouse', clicks: 1 })
      })
    }
    await settle()
    expect(contestMergeUnmerged).not.toHaveBeenCalled()
    expect(screen.getByRole('alert').textContent).toBe(
      'Only at the station: a press through Remote merges no contact into the logbook.',
    )
    fireEvent.click(merge())
    await settle()
    expect(contestMergeUnmerged).toHaveBeenCalledWith('ilqp')
  })
})
