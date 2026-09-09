// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { OperateDecodes } from '../components/OperateDecodes'
import { StationControlContext } from '../stationAccess'
import { RemoteCollectionsContext, RemoteHistoryContext, RemoteCollections } from './collections'
import type { HistoryRow, RemoteHistory } from './collections'
import type { ApplicationClient } from './application-client'

afterEach(() => { cleanup(); vi.restoreAllMocks() })
const entry = (call: string, sequence: number, firstSequence = sequence): HistoryRow => ({
  sequence, firstSequence, slot: 1, at: 1788940800000,
  row: { from: call, message: `CQ ${call} FN31`, tier: 'FT8', freqHz: 1500, snr: -10,
    dtSec: 0.1, isCq: true, directedToMe: false, worked: false, rv: -1 },
})
it('local erase survives metadata updates, a new session resets cursors, and no erase/work reaches the station', () => {
  const call = vi.fn(), erase = vi.fn()
  const source = {} as RemoteCollections
  const view = (generation: string, rows: HistoryRow[]) => {
    const history: RemoteHistory = { generation, rows, band: '20m', tier: 'FT8', dropped: 0 }
    return <StationControlContext.Provider value={false}><RemoteCollectionsContext.Provider value={source}>
      <RemoteHistoryContext.Provider value={history}><OperateDecodes decodes={[]} slot={1} rxOffsetHz={1500}
        band="20m" tier="FT8" harqRescues={0} onCall={call} onErase={erase} compact lockedFilter="all" />
      </RemoteHistoryContext.Provider></RemoteCollectionsContext.Provider></StationControlContext.Provider>
  }
  const { container, rerender } = render(view('first:1', [entry('W1AW', 1)]))
  expect(container.textContent).toContain('CQ W1AW FN31')
  fireEvent.doubleClick(container.querySelector('.decode-msg')!)
  expect(call).not.toHaveBeenCalled()
  fireEvent.click(screen.getByRole('button', { name: 'Erase' }))
  expect(erase).not.toHaveBeenCalled()
  rerender(view('first:1', [entry('W1AW', 2, 1)]))
  expect(container.textContent).not.toContain('CQ W1AW FN31')
  rerender(view('first:1', [entry('W1AW', 2, 1), entry('JA1NEW', 3)]))
  expect(container.textContent).toContain('CQ JA1NEW FN31')
  rerender(view('second:1', [entry('W2NEW', 1)]))
  expect(container.textContent).toContain('CQ W2NEW FN31')
  expect(container.textContent).not.toContain('CQ JA1NEW FN31')
})
it('QRZ navigation stays in the browser and never becomes a station URL/credential command', async () => {
  const invoke = vi.fn(), open = vi.spyOn(window, 'open').mockReturnValue(null)
  const source = new RemoteCollections({ invoke, getPhase: () => 'ready' } as unknown as ApplicationClient)
  await source.invoke('open_qrz_page', { call: 'PJ4/K1ABC' })
  expect(open).toHaveBeenCalledWith('https://www.qrz.com/db/K1ABC', '_blank', 'noopener,noreferrer')
  expect(invoke).not.toHaveBeenCalled()
})
