// @vitest-environment jsdom
//
// THE LOG LINE'S HUNT TAG HAS AN ✕, AND IT ENDS THE HUNT. A hunt set on purpose (HUNT on the POTA /
// SOTA view, the map, an FT8 row) could only be ended on the POTA / SOTA view, and the log line's Clear
// could not get rid of it: the hunt's prefill put the park straight back into the emptied strip. The
// ✕ on the tag makes the banner ✕'s own call (`clearHuntTarget`) and hands the cleared snapshot up the
// way that view does, so the tag and the banner go at once. The park the hunt's prefill put in the box
// goes with it; a park the operator typed, or one a Needed click handed over, is theirs and stays.
//
// Not on the hosted Remote page: there the form's one station write is Log, and a browser ends a hunt
// on the POTA / SOTA view, through its own change under the logging grant (RemoteOta).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor, act, within } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import { clearHuntTarget, logQso } from '../api'
import { pushToast } from '../toast'
import { StationControlContext } from '../stationAccess'
import { t } from '../i18n'
import type { AppSnapshot, LoggedQso } from '../types'

// THE BUDGET: the house 15 s, as LogEntry.workPark.test.tsx has it (the same strip, the same waits).
vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', async (importOriginal) => {
  // Every export stubbed from the real module (LogEntry.workPark's pattern); the overrides are the
  // callbook, the park directory, the log write and the hunt's clear this file reads.
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => null) : actual[k]
  }
  return {
    ...auto,
    qrzLookup: vi.fn(async () => null),
    resolveEntity: vi.fn(async () => null),
    lookupPark: vi.fn(async () => null),
    lookupParkLive: vi.fn(async () => null),
    searchParks: vi.fn(async () => []),
    setCwPeerInfo: vi.fn(async () => {}),
    setLogFormGrid: vi.fn(async () => {}),
    logQso: vi.fn(async (rec: unknown) => rec),
    askLog: vi.fn(() => new Promise(() => {})),
    clearHuntTarget: vi.fn(async () => null),
  }
})
// The real `withErrorToast`'s contract: the action's value, or null after an error toast.
vi.mock('../toast', () => {
  const pushToast = vi.fn()
  return {
    pushToast,
    withErrorToast: vi.fn(async (action: () => Promise<unknown>, fallback: string) => {
      try {
        return await action()
      } catch {
        pushToast(fallback, 'error')
        return null
      }
    }),
  }
})

const mockedClear = vi.mocked(clearHuntTarget)
const mockedLog = vi.mocked(logQso)

type Hunt = { program: string; reference: string; call: string }
type Work = { call: string; ts: number; park?: { program: string; reference: string } | null }
const snapWith = (hunt: Hunt | null) =>
  ({ mycall: 'KD9TAW', radio: { band: '20m', dialMhz: 14.25 }, hunt }) as unknown as AppSnapshot
const HUNT: Hunt = { program: 'POTA', reference: 'US-1000', call: 'K9XYZ' }
/** What `clear_hunt_target` answers: the station's snapshot with no hunt. */
const CLEARED = snapWith(null)
const pota = (reference: string) => ({ program: 'POTA', reference })

function strip(hunt: Hunt | null, work: Work | null, onSnap?: (s: AppSnapshot) => void) {
  return (
    <LogEntry snap={snapWith(hunt)} mode="SSB" defaultRst="59" exchange="terrestrial" pendingWork={work} onSnap={onSnap} />
  )
}
const park = () => document.querySelector('input.le-park-ref') as HTMLInputElement
const callBox = () => document.querySelector('input.le-call') as HTMLInputElement
const chip = () => document.querySelector('.le-hunt-chip') as HTMLElement | null
const huntX = () => within(chip()!).getByRole('button', { name: t('ota.hunt.clear') })
const clearButton = () => screen.getByRole('button', { name: /^clear$/i })
const settle = () => act(async () => { for (let i = 0; i < 8; i++) await Promise.resolve() })
/** The record the Log button wrote. */
async function logged(): Promise<LoggedQso> {
  fireEvent.click(screen.getByRole('button', { name: /^log$/i }))
  await waitFor(() => expect(mockedLog).toHaveBeenCalledTimes(1))
  return mockedLog.mock.calls[0][0] as LoggedQso
}

beforeEach(() => {
  mockedClear.mockReset().mockImplementation(async () => CLEARED)
  mockedLog.mockClear()
  vi.mocked(pushToast).mockClear()
})
afterEach(cleanup)

describe('the hunt tag’s ✕', () => {
  it('is a real button named as the POTA / SOTA banner’s ✕, reachable from the keyboard', () => {
    render(strip(HUNT, null))
    const x = huntX()
    expect(x.tagName).toBe('BUTTON')
    expect(x.getAttribute('type')).toBe('button')
    expect(x.getAttribute('title')).toBe(t('ota.hunt.clear'))
    x.focus()
    expect(document.activeElement, 'the ✕ cannot take the focus').toBe(x)
  })

  it('ends the hunt with the banner’s own call and hands the cleared snapshot up', async () => {
    const onSnap = vi.fn()
    const view = render(strip(HUNT, null, onSnap))
    fireEvent.click(huntX())
    await waitFor(() => expect(onSnap).toHaveBeenCalledTimes(1))
    expect(mockedClear.mock.calls, 'not the banner’s call').toEqual([[]])
    expect(onSnap.mock.calls[0][0], 'the snapshot handed up is not the one the clear answered').toBe(CLEARED)
    expect(vi.mocked(pushToast).mock.calls).toEqual([[t('ota.hunt.cleared'), 'info', 2000]])
    // The host applies it, as App does the POTA / SOTA view's: the tag goes.
    view.rerender(strip(null, null, onSnap))
    expect(chip()).toBeNull()
  })

  it('a clear that fails hands nothing up and leaves the hunted park', async () => {
    mockedClear.mockImplementation(async () => {
      throw new Error('busy')
    })
    const onSnap = vi.fn()
    render(strip(HUNT, null, onSnap))
    fireEvent.change(callBox(), { target: { value: 'K9XYZ' } })
    await waitFor(() => expect(park().value).toBe('US-1000'))
    fireEvent.click(huntX())
    await waitFor(() => expect(vi.mocked(pushToast)).toHaveBeenCalledWith(t('ota.hunt.clearFailed'), 'error'))
    await settle()
    expect(onSnap).not.toHaveBeenCalled()
    expect(park().value, 'a hunt that did not end took its park out of the box').toBe('US-1000')
    expect(chip()).toBeTruthy()
  })
})

describe('the hunted park goes with the hunt', () => {
  it('takes it out of the box, and Clear no longer brings it back', async () => {
    const onSnap = vi.fn()
    const view = render(strip(HUNT, null, onSnap))
    fireEvent.change(callBox(), { target: { value: 'K9XYZ' } })
    await waitFor(() => expect(park().value).toBe('US-1000'))
    // CONTROL — the report itself, before the ✕: Clear empties the strip and the hunt fills it again.
    fireEvent.click(clearButton())
    await settle()
    expect(callBox().value).toBe('')
    expect(park().value, 'premise: Clear no longer refills the hunted park').toBe('US-1000')
    fireEvent.change(callBox(), { target: { value: 'K9XYZ' } })
    await settle()
    fireEvent.click(huntX())
    await waitFor(() => expect(onSnap).toHaveBeenCalledWith(CLEARED))
    view.rerender(strip(null, null, onSnap))
    await settle()
    expect(chip()).toBeNull()
    expect(park().value, 'the hunted park outlived its hunt').toBe('')
    fireEvent.click(clearButton())
    await settle()
    expect(park().value, 'Clear brought the hunted park back').toBe('')
    fireEvent.change(callBox(), { target: { value: 'K9XYZ' } })
    await settle()
    expect(park().value).toBe('')
    expect((await logged()).ota, 'the ended hunt’s park rode onto the contact').toBeUndefined()
  })

  // CONTROL — the same park, typed by the operator for the same station: theirs, not the hunt's.
  it('leaves a park the operator typed, even the hunted park itself', async () => {
    const onSnap = vi.fn()
    const view = render(strip(HUNT, null, onSnap))
    fireEvent.change(callBox(), { target: { value: 'K9XYZ' } })
    await waitFor(() => expect(park().value).toBe('US-1000'))
    fireEvent.change(park(), { target: { value: '' } })
    fireEvent.change(park(), { target: { value: 'US-1000' } })
    await settle()
    fireEvent.click(huntX())
    await waitFor(() => expect(onSnap).toHaveBeenCalledWith(CLEARED))
    expect(park().value, 'the ✕ took a park the operator typed').toBe('US-1000')
    view.rerender(strip(null, null, onSnap))
    await settle()
    expect(park().value).toBe('US-1000')
    expect((await logged()).ota).toEqual({ theirProgram: 'POTA', theirRef: 'US-1000' })
  })

  // CONTROL — the park an earlier hunt filled stays with its station when the hunt moves on to
  // another activator (LogEntry.huntPrefill); ending the new hunt does not take it.
  it('leaves the park an earlier hunt left with its station', async () => {
    const onSnap = vi.fn()
    const view = render(strip({ program: 'POTA', reference: 'US-2000', call: 'W1ABC' }, null, onSnap))
    fireEvent.change(callBox(), { target: { value: 'W1ABC' } })
    await waitFor(() => expect(park().value).toBe('US-2000'))
    view.rerender(strip(HUNT, null, onSnap))
    await settle()
    expect(park().value, 'premise: the earlier park stays with its station').toBe('US-2000')
    fireEvent.click(huntX())
    await waitFor(() => expect(onSnap).toHaveBeenCalledWith(CLEARED))
    expect(park().value, 'the ✕ took a park that is not the ended hunt’s').toBe('US-2000')
  })

  // CONTROL — a Needed click's park is the operator's pick for that station, even at the hunted park.
  it('leaves a park a Needed click handed over, even at the hunted park', async () => {
    const onSnap = vi.fn()
    const view = render(strip(HUNT, { call: 'K9XYZ', ts: 1, park: pota('US-1000') }, onSnap))
    await waitFor(() => expect(callBox().value).toBe('K9XYZ'))
    await settle()
    expect(park().value).toBe('US-1000')
    fireEvent.click(huntX())
    await waitFor(() => expect(onSnap).toHaveBeenCalledWith(CLEARED))
    await settle()
    expect(park().value, 'the ✕ took the park a Needed click handed over').toBe('US-1000')
    view.rerender(strip(null, { call: 'K9XYZ', ts: 1, park: pota('US-1000') }, onSnap))
    await settle()
    expect(park().value).toBe('US-1000')
    expect((await logged()).ota).toEqual({ theirProgram: 'POTA', theirRef: 'US-1000' })
  })

  // CONTROL — no hunt, no tag, nothing to end: a Needed handoff's strip is as it was.
  it('a Needed handoff with no hunt shows no tag and no ✕', async () => {
    render(strip(null, { call: 'K9XYZ', ts: 1, park: pota('US-1000') }))
    await waitFor(() => expect(park().value).toBe('US-1000'))
    expect(chip()).toBeNull()
    expect(screen.queryByRole('button', { name: t('ota.hunt.clear') })).toBeNull()
  })
})

// The hosted page provides `StationControlContext` false for the whole application
// (BrowserApplication). Its log line comes with the hosted adapter in the cockpits (RemoteLogEntry)
// and without it in the Satellites view; neither offers the ✕, whose call the page cannot make.
describe('on the hosted Remote page', () => {
  const adapter = { submit: vi.fn(async () => {}), canSubmit: true, busy: false, resetKey: 0, recall: () => null }
  for (const [name, remote, exchange] of [
    ['with the hosted adapter', adapter, 'terrestrial'],
    ['without it (Satellites)', undefined, 'satellite'],
  ] as const) {
    it(`shows the tag with no ✕, ${name}`, () => {
      render(
        <StationControlContext.Provider value={false}>
          <LogEntry snap={snapWith(HUNT)} mode="SSB" defaultRst="59" exchange={exchange} remote={remote} onSnap={vi.fn()} />
        </StationControlContext.Provider>,
      )
      expect(chip()?.textContent).toContain('US-1000')
      expect(within(chip()!).queryByRole('button'), 'the hosted page offers a clear it cannot make').toBeNull()
    })
  }
})
