// @vitest-environment jsdom
//
// THE CLEAN-UP TOOLS — what each asks the backend for: a merge sends the copy kept and the copy
// each conflicting field is taken from, and waits for a guarded conflict to be chosen; a bulk edit
// previews, names the count, asks once more, and sends exactly the fields and contacts chosen; the
// check sends only the fixes ticked, each with what it found. Each change's toast offers Undo of
// its whole run. Driving another station, nothing that changes the log can be pressed.
import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest'
import { render, cleanup, fireEvent, screen, waitFor } from '@testing-library/react'
import type { DupeGroup, LogProblem } from '../api'
import type { LoggedQso } from '../types'
import { subscribeToasts, type Toast } from '../toast'
import { setRemoteStation } from '../features/durableStore'
import { DEFAULT_LOG_QUERY } from '../features/logQuery'

const api = vi.hoisted(() => ({
  findLogDuplicates: vi.fn(),
  mergeLogDuplicates: vi.fn(),
  bulkEditPreview: vi.fn(),
  bulkEditApply: vi.fn(),
  checkLogbook: vi.fn(),
  applyLogFixes: vi.fn(),
  undoHistoryRun: vi.fn(),
}))
vi.mock('../api', () => api)

import { LogTools, type LogTool } from './LogTools'
import { LogBulkEdit } from './LogBulkEdit'

/** The last of a list (the lib target here predates `Array.prototype.at`). */
const last = <T,>(xs: T[]): T => xs[xs.length - 1]

let toasts: Toast[] = []
let unsubscribe = () => {}
beforeEach(() => {
  for (const f of Object.values(api)) f.mockReset()
  unsubscribe = subscribeToasts((t) => { toasts = t })
})
afterEach(() => {
  unsubscribe()
  cleanup()
  setRemoteStation(false)
})

const made = { changed: 2, run: 'run-1', gone: 0, changedSince: 0, needsChoice: 0, distinct: 0 }

const qso = (id: string, whenUnix: number, over: Partial<LoggedQso> = {}): LoggedQso =>
  ({ id, call: 'W1AW', band: '20m', mode: 'FT8', whenUnix, freqMhz: 14.074, ...over }) as LoggedQso

function open(tool: LogTool, picked: string[] = []) {
  const onChanged = vi.fn()
  render(
    <LogTools
      open
      tool={tool}
      onTool={() => {}}
      onClose={() => {}}
      onChanged={onChanged}
      onCallbookStarted={() => {}}
      picked={new Set(picked)}
      query={DEFAULT_LOG_QUERY}
      shown={42}
    />,
  )
  return onChanged
}

describe('duplicates', () => {
  const group: DupeGroup = {
    copies: [qso('a', 1_767_268_800, { confirmed: true }), qso('b', 1_767_268_860)],
    revisions: ['ka', 'kb'],
    keep: 1,
    conflicts: [
      { tag: 'GRIDSQUARE', values: [{ id: 'a', value: 'FN31' }, { id: 'b', value: 'FN42' }], guarded: true, chosen: 'b' },
      { tag: 'COMMENT', values: [{ id: 'a', value: 'one' }, { id: 'b', value: 'two' }], guarded: false, chosen: 'b' },
    ],
  }

  it('waits for a guarded conflict to be chosen, then merges with the copy kept and each choice', async () => {
    api.findLogDuplicates.mockResolvedValue({ scanned: 10, groups: [group] })
    api.mergeLogDuplicates.mockResolvedValue(made)
    api.undoHistoryRun.mockResolvedValue({
      changed: 2, run: 'u', kept: [], held: [], gone: 0, alreadyThere: 0, changedSince: 0, duplicate: 0,
      unknownKind: 0, changedSinceCalls: [], duplicateCalls: [], owedCalls: [], owedAt: [],
    })
    const onChanged = open('dupes')
    fireEvent.click(last(screen.getAllByRole('button', { name: 'Find' })))
    const merge = await screen.findByRole('button', { name: 'Merge 2 into one' })
    expect(api.findLogDuplicates).toHaveBeenCalledWith('rule')
    expect((merge as HTMLButtonElement).disabled).toBe(true)
    expect(screen.getByRole('button', { name: 'Merge all 0 ready' })).toHaveProperty('disabled', true)

    fireEvent.click(screen.getByRole('radio', { name: 'FN31' }))
    expect((merge as HTMLButtonElement).disabled).toBe(false)
    fireEvent.click(merge)
    await waitFor(() => expect(api.mergeLogDuplicates).toHaveBeenCalledTimes(1))
    expect(api.mergeLogDuplicates).toHaveBeenCalledWith([{
      keep: 'b',
      copies: [{ id: 'a', revision: 'ka' }, { id: 'b', revision: 'kb' }],
      choices: { GRIDSQUARE: 'a', COMMENT: 'b' },
    }])
    await waitFor(() => expect(onChanged).toHaveBeenCalled())
    const toast = last(toasts)
    expect(toast.message).toBe('Merged 1 contact')
    await toast.action!()
    expect(api.undoHistoryRun).toHaveBeenCalledWith('run-1')
  })

  it('keeping the other copy makes its value the default of an unguarded conflict', async () => {
    api.findLogDuplicates.mockResolvedValue({ scanned: 10, groups: [group] })
    api.mergeLogDuplicates.mockResolvedValue(made)
    open('dupes')
    fireEvent.click(last(screen.getAllByRole('button', { name: 'Find' })))
    await screen.findByRole('button', { name: 'Merge 2 into one' })
    const keeps = screen.getAllByRole('radio').filter((r) => (r as HTMLInputElement).name === 'keep-0')
    fireEvent.click(keeps[0])
    fireEvent.click(screen.getByRole('radio', { name: 'FN42' }))
    fireEvent.click(screen.getByRole('button', { name: 'Merge 2 into one' }))
    await waitFor(() => expect(api.mergeLogDuplicates).toHaveBeenCalledTimes(1))
    expect(api.mergeLogDuplicates.mock.calls[0][0][0]).toMatchObject({
      keep: 'a',
      choices: { GRIDSQUARE: 'b', COMMENT: 'a' },
    })
  })
})

describe('bulk edit', () => {
  it('previews, names the count, asks once more and sends exactly the fields and contacts', async () => {
    api.bulkEditPreview.mockResolvedValue({ ids: ['x', 'y'], selected: 2, change: 2, unchanged: 0, missing: 0, recall: 0, resend: 0, unconfirm: 0 })
    api.bulkEditApply.mockResolvedValue(made)
    open('bulk', ['x', 'y'])
    fireEvent.change(screen.getByRole('textbox', { name: 'Value' }), { target: { value: ' EM10 ' } })
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    const apply = await screen.findByRole('button', { name: 'Change 2 contacts' })
    expect(api.bulkEditPreview).toHaveBeenCalledWith(
      { ids: ['x', 'y'] },
      [{ field: 'MY_GRIDSQUARE', value: 'EM10' }],
    )
    fireEvent.click(apply)
    expect(api.bulkEditApply).not.toHaveBeenCalled()
    expect(screen.queryByText(/Undo puts the station call/)).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Yes, change 2' }))
    await waitFor(() => expect(api.bulkEditApply).toHaveBeenCalledWith(
      ['x', 'y'],
      2,
      0,
      null,
      [{ field: 'MY_GRIDSQUARE', value: 'EM10' }],
    ))
  })

  it('a park re-tag names the contacts already uploaded and the exported activation files it puts out of date', async () => {
    api.bulkEditPreview.mockResolvedValue({
      ids: ['x', 'y', 'z'], selected: 3, change: 2, unchanged: 1, missing: 0, recall: 0, resend: 0, unconfirm: 0,
      moves: 0, moveTo: null, moveFrom: [],
      activationChanges: 2, activationUploaded: 1, activationExported: 2,
      activationFiles: ['US-0001 2026-01-01 KD9TAW', 'US-0004 2026-01-02 KD9TAW'],
    })
    open('bulk', ['x', 'y', 'z'])
    fireEvent.change(screen.getByRole('combobox', { name: 'Field' }), { target: { value: 'MY_POTA_REF' } })
    fireEvent.change(screen.getByRole('textbox', { name: 'Value' }), { target: { value: 'US-0002,US-0003' } })
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    await screen.findByRole('button', { name: 'Change 2 contacts' })
    expect(api.bulkEditPreview).toHaveBeenCalledWith(
      { ids: ['x', 'y', 'z'] },
      [{ field: 'MY_POTA_REF', value: 'US-0002,US-0003' }],
    )
    expect(screen.getByText(/1 of these was already sent to LoTW, eQSL, QRZ or Club Log/)).toBeTruthy()
    expect(
      screen.getByText(/2 of these are in activation files you already exported \(US-0001 2026-01-01 KD9TAW, US-0004 2026-01-02 KD9TAW\)/),
    ).toBeTruthy()
  })

  it('a preview that moves no exported or uploaded contact says nothing of activation files', async () => {
    api.bulkEditPreview.mockResolvedValue({
      ids: ['x'], selected: 1, change: 1, unchanged: 0, missing: 0, recall: 0, resend: 0, unconfirm: 0,
      moves: 0, moveTo: null, moveFrom: [],
      activationChanges: 1, activationUploaded: 0, activationExported: 0, activationFiles: [],
    })
    open('bulk', ['x'])
    fireEvent.change(screen.getByRole('textbox', { name: 'Value' }), { target: { value: 'EM10' } })
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    await screen.findByRole('button', { name: 'Change 1 contact' })
    expect(screen.queryByText(/activation file/)).toBeNull()
    expect(screen.queryByText(/already sent to LoTW/)).toBeNull()
  })

  it('applies to the ids the preview answered, with its count, not to the filter as it now stands', async () => {
    const ids = Array.from({ length: 42 }, (_, i) => `q${i}`)
    api.bulkEditPreview.mockResolvedValue({ ids, selected: 42, change: 40, unchanged: 2, missing: 0, recall: 4, resend: 3, unconfirm: 5 })
    api.bulkEditApply.mockResolvedValue(made)
    open('bulk')
    fireEvent.change(screen.getByRole('combobox', { name: 'Field' }), { target: { value: 'STATION_CALLSIGN' } })
    fireEvent.change(screen.getByRole('textbox', { name: 'Value' }), { target: { value: 'W6R' } })
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Change 40 contacts' }))
    expect(screen.getByText(/3 of these were uploaded under another station call/)).toBeTruthy()
    // What an undo of it cannot take back.
    expect(screen.getByText(
      'Undo puts the station call and its confirmations back. Anything uploaded under the new call stays on that service, and these contacts are sent again under the call they get back.',
    )).toBeTruthy()
    expect(screen.getByText(
      '5 LoTW, eQSL or QRZ confirmations were matched under the old station call and will be cleared. Paper QSL cards stay.',
    )).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Yes, change 40' }))
    await waitFor(() => expect(api.bulkEditApply).toHaveBeenCalledWith(
      ids,
      42,
      0,
      null,
      [{ field: 'STATION_CALLSIGN', value: 'W6R' }],
    ))
  })

  it('names the contacts a fill or clear sends under another call and waits for the tick', async () => {
    api.bulkEditPreview.mockResolvedValue({
      ids: ['x', 'y', 'z'], selected: 3, change: 3, unchanged: 0, missing: 0, recall: 0, resend: 0, unconfirm: 0,
      moves: 2, moveTo: 'KD9NEW', moveFrom: ['KA1OLD', 'W6R'],
    })
    api.bulkEditApply.mockResolvedValue(made)
    open('bulk')
    fireEvent.change(screen.getByRole('combobox', { name: 'Field' }), { target: { value: 'STATION_CALLSIGN' } })
    fireEvent.change(screen.getByRole('combobox', { name: 'Set or clear' }), { target: { value: 'clear' } })
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Change 3 contacts' }))
    expect(screen.getByText(
      '2 contacts would be sent under KD9NEW instead of KA1OLD, W6R to any service they have not been sent to yet. Uploads already made stay as they are, and LoTW cannot take back a QSO it has accepted.',
    )).toBeTruthy()
    const yes = screen.getByRole('button', { name: 'Yes, change 3' }) as HTMLButtonElement
    expect(yes.disabled).toBe(true)
    fireEvent.click(yes)
    expect(api.bulkEditApply).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('checkbox', { name: 'I understand: these contacts will be sent under KD9NEW' }))
    expect(yes.disabled).toBe(false)
    fireEvent.click(yes)
    await waitFor(() => expect(api.bulkEditApply).toHaveBeenCalledWith(
      ['x', 'y', 'z'],
      3,
      2,
      'KD9NEW',
      [{ field: 'STATION_CALLSIGN', value: null }],
    ))
  })

  it('takes the tick back on a new preview, an edit of the value and a reopened confirm', async () => {
    const preview = {
      ids: ['x', 'y'], selected: 2, change: 2, unchanged: 0, missing: 0, recall: 0, resend: 0, unconfirm: 0,
      moves: 2, moveTo: 'KD9NEW', moveFrom: ['KA1OLD'],
    }
    api.bulkEditPreview.mockResolvedValue(preview)
    open('bulk')
    fireEvent.change(screen.getByRole('combobox', { name: 'Field' }), { target: { value: 'STATION_CALLSIGN' } })
    fireEvent.change(screen.getByRole('textbox', { name: 'Value' }), { target: { value: 'KA1OLD' } })
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    const tick = (to: string) => screen.getByRole('checkbox', { name: `I understand: these contacts will be sent under ${to}` }) as HTMLInputElement
    const yes = () => screen.getByRole('button', { name: 'Yes, change 2' }) as HTMLButtonElement
    const ticked = (to: string) => {
      fireEvent.click(tick(to))
      expect(tick(to).checked).toBe(true)
      expect(yes().disabled).toBe(false)
    }

    // The confirm reopened.
    fireEvent.click(await screen.findByRole('button', { name: 'Change 2 contacts' }))
    ticked('KD9NEW')
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    fireEvent.click(screen.getByRole('button', { name: 'Change 2 contacts' }))
    expect(tick('KD9NEW').checked).toBe(false)
    expect(yes().disabled).toBe(true)

    // A new preview, with the confirm still open, naming another call.
    ticked('KD9NEW')
    api.bulkEditPreview.mockResolvedValue({ ...preview, moveTo: 'KD9NEW/P' })
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    await waitFor(() => expect(tick('KD9NEW/P').checked).toBe(false))
    expect(yes().disabled).toBe(true)

    // An edit of the value.
    ticked('KD9NEW/P')
    fireEvent.change(screen.getByRole('textbox', { name: 'Value' }), { target: { value: 'KA1OL' } })
    expect(screen.queryByRole('checkbox')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Change 2 contacts' }))
    expect(tick('KD9NEW/P').checked).toBe(false)
    expect(yes().disabled).toBe(true)
    expect(api.bulkEditApply).not.toHaveBeenCalled()
  })

  it('a change after the preview takes the apply away until it is previewed again', async () => {
    api.bulkEditPreview.mockResolvedValue({ ids: [], selected: 42, change: 40, unchanged: 2, missing: 0, recall: 0, resend: 0, unconfirm: 0 })
    open('bulk')
    fireEvent.change(screen.getByRole('combobox', { name: 'Set or clear' }), { target: { value: 'clear' } })
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    await screen.findByRole('button', { name: 'Change 40 contacts' })
    expect(screen.queryByText(/will be cleared/)).toBeNull()
    expect(api.bulkEditPreview.mock.calls[0]).toEqual([
      { query: DEFAULT_LOG_QUERY },
      [{ field: 'MY_GRIDSQUARE', value: null }],
    ])
    fireEvent.click(screen.getByRole('button', { name: 'Add a field' }))
    expect(screen.queryByRole('button', { name: 'Change 40 contacts' })).toBeNull()
  })

  it('a change of the contacts chosen or of the filter after the preview takes the apply away', async () => {
    api.bulkEditPreview.mockResolvedValue({ ids: ['x'], selected: 1, change: 1, unchanged: 0, missing: 0, recall: 0, resend: 0, unconfirm: 0 })
    const props = { onChanged: () => {}, afar: false, query: DEFAULT_LOG_QUERY, shown: 42 }
    const { rerender } = render(<LogBulkEdit {...props} picked={new Set(['x'])} />)
    fireEvent.change(screen.getByRole('textbox', { name: 'Value' }), { target: { value: 'EM10' } })
    const preview = async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
      await screen.findByRole('button', { name: 'Change 1 contact' })
    }
    await preview()
    rerender(<LogBulkEdit {...props} picked={new Set(['x', 'y'])} />)
    expect(screen.queryByRole('button', { name: 'Change 1 contact' })).toBeNull()
    await preview()
    rerender(<LogBulkEdit {...props} picked={new Set(['x', 'y'])} query={{ ...DEFAULT_LOG_QUERY, search: 'W1AW' }} />)
    expect(screen.queryByRole('button', { name: 'Change 1 contact' })).toBeNull()
  })

  it('driving another station, the change cannot be pressed', async () => {
    setRemoteStation(true)
    api.bulkEditPreview.mockResolvedValue({ ids: [], selected: 42, change: 40, unchanged: 2, missing: 0, recall: 0, resend: 0, unconfirm: 0 })
    open('bulk')
    fireEvent.change(screen.getByRole('textbox', { name: 'Value' }), { target: { value: 'EM10' } })
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    const apply = await screen.findByRole('button', { name: 'Change 40 contacts' })
    expect((apply as HTMLButtonElement).disabled).toBe(true)
  })
})

describe('the check', () => {
  const problems: LogProblem[] = [
    { id: 'a', call: 'W1AW', whenUnix: 1_767_268_800, kind: 'grid', field: 'GRIDSQUARE', current: 'FN31PRX', fix: { value: 'FN31PR' } },
    { id: 'b', call: 'K1ABC', whenUnix: 1_767_268_800, kind: 'grid', field: 'GRIDSQUARE', current: 'ZZ', fix: { value: null } },
    { id: 'c', call: 'W3AA', whenUnix: 4_000_000_000, kind: 'future', field: null, current: null, fix: null },
  ]

  it('two problems of one kind on one contact are ticked apart', async () => {
    const both: LogProblem[] = [
      { id: 'p', call: 'W1AW', whenUnix: 1_767_268_800, kind: 'pota', field: 'MY_POTA_REF', current: 'us1', fix: { value: 'US-0001' } },
      { id: 'p', call: 'W1AW', whenUnix: 1_767_268_800, kind: 'pota', field: 'POTA_REF', current: 'us2', fix: { value: 'US-0002' } },
    ]
    api.checkLogbook.mockResolvedValue({ checked: 1, problems: both })
    api.applyLogFixes.mockResolvedValue({ ...made, changed: 1 })
    open('check')
    fireEvent.click(screen.getByRole('button', { name: 'Check the log' }))
    await screen.findByText('2 problems in 1 contacts')
    const boxes = (screen.getAllByRole('checkbox') as HTMLInputElement[]).filter((b) => !b.name.startsWith('all'))
    fireEvent.click(last(boxes))
    fireEvent.click(screen.getByRole('button', { name: 'Fix 1 ticked' }))
    fireEvent.click(screen.getByRole('button', { name: 'Yes, fix 1' }))
    await waitFor(() => expect(api.applyLogFixes).toHaveBeenCalledWith([
      { id: 'p', field: 'POTA_REF', expect: 'us2', value: 'US-0002' },
    ]))
  })

  it('sends only the fixes ticked, each with what it found, after asking', async () => {
    api.checkLogbook.mockResolvedValue({ checked: 3, problems })
    api.applyLogFixes.mockResolvedValue({ ...made, changed: 1 })
    open('check')
    fireEvent.click(screen.getByRole('button', { name: 'Check the log' }))
    await screen.findByText('3 problems in 3 contacts')
    const boxes = screen.getAllByRole('checkbox') as HTMLInputElement[]
    // Tick all of the grids, then untick K1ABC; the future date has no fix to tick.
    expect(last(boxes).disabled).toBe(true)
    fireEvent.click(screen.getByRole('checkbox', { name: 'Tick all 2 with a fix' }))
    fireEvent.click(boxes[2])
    fireEvent.click(screen.getByRole('button', { name: 'Fix 1 ticked' }))
    expect(api.applyLogFixes).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'Yes, fix 1' }))
    await waitFor(() => expect(api.applyLogFixes).toHaveBeenCalledWith([
      { id: 'a', field: 'GRIDSQUARE', expect: 'FN31PRX', value: 'FN31PR' },
    ]))
  })
})
