// @vitest-environment jsdom
// AWARD STATUS: the Awards view's "Ready to submit" panel lists what the station says is ready,
// exports it, and marks it Submitted ONLY once the operator confirms the count the station
// previewed; the contact card's per-award choice goes through the same confirm; Bulk edit offers
// each award's status as a field of its own.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup, waitFor, fireEvent } from '@testing-library/react'
import { AwardStatusPanel, CardAwardStatus } from './AwardStatus'
import { LogBulkEdit } from './LogBulkEdit'
import type { AwardStatus, BulkPreview, ToolRun } from '../api'
import { bulkEditApply, bulkEditPreview, getAwardStatus, saveTextToDownloads } from '../api'
import { confirmDialog } from '../confirm'
import type { LoggedQso } from '../types'
import { EN } from '../i18n'
import { readyCsv, readyHtml, readyIds, statusOf } from '../features/awardStatus'

vi.mock('../api', () => ({
  getAwardStatus: vi.fn(),
  bulkEditPreview: vi.fn(),
  bulkEditApply: vi.fn(),
  saveTextToDownloads: vi.fn(),
  undoHistoryRun: vi.fn(),
}))
vi.mock('../confirm', () => ({ confirmDialog: vi.fn() }))

const READY: AwardStatus = {
  award: 'dxcc',
  creditCode: 'DXCC',
  worked: 5,
  confirmed: 3,
  submitted: 1,
  granted: 1,
  ready: [
    { entity: 'Fed. Rep. of Germany', id: 'id-dl', call: 'DL2ABC', whenUnix: 1_700_000_000, band: '20m', mode: 'CW', confirmedBy: ['card'] },
    { entity: 'Japan', id: 'id-ja', call: 'JA1ABC', whenUnix: 1_700_086_400, band: '15m', mode: 'FT8', confirmedBy: ['lotw'] },
  ],
}

const PREVIEW: BulkPreview = {
  ids: ['id-dl', 'id-ja'],
  selected: 2,
  change: 2,
  unchanged: 0,
  missing: 0,
  recall: 0,
  resend: 0,
  unconfirm: 0,
  moves: 0,
  moveTo: null,
  moveFrom: [],
  activationChanges: 0,
  activationUploaded: 0,
  activationExported: 0,
  activationFiles: [],
} as BulkPreview

const RUN = { changed: 2, gone: 0, changedSince: 0, needsChoice: 0, distinct: 0, run: null } as unknown as ToolRun

beforeEach(() => {
  vi.mocked(getAwardStatus).mockResolvedValue(READY)
  vi.mocked(bulkEditPreview).mockResolvedValue(PREVIEW)
  vi.mocked(bulkEditApply).mockResolvedValue(RUN)
  vi.mocked(saveTextToDownloads).mockResolvedValue('/home/op/Downloads/x')
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('the Ready to submit panel', () => {
  it('lists the station’s ready rows and its counts', async () => {
    render(<AwardStatusPanel />)
    expect(await screen.findByText(/DL2ABC/)).toBeTruthy()
    expect(screen.getByText(/JA1ABC/)).toBeTruthy()
    expect(screen.getByTestId('award-status-counts').textContent).toBe(
      '5 worked · 3 confirmed · 1 submitted · 1 granted',
    )
    expect(getAwardStatus).toHaveBeenCalledWith('dxcc', false)
    fireEvent.change(screen.getByLabelText(EN['awards.status.pick'] as string), { target: { value: 'newest' } })
    await waitFor(() => expect(getAwardStatus).toHaveBeenLastCalledWith('dxcc', true))
    fireEvent.change(screen.getByLabelText(EN['awards.status.award'] as string), { target: { value: 'usaca' } })
    await waitFor(() => expect(getAwardStatus).toHaveBeenLastCalledWith('usaca', true))
  })

  it('changes nothing when the operator does not confirm', async () => {
    vi.mocked(confirmDialog).mockResolvedValue(false)
    render(<AwardStatusPanel />)
    fireEvent.click(await screen.findByRole('button', { name: 'Mark 2 submitted…' }))
    await waitFor(() => expect(confirmDialog).toHaveBeenCalledTimes(1))
    expect(bulkEditPreview).toHaveBeenCalledWith({ ids: ['id-dl', 'id-ja'] }, [{ field: 'CREDIT_DXCC', value: 'SUBMITTED' }])
    expect(bulkEditApply).not.toHaveBeenCalled()
  })

  it('marks exactly the previewed contacts Submitted once the operator confirms', async () => {
    vi.mocked(confirmDialog).mockResolvedValue(true)
    render(<AwardStatusPanel />)
    fireEvent.click(await screen.findByRole('button', { name: 'Mark 2 submitted…' }))
    await waitFor(() => expect(bulkEditApply).toHaveBeenCalledTimes(1))
    expect(vi.mocked(confirmDialog).mock.calls[0][0].title).toBe('Mark 2 contacts submitted for DXCC?')
    expect(bulkEditApply).toHaveBeenCalledWith(['id-dl', 'id-ja'], 2, 0, null, [{ field: 'CREDIT_DXCC', value: 'SUBMITTED' }])
    // The list is read again after the change.
    await waitFor(() => expect(getAwardStatus).toHaveBeenCalledTimes(2))
  })

  it('exports the list as CSV and as a page to print', async () => {
    render(<AwardStatusPanel />)
    fireEvent.click(await screen.findByRole('button', { name: EN['awards.status.csv'] as string }))
    await waitFor(() => expect(saveTextToDownloads).toHaveBeenCalledTimes(1))
    const [name, csv] = vi.mocked(saveTextToDownloads).mock.calls[0]
    expect(name).toMatch(/^nexus-dxcc-ready-\d{8}\.csv$/)
    expect(csv).toBe(readyCsv(READY.ready))
    fireEvent.click(screen.getByRole('button', { name: EN['awards.status.printButton'] as string }))
    await waitFor(() => expect(saveTextToDownloads).toHaveBeenCalledTimes(2))
    const [page, html] = vi.mocked(saveTextToDownloads).mock.calls[1]
    expect(page).toMatch(/\.html$/)
    expect(html).toContain('<td>JA1ABC</td>')
    expect(html).toContain('<td>Card</td>')
  })

  it('offers no marking on a read-only view', async () => {
    render(<AwardStatusPanel readOnly />)
    await screen.findByText(/DL2ABC/)
    expect(screen.queryByRole('button', { name: 'Mark 2 submitted…' })).toBeNull()
  })
})

describe('the contact card', () => {
  const q = { id: 'id-ja', call: 'JA1ABC', creditGranted: ['DXCC_BAND', 'WAS'], creditSubmitted: ['IOTA'] } as LoggedQso

  it('shows each award’s status by its exact code', () => {
    render(<CardAwardStatus q={q} canSave onChanged={() => {}} />)
    expect((screen.getByLabelText('DXCC') as HTMLSelectElement).value).toBe('')
    expect((screen.getByLabelText('WAS') as HTMLSelectElement).value).toBe('GRANTED')
    expect((screen.getByLabelText('IOTA') as HTMLSelectElement).value).toBe('SUBMITTED')
  })

  it('sets one award’s status only once the operator confirms', async () => {
    vi.mocked(bulkEditPreview).mockResolvedValue({ ...PREVIEW, ids: ['id-ja'], selected: 1, change: 1 })
    vi.mocked(confirmDialog).mockResolvedValueOnce(false).mockResolvedValueOnce(true)
    const changed = vi.fn()
    render(<CardAwardStatus q={q} canSave onChanged={changed} />)
    fireEvent.change(screen.getByLabelText('USA-CA'), { target: { value: 'GRANTED' } })
    await waitFor(() => expect(confirmDialog).toHaveBeenCalledTimes(1))
    expect(bulkEditApply).not.toHaveBeenCalled()
    fireEvent.change(screen.getByLabelText('USA-CA'), { target: { value: 'GRANTED' } })
    await waitFor(() => expect(bulkEditApply).toHaveBeenCalledTimes(1))
    expect(bulkEditApply).toHaveBeenCalledWith(['id-ja'], 1, 0, null, [{ field: 'CREDIT_USACA', value: 'GRANTED' }])
    expect(changed).toHaveBeenCalled()
  })

  it('is read only where the card cannot save', () => {
    render(<CardAwardStatus q={q} canSave={false} onChanged={() => {}} />)
    expect((screen.getByLabelText('DXCC') as HTMLSelectElement).disabled).toBe(true)
  })
})

describe('Bulk edit', () => {
  it('offers each award’s status as Submitted or Granted', async () => {
    render(<LogBulkEdit onChanged={() => {}} afar={false} picked={new Set(['id-ja'])} query={{} as never} shown={1} />)
    const field = screen.getByLabelText(EN['logbook.tools.bulk.field'] as string)
    fireEvent.change(field, { target: { value: 'CREDIT_USACA' } })
    expect(screen.getByRole('option', { name: 'USA-CA award status' })).toBeTruthy()
    const value = screen.getByLabelText(EN['logbook.tools.bulk.value'] as string) as HTMLSelectElement
    expect(value.tagName).toBe('SELECT')
    expect(value.value).toBe('SUBMITTED')
    fireEvent.change(value, { target: { value: 'GRANTED' } })
    fireEvent.click(screen.getByRole('button', { name: EN['logbook.tools.bulk.preview'] as string }))
    await waitFor(() => expect(bulkEditPreview).toHaveBeenCalledTimes(1))
    expect(bulkEditPreview).toHaveBeenCalledWith({ ids: ['id-ja'] }, [{ field: 'CREDIT_USACA', value: 'GRANTED' }])
  })
})

describe('the list’s files', () => {
  it('quotes a CSV cell that needs it and escapes the page', () => {
    const rows = [{ entity: 'MD,Prince George\'s', id: 'a', call: 'W3<X>', whenUnix: 0, band: '20m', mode: 'CW', confirmedBy: ['card', 'eqsl'] }]
    expect(readyCsv(rows)).toBe(
      'ENTITY,CALL,QSO_DATE,TIME_ON,BAND,MODE,CONFIRMED_BY\r\n"MD,Prince George\'s",W3<X>,19700101,0000,20m,CW,card+eqsl\r\n',
    )
    const html = readyHtml(rows, {
      title: 'T', summary: 'S', entity: 'E', call: 'C', date: 'D', band: 'B', mode: 'M', confirmedBy: 'By',
      channel: (c) => c.toUpperCase(),
    })
    expect(html).toContain('<td>W3&lt;X&gt;</td>')
    expect(html).toContain('<td>CARD + EQSL</td>')
    expect(html).not.toContain('<script')
  })

  it('names each contact once and reads status by the exact code', () => {
    expect(readyIds({ ready: [...READY.ready, READY.ready[0]] })).toEqual(['id-dl', 'id-ja'])
    expect(statusOf({ creditGranted: ['DXCC_BAND'], creditSubmitted: ['dxcc'] }, 'DXCC')).toBe('SUBMITTED')
    expect(statusOf({ creditGranted: ['DXCC'], creditSubmitted: ['DXCC'] }, 'DXCC')).toBe('GRANTED')
    expect(statusOf({}, 'USACA')).toBeNull()
  })
})
