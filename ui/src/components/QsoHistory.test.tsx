// @vitest-environment jsdom
//
// A CONTACT'S HISTORY and RECENTLY DELETED — what the operator sees of the log's change history,
// and that each Undo, Redo and Restore asks the backend for exactly that one thing.
import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest'
import { render, cleanup, fireEvent, screen, waitFor, act } from '@testing-library/react'
import type { QsoHistoryEntry } from '../api'
import { dismissToast, subscribeToasts, type Toast } from '../toast'
import { setRemoteStation } from '../features/durableStore'

const api = vi.hoisted(() => ({
  qsoHistory: vi.fn(),
  undoQsoChange: vi.fn(),
  undoHistoryRun: vi.fn(),
  redoQsoChange: vi.fn(),
  recentlyDeletedQsos: vi.fn(),
  restoreDeletedQso: vi.fn(),
  undoableHistoryRuns: vi.fn(),
}))
vi.mock('../api', () => api)
/** Whether the event being handled came through the Remote stream, as `isStreamInput` answers it. */
const stream = vi.hoisted(() => ({ on: false }))
vi.mock('../remote-native/stream-input', () => ({ isStreamInput: () => stream.on }))

import { QsoHistory, sayUndone } from './QsoHistory'
import { GUARD_MS, RecentlyDeleted } from './RecentlyDeleted'

let toasts: Toast[] = []
let unsubscribe = () => {}
beforeEach(() => {
  for (const f of Object.values(api)) f.mockReset()
  api.undoableHistoryRuns.mockResolvedValue([])
  unsubscribe = subscribeToasts((t) => { toasts = t })
})
afterEach(() => {
  unsubscribe()
  cleanup()
  stream.on = false
  setRemoteStation(false)
})

const entry = (over: Partial<QsoHistoryEntry>): QsoHistoryEntry => ({
  seq: 1,
  kind: 'create',
  source: 'operator',
  run: 'r1',
  atMs: Date.UTC(2026, 9, 24, 12, 0, 0),
  undoes: null,
  undoneBy: null,
  changes: [],
  ...over,
})

const done = {
  changed: 1, run: 'undo-run', kept: [], held: [], heldUnder: null, gone: 0, alreadyThere: 0, changedSince: 0, duplicate: 0,
  unknownKind: 0, changedSinceCalls: [] as string[], duplicateCalls: [] as string[], owedCalls: [] as string[],
  owedAt: [] as { call: string; services: string[] }[],
}

describe("a contact's history", () => {
  it('lists each change newest first, who made it and the fields it changed, old → new', async () => {
    api.qsoHistory.mockResolvedValue([
      entry({ seq: 1 }),
      entry({ seq: 2, kind: 'merge', source: 'lotw', changes: [{ tag: 'LOTW_QSL_RCVD', old: null, new: 'Y' }] }),
      entry({ seq: 3, kind: 'edit', changes: [{ tag: 'NAME', old: 'Bob', new: 'Robert' }] }),
    ])
    render(<QsoHistory id="1:2:3" />)
    await screen.findByText('NAME')
    expect(api.qsoHistory).toHaveBeenCalledWith('1:2:3')
    const items = [...document.querySelectorAll('.qso-history-entry')].map((li) => li.textContent ?? '')
    expect(items).toHaveLength(3)
    expect(items[0]).toContain('Edited · you')
    expect(items[0]).toContain('Bob → Robert')
    expect(items[1]).toContain('Merged · LoTW')
    expect(items[1]).toContain('(none) → Y')
    expect(items[2]).toContain('Logged · you')
    // Undo on a change of fields; none on the contact's logging.
    expect(screen.getAllByRole('button', { name: 'Undo' })).toHaveLength(2)
  })

  it('undoes one change, says so with a Redo, and the Redo undoes the undo', async () => {
    api.qsoHistory.mockResolvedValue([entry({ seq: 7, kind: 'edit', changes: [{ tag: 'COMMENT', old: null, new: 'x' }] })])
    api.undoQsoChange.mockResolvedValue(done)
    api.redoQsoChange.mockResolvedValue({ ...done, run: 'redo-run' })
    const onChanged = vi.fn()
    render(<QsoHistory id="1:2:3" onChanged={onChanged} />)
    fireEvent.click(await screen.findByRole('button', { name: 'Undo' }))
    await waitFor(() => expect(api.undoQsoChange).toHaveBeenCalledWith(7))
    await waitFor(() => expect(onChanged).toHaveBeenCalled())
    const toast = toasts.find((t) => t.message.startsWith('Change undone'))
    expect(toast?.actionLabel).toBe('Redo')
    toast?.action?.()
    // One contact's redo, which a computer driving the station may ask for as it may the undo;
    // never the whole-run undo, which is the station's alone.
    await waitFor(() => expect(api.redoQsoChange).toHaveBeenCalledWith('undo-run'))
    expect(api.undoHistoryRun).not.toHaveBeenCalled()
  })

  it("redoes a whole run's undo of several contacts as the whole run, at the station", async () => {
    api.undoHistoryRun.mockResolvedValue({ ...done, changed: 41, run: 'redo-run' })
    sayUndone({ ...done, changed: 41 }, 'Whole run undone', () => {})
    const toast = toasts.find((t) => t.message.startsWith('Whole run undone'))
    expect(toast?.actionLabel).toBe('Redo')
    toast?.action?.()
    await waitFor(() => expect(api.undoHistoryRun).toHaveBeenCalledWith('undo-run'))
    expect(api.redoQsoChange).not.toHaveBeenCalled()
  })

  it('names the contacts an undo left because they changed since, and those the log holds already', () => {
    sayUndone({
      ...done, changed: 2, changedSince: 1, duplicate: 7,
      changedSinceCalls: ['N0IMP'],
      duplicateCalls: ['K0AA', 'K1AA', 'K2AA', 'K3AA', 'K4AA', 'K5AA', 'K6AA'],
    }, 'Import undone', () => {})
    const said = toasts.find((x) => x.message.startsWith('Import undone'))?.message ?? ''
    expect(said).toContain('N0IMP was changed since, and kept')
    expect(said).toContain('7 contacts were not put back: the same station, band and mode within 5 minutes was logged again after each was deleted: K0AA, K1AA, K2AA, K3AA, K4AA and 2 more')
    sayUndone({ ...done, changed: 0, run: null, duplicate: 1, duplicateCalls: ['K2ABC'] }, 'Restored K2ABC', () => {})
    expect(toasts.some((x) => x.message === 'Nothing put back: a contact with K2ABC on the same band and mode within 5 minutes was logged again after it was deleted.')).toBe(true)
    sayUndone({ ...done, changed: 0, run: null, unknownKind: 1 }, 'Change undone', () => {})
    expect(toasts.some((x) => x.message.startsWith('That change was made by a newer version'))).toBe(true)
  })

  it('says plainly when an undone call correction may already be at the logging services', () => {
    sayUndone({ ...done, owedCalls: ['K1ABC'] }, 'Call fix undone', () => {})
    const said = toasts.find((x) => x.message.startsWith('Call fix undone'))?.message ?? ''
    expect(said).toBe('Call fix undone; K1ABC is marked not uploaded: the call this replaced may already be at your logging services, so it goes again with the next upload')
    sayUndone({ ...done, changed: 2, owedCalls: ['K1ABC', 'K2ABC'] }, 'Call fixes undone', () => {})
    expect(toasts.find((x) => x.message.startsWith('Call fixes undone'))?.message).toContain('2 contacts are marked not uploaded')
    sayUndone({ ...done }, 'Nothing owed', () => {})
    expect(toasts.find((x) => x.message.startsWith('Nothing owed'))?.message).toBe('Nothing owed')
  })

  it('names the services an undone call correction may already be at, and only those', () => {
    sayUndone({ ...done, owedAt: [{ call: 'K1ABC', services: ['LoTW'] }] }, 'One service', () => {})
    expect(toasts.find((x) => x.message.startsWith('One service'))?.message)
      .toBe('One service; K1ABC is marked not uploaded to LoTW: the call this replaced may already be there')
    sayUndone({
      ...done,
      changed: 3,
      owedAt: [
        { call: 'K1ABC', services: ['QRZ', 'Club Log'] },
        { call: 'K2ABC', services: ['QRZ', 'Club Log'] },
        { call: 'K3ABC', services: ['LoTW'] },
      ],
    }, 'Three undone', () => {})
    expect(toasts.find((x) => x.message.startsWith('Three undone'))?.message).toBe(
      'Three undone; 2 contacts are marked not uploaded to QRZ, Club Log: the calls this replaced may already be there: K1ABC, K2ABC'
        + '; K3ABC is marked not uploaded to LoTW: the call this replaced may already be there',
    )
  })

  it('says which field it kept, and the Settings call a blank would have gone under', () => {
    const held = { held: ['STATION_CALLSIGN'], heldUnder: 'KD9NEW' }
    sayUndone({ ...done, changed: 2, ...held }, 'Bulk edit undone', () => {})
    const said = toasts.find((x) => x.message.startsWith('Bulk edit undone'))?.message ?? ''
    expect(said).toBe('Bulk edit undone; STATION_CALLSIGN kept: put back blank, these contacts would be sent under KD9NEW, the call in Settings now')
    sayUndone({ ...done, changed: 0, run: null, ...held }, 'Change undone', () => {})
    expect(toasts.some((x) => x.message === 'Nothing undone: STATION_CALLSIGN was kept. Put back blank, these contacts would be sent under KD9NEW, the call in Settings now.')).toBe(true)
    // A redo that held everything says it redid nothing.
    sayUndone({ ...done, changed: 0, run: null, ...held }, 'Change made again', () => {}, true)
    expect(toasts.some((x) => x.message === 'Nothing redone: STATION_CALLSIGN was kept. Put back blank, these contacts would be sent under KD9NEW, the call in Settings now.')).toBe(true)
  })

  it('says Settings has no call when it names none, never an empty call', () => {
    const held = { held: ['STATION_CALLSIGN'], heldUnder: null }
    sayUndone({ ...done, changed: 2, ...held }, 'Fill undone', () => {})
    const said = toasts.find((x) => x.message.startsWith('Fill undone'))?.message ?? ''
    expect(said).toBe('Fill undone; STATION_CALLSIGN kept: put back blank, these contacts would name no station call, and Settings has no call either')
    sayUndone({ ...done, changed: 0, run: null, ...held }, 'Change undone', () => {})
    expect(toasts.some((x) => x.message === 'Nothing undone: STATION_CALLSIGN was kept. Put back blank, these contacts would name no station call, and Settings has no call either.')).toBe(true)
    sayUndone({ ...done, changed: 0, run: null, ...held }, 'Change made again', () => {}, true)
    expect(toasts.some((x) => x.message === 'Nothing redone: STATION_CALLSIGN was kept. Put back blank, these contacts would name no station call, and Settings has no call either.')).toBe(true)
    expect(toasts.some((x) => x.message.includes('under ,'))).toBe(false)
  })

  it("a Redo that held everything says nothing was redone", async () => {
    for (const x of [...toasts]) dismissToast(x.id)
    expect(toasts).toEqual([])
    api.qsoHistory.mockResolvedValue([entry({ seq: 7, kind: 'edit', changes: [{ tag: 'STATION_CALLSIGN', old: 'KA1OLD', new: null }] })])
    api.undoQsoChange.mockResolvedValue(done)
    // A one-contact undo is redone through the one-contact redo.
    api.redoQsoChange.mockResolvedValue({ ...done, changed: 0, run: null, held: ['STATION_CALLSIGN'], heldUnder: 'KD9NEW' })
    render(<QsoHistory id="1:2:3" />)
    fireEvent.click(await screen.findByRole('button', { name: 'Undo' }))
    await waitFor(() => expect(toasts.some((t) => t.message.startsWith('Change undone'))).toBe(true))
    toasts.find((t) => t.message.startsWith('Change undone'))?.action?.()
    await waitFor(() => expect(toasts.some((t) => t.message.startsWith('Nothing redone: STATION_CALLSIGN was kept'))).toBe(true))
    expect(toasts.some((t) => t.message.startsWith('Nothing undone'))).toBe(false)
  })

  it('shows an undone change as undone, with no Undo of its own', async () => {
    api.qsoHistory.mockResolvedValue([
      entry({ seq: 2, kind: 'edit', undoneBy: 3, changes: [{ tag: 'NAME', old: 'A', new: 'B' }] }),
      entry({ seq: 3, kind: 'edit', source: 'undo', undoes: 2, changes: [{ tag: 'NAME', old: 'B', new: 'A' }] }),
    ])
    render(<QsoHistory id="1:2:3" />)
    await screen.findByText('Undone')
    // The undo itself can be undone: that is the redo.
    expect(screen.getAllByRole('button', { name: 'Undo' })).toHaveLength(1)
  })

  it('says when the history cannot be read, rather than showing nothing', async () => {
    api.qsoHistory.mockRejectedValue('no store')
    render(<QsoHistory id="1:2:3" />)
    await screen.findByText('The history could not be read.')
  })
})

describe('recently deleted', () => {
  it('lists the deleted contacts and restores the one asked for', async () => {
    api.recentlyDeletedQsos.mockResolvedValue([
      { id: 'a', call: 'K1ABC', band: '20m', mode: 'FT8', whenUnix: 1_782_583_500, deletedAtMs: 0, source: 'operator' },
    ])
    api.restoreDeletedQso.mockResolvedValue(done)
    const onRestored = vi.fn()
    render(<RecentlyDeleted open onClose={() => {}} onRestored={onRestored} />)
    expect(await screen.findByText('K1ABC')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Restore' }))
    await waitFor(() => expect(api.restoreDeletedQso).toHaveBeenCalledWith('a', false))
    await waitFor(() => expect(onRestored).toHaveBeenCalled())
    expect(toasts.some((t) => t.message.startsWith('Restored K1ABC'))).toBe(true)
  })

  it('offers Restore anyway once a restore is refused because the contact was logged again', async () => {
    api.recentlyDeletedQsos.mockResolvedValue([
      { id: 'a', call: 'K1ABC', band: '20m', mode: 'FT8', whenUnix: 1_782_583_500, deletedAtMs: 0, source: 'operator' },
    ])
    api.restoreDeletedQso.mockResolvedValueOnce({
      ...done, changed: 0, run: null, duplicate: 1, duplicateCalls: ['K1ABC'],
    })
    render(<RecentlyDeleted open onClose={() => {}} onRestored={() => {}} />)
    fireEvent.click(await screen.findByRole('button', { name: 'Restore' }))
    await waitFor(() => expect(api.restoreDeletedQso).toHaveBeenCalledWith('a', false))
    await waitFor(() => expect(toasts.some((t) => t.message.includes('logged again after it was deleted'))).toBe(true))
    // Said in words on the row, not only in a tooltip.
    expect(screen.getByText(/logged again after this contact was deleted/)).toBeTruthy()
    api.restoreDeletedQso.mockResolvedValueOnce(done)
    const later = Date.now() + GUARD_MS + 1
    vi.spyOn(Date, 'now').mockReturnValue(later)
    fireEvent.click(await screen.findByRole('button', { name: 'Restore anyway' }))
    await waitFor(() => expect(api.restoreDeletedQso).toHaveBeenLastCalledWith('a', true))
    vi.restoreAllMocks()
  })

  it('never turns a double-click on Restore into Restore anyway', async () => {
    api.recentlyDeletedQsos.mockResolvedValue([
      { id: 'a', call: 'K1ABC', band: '20m', mode: 'FT8', whenUnix: 1_782_583_500, deletedAtMs: 0, source: 'operator' },
    ])
    api.restoreDeletedQso.mockResolvedValue({
      ...done, changed: 0, run: null, duplicate: 1, duplicateCalls: ['K1ABC'],
    })
    render(<RecentlyDeleted open onClose={() => {}} onRestored={() => {}} />)
    const restore = await screen.findByRole('button', { name: 'Restore' })
    fireEvent.click(restore)
    // The Duplicate answer is in; the double-click's second click lands where the first did, and
    // a quick hand reaches for the new button too.
    const anyway = await screen.findByRole('button', { name: 'Restore anyway' })
    expect(anyway).not.toBe(restore)
    fireEvent.click(screen.getByRole('button', { name: 'Restore' }))
    fireEvent.click(anyway)
    await new Promise((r) => setTimeout(r, 20))
    expect(api.restoreDeletedQso.mock.calls).toEqual([['a', false]])
  })

  it('never sends two restores for two clicks before the first answers', async () => {
    api.recentlyDeletedQsos.mockResolvedValue([
      { id: 'a', call: 'K1ABC', band: '20m', mode: 'FT8', whenUnix: 1_782_583_500, deletedAtMs: 0, source: 'operator' },
      { id: 'b', call: 'K2XYZ', band: '40m', mode: 'CW', whenUnix: 1_782_583_600, deletedAtMs: 0, source: 'operator' },
    ])
    api.restoreDeletedQso.mockReturnValue(new Promise(() => {}))
    render(<RecentlyDeleted open onClose={() => {}} onRestored={() => {}} />)
    await screen.findByText('K2XYZ')
    const [a, b] = screen.getAllByRole('button', { name: 'Restore' })
    // Both before the screen redraws them disabled.
    act(() => {
      a.click()
      b.click()
    })
    expect(api.restoreDeletedQso.mock.calls).toEqual([['a', false]])
  })

  const at = Date.UTC(2026, 9, 12, 12, 0, 0)
  const purge = { run: 'purge-run', kind: 'purge', atMs: at, contacts: 500, removes: false }
  const imported = { run: 'import-run', kind: 'import', atMs: at, contacts: 41, removes: true }
  const undone = { run: 'undo-run', kind: 'undo', atMs: at, contacts: 41, removes: true }

  it('undoes a whole purge after its toast has gone, once asked, and keeps it listed when the undo is refused', async () => {
    api.recentlyDeletedQsos.mockResolvedValue([])
    api.undoableHistoryRuns.mockResolvedValue([purge])
    api.undoHistoryRun.mockRejectedValueOnce(
      'Stop transmitting first: undoing this many contacts holds up the radio while it is made.',
    )
    const onRestored = vi.fn()
    render(<RecentlyDeleted open onClose={() => {}} onRestored={onRestored} />)
    expect(await screen.findByText('Purge of 500 contacts · 2026-10-12 12:00:00Z')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Undo this purge' }))
    expect(screen.getByText('Undo this purge of 500 contacts from 2026-10-12? They will be put back in the log.')).toBeTruthy()
    expect(api.undoHistoryRun).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'Undo it' }))
    await waitFor(() => expect(toasts.some((t) => t.message.includes('Stop transmitting first'))).toBe(true))
    expect(api.undoHistoryRun).toHaveBeenCalledWith('purge-run')
    expect(onRestored).not.toHaveBeenCalled()

    // Transmit off: pressed again, and asked again, it is made.
    api.undoHistoryRun.mockResolvedValueOnce({ ...done, changed: 500 })
    vi.spyOn(Date, 'now').mockReturnValue(Date.now() + GUARD_MS + 1)
    fireEvent.click(screen.getByRole('button', { name: 'Undo this purge' }))
    fireEvent.click(screen.getByRole('button', { name: 'Undo it' }))
    await waitFor(() => expect(onRestored).toHaveBeenCalled())
    expect(api.undoHistoryRun.mock.calls).toEqual([['purge-run'], ['purge-run']])
    expect(toasts.some((t) => t.message.startsWith('Purge undone'))).toBe(true)
    vi.restoreAllMocks()
  })

  it('asks before undoing a whole import, naming how many and when, and Cancel deletes nothing', async () => {
    api.recentlyDeletedQsos.mockResolvedValue([])
    api.undoableHistoryRuns.mockResolvedValue([imported])
    api.undoHistoryRun.mockResolvedValue({ ...done, changed: 41 })
    render(<RecentlyDeleted open onClose={() => {}} onRestored={() => {}} />)
    fireEvent.click(await screen.findByRole('button', { name: 'Undo this import' }))
    expect(screen.getByText('Undo this import of 41 contacts from 2026-10-12? They will be removed from the log.')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    expect(screen.queryByRole('button', { name: 'Undo it' })).toBeNull()
    await new Promise((r) => setTimeout(r, 20))
    expect(api.undoHistoryRun).not.toHaveBeenCalled()

    fireEvent.click(screen.getByRole('button', { name: 'Undo this import' }))
    fireEvent.click(screen.getByRole('button', { name: 'Undo it' }))
    await waitFor(() => expect(api.undoHistoryRun.mock.calls).toEqual([['import-run']]))
    await waitFor(() => expect(toasts.some((t) => t.message.startsWith('Import undone'))).toBe(true))
  })

  it('lists the one-time contest-column copy for one undo of the whole of it, saying what it does', async () => {
    const copy = { run: 'copy-run', kind: 'contestColumns', atMs: at, contacts: 312, removes: false }
    api.recentlyDeletedQsos.mockResolvedValue([])
    api.undoableHistoryRuns.mockResolvedValue([copy])
    api.undoHistoryRun.mockResolvedValue({ ...done, changed: 312 })
    render(<RecentlyDeleted open onClose={() => {}} onRestored={() => {}} />)
    expect(await screen.findByText('Contest grid, name and QTH copied into 312 contacts · 2026-10-12 12:00:00Z')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Undo this copy' }))
    expect(screen.getByText(
      'Undo copying the contest grid, name and QTH into 312 contacts from 2026-10-12? The copied fields are blank again; the contest exchange keeps the values.',
    )).toBeTruthy()
    expect(api.undoHistoryRun).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'Undo it' }))
    await waitFor(() => expect(api.undoHistoryRun.mock.calls).toEqual([['copy-run']]))
    await waitFor(() => expect(toasts.some((t) => t.message.startsWith('Copy of contest grid, name and QTH undone'))).toBe(true))
  })

  it('lists a whole undo with Redo, through the same question and the same undo', async () => {
    api.recentlyDeletedQsos.mockResolvedValue([])
    api.undoableHistoryRuns.mockResolvedValue([undone])
    api.undoHistoryRun.mockResolvedValue({ ...done, changed: 41 })
    render(<RecentlyDeleted open onClose={() => {}} onRestored={() => {}} />)
    expect(await screen.findByText('Undo of 41 contacts · 2026-10-12 12:00:00Z')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Redo' }))
    expect(screen.getByText('Redo the undo of 41 contacts from 2026-10-12? They will be removed from the log.')).toBeTruthy()
    expect(api.undoHistoryRun).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'Redo it' }))
    await waitFor(() => expect(api.undoHistoryRun.mock.calls).toEqual([['undo-run']]))
    await waitFor(() => expect(toasts.some((t) => t.message.startsWith('Change made again'))).toBe(true))
  })

  it('never sends two whole undos for two clicks before the first answers', async () => {
    api.recentlyDeletedQsos.mockResolvedValue([])
    api.undoableHistoryRuns.mockResolvedValue([imported])
    api.undoHistoryRun.mockReturnValue(new Promise(() => {}))
    render(<RecentlyDeleted open onClose={() => {}} onRestored={() => {}} />)
    fireEvent.click(await screen.findByRole('button', { name: 'Undo this import' }))
    const go = screen.getByRole('button', { name: 'Undo it' })
    act(() => {
      go.click()
      go.click()
    })
    expect(api.undoHistoryRun.mock.calls).toEqual([['import-run']])
  })

  it('driving the station from another computer, lists the runs but undoes none: only at the station', async () => {
    setRemoteStation(true)
    api.recentlyDeletedQsos.mockResolvedValue([])
    api.undoableHistoryRuns.mockResolvedValue([imported, undone])
    render(<RecentlyDeleted open onClose={() => {}} onRestored={() => {}} />)
    const undo = await screen.findByRole('button', { name: 'Undo this import' })
    expect(undo).toHaveProperty('disabled', true)
    expect(screen.getByRole('button', { name: 'Redo' })).toHaveProperty('disabled', true)
    expect(screen.getAllByText('Only at the station')).toHaveLength(2)
    fireEvent.click(undo)
    expect(screen.queryByRole('button', { name: 'Undo it' })).toBeNull()
    expect(api.undoHistoryRun).not.toHaveBeenCalled()
  })

  it('a press through the Remote stream undoes no run, and says so', async () => {
    api.recentlyDeletedQsos.mockResolvedValue([])
    api.undoableHistoryRuns.mockResolvedValue([imported])
    api.undoHistoryRun.mockResolvedValue({ ...done, changed: 41 })
    render(<RecentlyDeleted open onClose={() => {}} onRestored={() => {}} />)
    const undo = await screen.findByRole('button', { name: 'Undo this import' })
    stream.on = true
    fireEvent.click(undo)
    expect(screen.queryByRole('button', { name: 'Undo it' })).toBeNull()
    expect(screen.getByText('Only at the station: a press through Remote undoes no purge, import or undo.')).toBeTruthy()

    // Asked at the station, then the confirm pressed through the stream: still nothing.
    stream.on = false
    fireEvent.click(undo)
    const go = screen.getByRole('button', { name: 'Undo it' })
    stream.on = true
    fireEvent.click(go)
    await new Promise((r) => setTimeout(r, 20))
    expect(api.undoHistoryRun).not.toHaveBeenCalled()
  })

  it('says when nothing has been deleted', async () => {
    api.recentlyDeletedQsos.mockResolvedValue([])
    render(<RecentlyDeleted open onClose={() => {}} onRestored={() => {}} />)
    await screen.findByText('No deleted contacts.')
  })
})
