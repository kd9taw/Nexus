// @vitest-environment jsdom
//
// The import preview: what the file holds and what the import would do, before anything is
// written; the three choices; and the report saved after.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/react'
import { ImportPreview } from './ImportPreview'
import { t } from '../i18n'
import * as api from '../api'
import type { ImportDone, ImportPreview as Preview } from '../features/logImport'

vi.mock('../api', () => ({
  previewImport: vi.fn(),
  importLog: vi.fn(),
  importLotwUnmatched: vi.fn(),
  saveTextToDownloads: vi.fn(async (name: string) => `/downloads/${name}`),
}))

const PREVIEW: Preview = {
  format: 'adif',
  template: null,
  records: 5,
  contacts: 4,
  firstUnix: 1_704_015_000,
  lastUnix: 1_709_251_200,
  new: 3,
  duplicates: 1,
  updates: 1,
  notes: [
    { kind: 'noCall', count: 1, examples: [{ kind: 'noCall', at: 4, text: 'NAME=nobody' }] },
    { kind: 'badValue', count: 1, examples: [{ kind: 'badValue', at: 2, text: 'FREQ=seven' }] },
  ],
  coverage: [
    { field: 'CALL', count: 4 },
    { field: 'FREQ', count: 1 },
  ],
  unmapped: [{ what: 'PROGRAMID', side: null, count: 1, reason: 'header' }],
}

const DONE: ImportDone = {
  stats: { added: 3, skipped: 1, updated: 0, total: 10, run: 'run-1' },
  report: {
    format: 'adif', template: null, records: 5, contacts: 4, added: 3, duplicates: 1, updated: 0, newOnly: true,
    fields: [
      { field: 'CALL', occurrences: 5, stored: 4, normalized: 0, passthrough: 0, dropped: 1, reasons: [{ reason: 'noContact', count: 1 }], examples: [] },
    ],
    unmapped: [], notes: [], accounted: true,
  },
}

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

function open(onImported = vi.fn(), onClose = vi.fn()) {
  vi.mocked(api.previewImport).mockResolvedValue(PREVIEW)
  vi.mocked(api.importLog).mockResolvedValue(DONE)
  render(<ImportPreview text="FILE" fileName="mylog.adi" onClose={onClose} onImported={onImported} />)
  return { onImported, onClose }
}

describe('the import preview', () => {
  it('shows the counts, the range, the problems with examples, and the fields — and writes nothing', async () => {
    open()
    expect(await screen.findByText(t('logbook.import.preview.new', { count: 3 }))).toBeTruthy()
    expect(api.previewImport).toHaveBeenCalledWith('FILE')
    expect(screen.getByText(t('logbook.import.preview.records', { count: 5 }))).toBeTruthy()
    expect(screen.getByText(t('logbook.import.preview.dupes', { count: 1 }))).toBeTruthy()
    expect(screen.getByText(t('logbook.import.preview.updates', { count: 1 }))).toBeTruthy()
    expect(screen.getByText(t('logbook.import.preview.range', { from: '2023-12-31', to: '2024-03-01' }))).toBeTruthy()
    expect(document.body.textContent).toContain(t('logbook.import.note.badValue'))
    expect(document.body.textContent).toContain(`${t('logbook.import.at.record', { n: 2 })}: FREQ=seven`)
    expect(document.body.textContent).toContain(t('logbook.import.unmapped.header'))
    expect(document.querySelector('.import-preview-coverage')?.textContent).toContain('FREQ')
    expect(api.importLog).not.toHaveBeenCalled()
  })

  it('lists the first 200 of what is not stored, and counts the rest', async () => {
    const unmapped = Array.from({ length: 250 }, (_, i) => ({ what: `X${i}`, side: null, count: 1, reason: 'header' as const }))
    vi.mocked(api.previewImport).mockResolvedValue({ ...PREVIEW, unmapped })
    render(<ImportPreview text="FILE" fileName="mylog.adi" onClose={vi.fn()} onImported={vi.fn()} />)
    expect(await screen.findByText(t('logbook.import.preview.moreUnmapped', { count: 50 }))).toBeTruthy()
    const list = document.querySelector('.import-preview-unmapped')!
    expect(list.querySelectorAll('li')).toHaveLength(201)
    expect(list.textContent).toContain('X199')
    expect(list.textContent).not.toContain('X200')
  })

  it('says why the engine refused a file naming too many fields', async () => {
    vi.mocked(api.previewImport).mockRejectedValue('tooManyNames')
    render(<ImportPreview text="FILE" fileName="mylog.adi" onClose={vi.fn()} onImported={vi.fn()} />)
    expect(await screen.findByText(t('logbook.import.tooManyNames'))).toBeTruthy()
  })

  it('imports new only on that choice, says the report, and saves it as text', async () => {
    const { onImported } = open()
    fireEvent.click(await screen.findByRole('button', { name: t('logbook.import.preview.importNew') }))
    await waitFor(() => expect(api.importLog).toHaveBeenCalledWith('FILE', true))
    await waitFor(() => expect(onImported).toHaveBeenCalledWith(DONE.stats))
    expect(await screen.findByText(t('logbook.import.done.accounted'))).toBeTruthy()
    expect(screen.getByText(t('logbook.import.done.dropped', { count: 1 }))).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: t('logbook.import.done.save') }))
    await waitFor(() => expect(api.saveTextToDownloads).toHaveBeenCalled())
    const [name, text] = vi.mocked(api.saveTextToDownloads).mock.calls[0]
    expect(name).toMatch(/^nexus-import-report-mylog-\d{4}-\d{2}-\d{2}\.txt$/)
    expect(text).toContain('CALL: 5 = 4')
    expect(text).toContain(t('logbook.import.reason.noContact'))
  })

  it('Import makes the ordinary import; Cancel makes none', async () => {
    const { onClose } = open()
    fireEvent.click(await screen.findByRole('button', { name: t('logbook.import.preview.cancel') }))
    expect(onClose).toHaveBeenCalled()
    expect(api.importLog).not.toHaveBeenCalled()
    cleanup()
    open()
    fireEvent.click(await screen.findByRole('button', { name: t('logbook.import.preview.import') }))
    await waitFor(() => expect(api.importLog).toHaveBeenCalledWith('FILE', false))
  })

  it('offers no "new only" when nothing in the file is in the log', async () => {
    vi.mocked(api.previewImport).mockResolvedValue({ ...PREVIEW, duplicates: 0, updates: 0 })
    render(<ImportPreview text="FILE" fileName="a.adi" onClose={vi.fn()} onImported={vi.fn()} />)
    await screen.findByRole('button', { name: t('logbook.import.preview.import') })
    expect(screen.queryByRole('button', { name: t('logbook.import.preview.importNew') })).toBeNull()
  })
})

// A LoTW report downloaded by hand: its import confirms the contacts it matches and adds none, and
// only the explicit Add, after it, adds the rows that matched no contact.
describe('the import preview of a LoTW report', () => {
  const LOTW: Preview = { ...PREVIEW, new: 0, duplicates: 1, updates: 1, lotwUnmatched: 2 }
  const LOTW_DONE: ImportDone = {
    ...DONE,
    stats: { added: 0, skipped: 1, updated: 1, total: 10, run: 'run-1', unmatched: 2 },
  }
  const ADDED = { added: 2, skipped: 0, updated: 0, total: 12, run: 'run-2', unmatched: 0 }
  const addLabel = t('logbook.import.addUnmatched', { count: 2 })

  function openLotw(onImported = vi.fn()) {
    vi.mocked(api.previewImport).mockResolvedValue(LOTW)
    vi.mocked(api.importLog).mockResolvedValue(LOTW_DONE)
    render(<ImportPreview text="LOTW" fileName="lotwreport.adi" onClose={vi.fn()} onImported={onImported} />)
    return onImported
  }

  it('says it confirms and adds none, counts the unmatched, and offers no "new only"', async () => {
    openLotw()
    expect(await screen.findByText(t('logbook.import.preview.lotw'))).toBeTruthy()
    expect(screen.getByText(t('logbook.import.preview.lotwUnmatched', { count: 2 }))).toBeTruthy()
    expect(screen.queryByRole('button', { name: t('logbook.import.preview.importNew') })).toBeNull()
  })

  it('after the import counts the unmatched rows and adds them only when Add is pressed', async () => {
    vi.mocked(api.importLotwUnmatched).mockResolvedValue(ADDED)
    const onImported = openLotw()
    fireEvent.click(await screen.findByRole('button', { name: t('logbook.import.preview.import') }))
    await waitFor(() => expect(onImported).toHaveBeenCalledWith(LOTW_DONE.stats))
    expect(document.body.textContent).toContain(t('logbook.pota.unmatched', { count: 2 }))
    const add = await screen.findByRole('button', { name: addLabel })
    expect(api.importLotwUnmatched).not.toHaveBeenCalled()
    fireEvent.click(add)
    await waitFor(() => expect(api.importLotwUnmatched).toHaveBeenCalledWith('LOTW'))
    // The add is a run of its own, handed to the host for its toast and Undo; it is not offered again.
    await waitFor(() => expect(onImported).toHaveBeenLastCalledWith(ADDED))
    await waitFor(() => expect(screen.queryByRole('button', { name: addLabel })).toBeNull())
  })

  it('keeps Add when it fails, so it can be pressed again', async () => {
    vi.mocked(api.importLotwUnmatched).mockRejectedValue('LogBusy')
    const onImported = openLotw()
    fireEvent.click(await screen.findByRole('button', { name: t('logbook.import.preview.import') }))
    fireEvent.click(await screen.findByRole('button', { name: addLabel }))
    await waitFor(() => expect(api.importLotwUnmatched).toHaveBeenCalled())
    expect(onImported).toHaveBeenCalledTimes(1)
    expect((await screen.findByRole('button', { name: addLabel })).hasAttribute('disabled')).toBe(false)
  })

  it('offers no Add after an import with nothing unmatched', async () => {
    vi.mocked(api.previewImport).mockResolvedValue({ ...LOTW, lotwUnmatched: 0 })
    vi.mocked(api.importLog).mockResolvedValue({ ...LOTW_DONE, stats: { ...LOTW_DONE.stats, unmatched: 0 } })
    render(<ImportPreview text="LOTW" fileName="lotwreport.adi" onClose={vi.fn()} onImported={vi.fn()} />)
    fireEvent.click(await screen.findByRole('button', { name: t('logbook.import.preview.import') }))
    expect(await screen.findByText(t('logbook.import.done.accounted'))).toBeTruthy()
    expect(screen.queryByRole('button', { name: /unmatched/ })).toBeNull()
  })
})
