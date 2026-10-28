// @vitest-environment jsdom
//
// UPDATE FROM CALLBOOK — what the tab asks the station for and tells the operator: the preview's
// counts, time and the day's lookups against the cap; a start only after asking, on exactly the
// contacts previewed with their count; the progress while it runs, and Stop; and however it ends,
// the toast whose Undo takes the whole fill back, the calls nobody knows and the calls left for
// another run — reported by the Logbook, so it comes with the dialog closed too. Driving another
// station, it can be previewed and not started.
import { useState } from 'react'
import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest'
import { render, cleanup, fireEvent, screen, waitFor, act } from '@testing-library/react'
import type { CallbookUpdateEnd, CallbookUpdatePreview, CallbookUpdateStatus } from '../api'
import { subscribeToasts, type Toast } from '../toast'
import { setRemoteStation } from '../features/durableStore'
import { DEFAULT_LOG_QUERY } from '../features/logQuery'

const api = vi.hoisted(() => ({
  callbookUpdatePreview: vi.fn(),
  callbookUpdateStart: vi.fn(),
  callbookUpdateStatus: vi.fn(),
  callbookUpdateCancel: vi.fn(),
  callbookUpdateTakeEnd: vi.fn(),
  undoHistoryRun: vi.fn(),
}))
vi.mock('../api', () => api)

import { LogTools } from './LogTools'
import { POLL_MS, useCallbookUpdateEnd } from './LogCallbookUpdate'

let toasts: Toast[] = []
let unsubscribe = () => {}
beforeEach(() => {
  for (const f of Object.values(api)) f.mockReset()
  api.callbookUpdateStatus.mockResolvedValue(idle)
  api.callbookUpdateCancel.mockResolvedValue(undefined)
  api.callbookUpdateTakeEnd.mockResolvedValue(null)
  unsubscribe = subscribeToasts((t) => { toasts = t })
})
afterEach(() => {
  unsubscribe()
  cleanup()
  setRemoteStation(false)
  vi.useRealTimers()
})

const idle: CallbookUpdateStatus = {
  running: false, done: 0, total: 0, fromCache: 0, lookedUp: 0, spent: 0, cap: 250,
  waitingForTx: false, qrzSaid: null, qrzSkipped: false, ended: null,
}

const preview: CallbookUpdatePreview = {
  ids: ['x', 'y', 'z'],
  selected: 3,
  missing: 0,
  calls: 2,
  kept: 1,
  toLookUp: 1,
  lookupsNeeded: 1,
  paceSecs: 2,
  estimateSecs: 2,
  spent: 10,
  cap: 250,
  qrzCount: null,
  // Fewer than selected: the start must send the count previewed, not the count it changes.
  couldChange: 2,
  fields: [['GRIDSQUARE', 3], ['NAME', 2]],
  configured: true,
  services: ['qrz', 'hamqth', 'callook'],
  locationTooOld: 0,
}

/** The Logbook's part: the end's reporter beside the dialog, which the operator can close. */
function Logbook({ picked, onChanged }: { picked: string[], onChanged: () => void }) {
  const follow = useCallbookUpdateEnd(onChanged)
  const [shut, setShut] = useState(false)
  return (
    <>
      <button type="button" onClick={() => setShut((x) => !x)}>toggle dialog</button>
      <LogTools
        open={!shut}
        tool="callbook"
        onTool={() => {}}
        onClose={() => setShut(true)}
        onChanged={onChanged}
        onCallbookStarted={follow}
        picked={new Set(picked)}
        query={DEFAULT_LOG_QUERY}
        shown={42}
      />
    </>
  )
}

function open(picked: string[] = []) {
  const onChanged = vi.fn()
  render(<Logbook picked={picked} onChanged={onChanged} />)
  return onChanged
}

const toggle = () => fireEvent.click(screen.getByRole('button', { name: 'toggle dialog', hidden: true }))

const undoAnswer = {
  changed: 3, run: 'undo-1', kept: [], held: [], heldUnder: null, gone: 0, alreadyThere: 0,
  changedSince: 0, duplicate: 0, unknownKind: 0, changedSinceCalls: [], duplicateCalls: [], owedCalls: [], owedAt: [],
}

/** Preview, the button, Start: an update running. */
async function start() {
  fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
  fireEvent.click(await screen.findByRole('button', { name: 'Update up to 2 contacts' }))
  fireEvent.click(screen.getByRole('button', { name: 'Start' }))
  await waitFor(() => expect(api.callbookUpdateStart).toHaveBeenCalled())
}

describe('update from callbook', () => {
  it('previews the calls, the time, the day’s lookups and the fields, by value', async () => {
    api.callbookUpdatePreview.mockResolvedValue({ ...preview, toLookUp: 300, lookupsNeeded: 300, estimateSecs: 600 })
    open(['x', 'y', 'z'])
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    await screen.findByText('3 contacts, 2 different calls: 1 already looked up, 300 to look up.')
    expect(api.callbookUpdatePreview).toHaveBeenCalledWith({ ids: ['x', 'y', 'z'] })
    screen.getByText('About 10 min, at one lookup every 2 s.')
    screen.getByText('Lookups today: 10 of 250 (Settings ▸ Logbook ▸ Callbook lookups).')
    screen.getByText(/Today’s limit stops it after 240 lookups\./)
    screen.getByText('GRIDSQUARE 3 · NAME 2')
    screen.getByText(
      'Asks QRZ, then HamQTH, then callook.info (US calls), the next only when the one before has no record. The pace and the daily limit cover them all. HamQTH data provided by HamQTH.com.',
    )
    screen.getByText(/^Location \(grid, QTH, state, county, CQ and ITU zones, latitude\/longitude, UTC offset\) is filled only on contacts from the last 12 months/)
    expect(screen.queryByText(/older than 12 months/)).toBeNull()
  })

  it('names only the services set up, and counts the older contacts whose location stays blank', async () => {
    api.callbookUpdatePreview.mockResolvedValue({ ...preview, services: ['hamqth'], locationTooOld: 7 })
    open(['x', 'y', 'z'])
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    await screen.findByText('7 contacts are older than 12 months: their location stays blank.')
    screen.getByText(/^Asks HamQTH, the next only when/)
    expect(screen.queryByText(/^Asks QRZ/)).toBeNull()
  })

  it('starts only after asking, on the contacts previewed with their count, then shows progress', async () => {
    api.callbookUpdatePreview.mockResolvedValue(preview)
    api.callbookUpdateStart.mockResolvedValue(undefined)
    open(['x', 'y', 'z'])
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Update up to 2 contacts' }))
    expect(api.callbookUpdateStart).not.toHaveBeenCalled()
    screen.getByText(/Look up 1 call now\?/)
    fireEvent.click(screen.getByRole('button', { name: 'Start' }))
    await waitFor(() => expect(api.callbookUpdateStart).toHaveBeenCalledWith(['x', 'y', 'z'], 3))
    await screen.findByText('Looked up 0 of 2 calls (0 from the cache) · lookups today: 10 of 250')
    fireEvent.click(screen.getByRole('button', { name: 'Stop' }))
    expect(api.callbookUpdateCancel).toHaveBeenCalled()
  })

  it('reports how it ended: one toast whose Undo takes the run back, the unknown calls and the rest', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    api.callbookUpdatePreview.mockResolvedValue(preview)
    api.callbookUpdateStart.mockResolvedValue(undefined)
    api.undoHistoryRun.mockResolvedValue(undoAnswer)
    const onChanged = open(['x', 'y', 'z'])
    await start()
    const end: CallbookUpdateEnd = {
      stop: 'limit',
      error: null,
      qrzSaid: null,
      qrzSkipped: false,
      unknown: ['Q9QQQ'],
      notLookedUp: 1,
      made: { changed: 3, run: 'run-7', gone: 0, changedSince: 0, needsChoice: 0, distinct: 0 },
    }
    api.callbookUpdateStatus.mockResolvedValue({
      running: false, done: 1, total: 2, fromCache: 1, lookedUp: 0, spent: 250, cap: 250,
      waitingForTx: false, qrzSaid: null, qrzSkipped: false, ended: end,
    })
    api.callbookUpdateTakeEnd.mockResolvedValueOnce(end)
    await act(async () => { await vi.advanceTimersByTimeAsync(POLL_MS + 10) })
    await screen.findByText('Stopped at today’s limit of 250 lookups. What was found is filled in.')
    screen.getByText('1 call not looked up yet: run the update again to continue.')
    screen.getByText('Q9QQQ')
    const toast = toasts.find((x) => x.message === 'Updated 3 contacts from the callbook')
    expect(toast?.actionLabel).toBe('Undo')
    expect(onChanged).toHaveBeenCalled()
    await act(async () => { await toast?.action?.() })
    expect(api.undoHistoryRun).toHaveBeenCalledWith('run-7')
    // Told once: a later poll does not toast again.
    const n = toasts.length
    await act(async () => { await vi.advanceTimersByTimeAsync(POLL_MS * 3) })
    expect(toasts.length).toBe(n)
  })

  it('reports the end with the dialog closed mid-run: the toast, its Undo and the reload, once; reopened, the end', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    api.callbookUpdatePreview.mockResolvedValue(preview)
    api.callbookUpdateStart.mockResolvedValue(undefined)
    api.undoHistoryRun.mockResolvedValue(undoAnswer)
    const onChanged = open(['x', 'y', 'z'])
    await start()
    toggle()
    expect(screen.queryByRole('button', { name: 'Stop' })).toBeNull()
    // Still running a while with the dialog closed: nothing is said.
    await act(async () => { await vi.advanceTimersByTimeAsync(POLL_MS * 3) })
    expect(toasts.find((x) => x.message === 'Updated 4 contacts from the callbook')).toBeUndefined()
    const end: CallbookUpdateEnd = {
      stop: 'done', error: null, qrzSaid: null, qrzSkipped: false, unknown: [], notLookedUp: 0,
      made: { changed: 4, run: 'run-9', gone: 0, changedSince: 0, needsChoice: 0, distinct: 0 },
    }
    api.callbookUpdateTakeEnd.mockResolvedValueOnce(end)
    api.callbookUpdateStatus.mockResolvedValue({ ...idle, done: 2, total: 2, ended: end })
    await act(async () => { await vi.advanceTimersByTimeAsync(POLL_MS + 10) })
    const toast = toasts.find((x) => x.message === 'Updated 4 contacts from the callbook')
    expect(toast?.actionLabel).toBe('Undo')
    expect(onChanged).toHaveBeenCalledTimes(1)
    await act(async () => { await toast?.action?.() })
    expect(api.undoHistoryRun).toHaveBeenCalledWith('run-9')
    const n = toasts.length
    await act(async () => { await vi.advanceTimersByTimeAsync(POLL_MS * 3) })
    expect(toasts.length).toBe(n)
    expect(api.callbookUpdateTakeEnd.mock.calls.length).toBeLessThanOrEqual(5)
    // Opened again: how it ended.
    toggle()
    await screen.findByText('Every call was looked up.')
  })

  it('an end that came while the Logbook was away is reported when it comes back', async () => {
    api.callbookUpdateTakeEnd.mockResolvedValueOnce({
      stop: 'cancelled', error: null, qrzSaid: null, qrzSkipped: false, unknown: [], notLookedUp: 3,
      made: { changed: 2, run: 'run-3', gone: 0, changedSince: 0, needsChoice: 0, distinct: 0 },
    } satisfies CallbookUpdateEnd)
    const onChanged = open()
    await waitFor(() => expect(toasts.find((x) => x.message === 'Updated 2 contacts from the callbook')).toBeDefined())
    expect(onChanged).toHaveBeenCalledTimes(1)
  })

  it('QRZ refusing: says so in a toast and in the pane', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    api.callbookUpdatePreview.mockResolvedValue(preview)
    api.callbookUpdateStart.mockResolvedValue(undefined)
    open(['x', 'y', 'z'])
    await start()
    const end: CallbookUpdateEnd = {
      stop: 'refused', error: null, qrzSaid: null, qrzSkipped: false, unknown: [], notLookedUp: 2, made: null,
    }
    api.callbookUpdateTakeEnd.mockResolvedValueOnce(end)
    api.callbookUpdateStatus.mockResolvedValue({ ...idle, ended: end })
    await act(async () => { await vi.advanceTimersByTimeAsync(POLL_MS + 10) })
    const said = /^Stopped: QRZ answered 3 calls in a row with no record/
    await waitFor(() => expect(toasts.find((x) => said.test(x.message))?.kind).toBe('error'))
    screen.getByText(said)
  })

  it('waiting for the radio: says so while the lookups are done and the fill waits', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    api.callbookUpdatePreview.mockResolvedValue(preview)
    api.callbookUpdateStart.mockResolvedValue(undefined)
    open(['x', 'y', 'z'])
    await start()
    api.callbookUpdateStatus.mockResolvedValue({
      ...idle, running: true, done: 2, total: 2, waitingForTx: true,
    })
    await act(async () => { await vi.advanceTimersByTimeAsync(POLL_MS + 10) })
    await screen.findByText('Waiting for the transmission to end before the contacts are changed…')
    screen.getByRole('button', { name: 'Stop' })
  })

  it('says exactly how many when every call is already looked up, and "up to" when some are not', async () => {
    api.callbookUpdatePreview.mockResolvedValue({ ...preview, kept: 2, toLookUp: 0, lookupsNeeded: 0 })
    open(['x', 'y', 'z'])
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    await screen.findByRole('button', { name: 'Update 2 contacts' })
  })

  it('the over-cap warning counts a stroked call twice and gives QRZ’s own count', async () => {
    api.callbookUpdatePreview.mockResolvedValue({
      ...preview, toLookUp: 5, lookupsNeeded: 7, spent: 245, cap: 250, qrzCount: 245,
    })
    open(['x', 'y', 'z'])
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    await screen.findByText(/Today’s limit stops it after 5 lookups\./)
    screen.getByText(/A call with a stroke is asked again for its base call: up to 7 lookups in all\./)
    screen.getByText(/QRZ counted 245 lookups on your account in the last day\./)
  })

  it('a stroked call alone can pass the cap: warned though the calls fit', async () => {
    api.callbookUpdatePreview.mockResolvedValue({ ...preview, toLookUp: 1, lookupsNeeded: 2, spent: 249 })
    open(['x', 'y', 'z'])
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    await screen.findByText(/Today’s limit stops it after 1 lookup\./)
  })

  it('credits HamQTH only when it is asked', async () => {
    api.callbookUpdatePreview.mockResolvedValue({ ...preview, services: ['qrz', 'callook'] })
    open(['x', 'y', 'z'])
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    await screen.findByText(/^Asks QRZ, then callook\.info/)
    expect(screen.queryByText(/HamQTH\.com/)).toBeNull()
  })

  it('shows what QRZ said, in its words, and that QRZ is left out without the XML subscription', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    api.callbookUpdatePreview.mockResolvedValue(preview)
    api.callbookUpdateStart.mockResolvedValue(undefined)
    open(['x', 'y', 'z'])
    await start()
    api.callbookUpdateStatus.mockResolvedValue({
      ...idle, running: true, done: 1, total: 2, qrzSkipped: true,
      qrzSaid: 'A subscription is required to obtain the complete data.',
    })
    await act(async () => { await vi.advanceTimersByTimeAsync(POLL_MS + 10) })
    await screen.findByText('QRZ XML subscription needed for bulk updates; using HamQTH/callook.info.')
    screen.getByText('A subscription is required to obtain the complete data.')
    const end: CallbookUpdateEnd = {
      stop: 'failed', error: 'QRZ refused service for this account ("Connection refused").',
      qrzSaid: 'Connection refused', qrzSkipped: false, unknown: [], notLookedUp: 1, made: null,
    }
    api.callbookUpdateTakeEnd.mockResolvedValueOnce(end)
    api.callbookUpdateStatus.mockResolvedValue({ ...idle, ended: end })
    await act(async () => { await vi.advanceTimersByTimeAsync(POLL_MS + 10) })
    await screen.findByText('Connection refused')
    screen.getByText('QRZ says:', { exact: false })
    await waitFor(() => expect(toasts.find((x) => x.message.startsWith('QRZ refused service'))?.kind).toBe('error'))
  })

  it('reports the end once when two takes overlap: one toast', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: false })
    api.callbookUpdatePreview.mockResolvedValue(preview)
    api.callbookUpdateStart.mockResolvedValue(undefined)
    open(['x', 'y', 'z'])
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    await act(async () => { await vi.advanceTimersByTimeAsync(0) })
    fireEvent.click(screen.getByRole('button', { name: 'Update up to 2 contacts' }))
    fireEvent.click(screen.getByRole('button', { name: 'Start' }))
    await act(async () => { await vi.advanceTimersByTimeAsync(0) })
    expect(api.callbookUpdateStart).toHaveBeenCalled()
    // Each take waits on the station until released: the second starts while the first is out.
    const pending: ((e: CallbookUpdateEnd | null) => void)[] = []
    api.callbookUpdateTakeEnd.mockImplementation(
      () => new Promise<CallbookUpdateEnd | null>((resolve) => { pending.push(resolve) }),
    )
    await act(async () => { await vi.advanceTimersByTimeAsync(POLL_MS) })
    await act(async () => { await vi.advanceTimersByTimeAsync(POLL_MS) })
    expect(pending.length).toBe(2)
    const end: CallbookUpdateEnd = {
      stop: 'done', error: null, qrzSaid: null, qrzSkipped: false, unknown: [], notLookedUp: 0,
      made: { changed: 5, run: 'run-8', gone: 0, changedSince: 0, needsChoice: 0, distinct: 0 },
    }
    await act(async () => {
      for (const resolve of pending) resolve(end)
      await vi.advanceTimersByTimeAsync(0)
    })
    expect(toasts.filter((x) => x.message === 'Updated 5 contacts from the callbook')).toHaveLength(1)
  })

  it('with no callbook set up, says so and cannot start', async () => {
    api.callbookUpdatePreview.mockResolvedValue({ ...preview, configured: false })
    open(['x'])
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    const apply = await screen.findByRole('button', { name: 'Update up to 2 contacts' })
    expect((apply as HTMLButtonElement).disabled).toBe(true)
    screen.getByText('Set up QRZ or HamQTH in Settings ▸ Logbook ▸ Callbook lookups first.')
  })

  it('driving another station, it previews and cannot be started', async () => {
    setRemoteStation(true)
    api.callbookUpdatePreview.mockResolvedValue(preview)
    open()
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    const apply = await screen.findByRole('button', { name: 'Update up to 2 contacts' })
    expect((apply as HTMLButtonElement).disabled).toBe(true)
  })
})
