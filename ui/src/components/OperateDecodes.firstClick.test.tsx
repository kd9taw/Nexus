// @vitest-environment jsdom
//
// Band Activity and Rx Frequency: a double-click works the row its FIRST click landed on.
//
// The operator's report (1.25.0) was on the Call Roster, where the grid moved a row between the two
// clicks of a double-click and the station that slid under the pointer was worked. The decode panes
// have the same gesture and the same exposure: a period's decodes land between the two clicks, the
// feed follows its newest line, and the rows move under a pointer that has not. A double-click is one
// gesture aimed at the row its first click landed on; that row is the one it acts on, and the second
// click does not re-select. If that row has left the feed in between, the double-click does nothing.
import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, cleanup, fireEvent } from '@testing-library/react'
import { OperateDecodes } from './OperateDecodes'
import type { DecodeRow } from '../types'

afterEach(cleanup)

function row(message: string, freqHz: number): DecodeRow {
  return { from: message.split(' ')[1], snr: -10, dtSec: 0.1, freqHz, message, isCq: true, directedToMe: false, worked: false, tier: 'FT8', rv: -1 }
}

function pane(decodes: DecodeRow[]) {
  const onCall = vi.fn(), onSelectDecode = vi.fn(), onToggleIgnore = vi.fn()
  const props = { rxOffsetHz: 1500, band: '20m', tier: 'FT8' as const, harqRescues: 0, onCall, onSelectDecode, onToggleIgnore }
  const view = render(<OperateDecodes decodes={decodes} slot={11} clearTick={0} {...props} />)
  const rerender = (next: DecodeRow[], clearTick = 0) =>
    view.rerender(<OperateDecodes decodes={next} slot={11} clearTick={clearTick} {...props} />)
  const rowFor = (message: string) => {
    const el = [...view.container.querySelectorAll('.decode-row')].find((r) => r.textContent?.includes(message))
    expect(el, `no row for ${message}`).toBeDefined()
    return el as HTMLElement
  }
  return { onCall, onSelectDecode, onToggleIgnore, rerender, rowFor }
}

const A = row('CQ KM7CAB DN17', 900)
const B = row('CQ KB2DJJ FN20', 1200)
const C = row('CQ K0PT EM28', 1500)

describe('a decode double-click works the row its first click landed on', () => {
  it('rows moved between the clicks: the first row is worked, not the one under the second click', () => {
    const { onCall, onSelectDecode, rerender, rowFor } = pane([A, B])
    fireEvent.click(rowFor('CQ KM7CAB'), { detail: 1 })
    rerender([A, B, C]) // a period's decodes land between the two clicks
    fireEvent.click(rowFor('CQ KB2DJJ'), { detail: 2 })
    fireEvent.doubleClick(rowFor('CQ KB2DJJ'), { detail: 2 })
    expect(onSelectDecode.mock.calls.map((c) => c[0])).toEqual(['KM7CAB'])
    expect(onCall).toHaveBeenCalledTimes(1)
    expect(onCall).toHaveBeenCalledWith('KM7CAB', undefined, 'CQ KM7CAB DN17', -10, 900)
  })

  it('Alt+double-click ignores the first row too', () => {
    const { onCall, onToggleIgnore, rowFor } = pane([A, B])
    fireEvent.click(rowFor('CQ KM7CAB'), { detail: 1 })
    fireEvent.click(rowFor('CQ KB2DJJ'), { detail: 2 })
    fireEvent.doubleClick(rowFor('CQ KB2DJJ'), { detail: 2, altKey: true })
    expect(onToggleIgnore.mock.calls.map((c) => c[0])).toEqual(['KM7CAB'])
    expect(onCall).not.toHaveBeenCalled()
  })

  it('the first row left the feed in between: nothing is worked', () => {
    const { onCall, rerender, rowFor } = pane([A, B])
    fireEvent.click(rowFor('CQ KM7CAB'), { detail: 1 })
    // The pane is cleared (a logger's Clear over UDP) and the next period brings only KB2DJJ.
    rerender([], 1)
    rerender([B], 1)
    fireEvent.click(rowFor('CQ KB2DJJ'), { detail: 2 })
    fireEvent.doubleClick(rowFor('CQ KB2DJJ'), { detail: 2 })
    expect(onCall).not.toHaveBeenCalled()
  })

  it('a still double-click works its own row, and the next gesture starts afresh', () => {
    const { onCall, rowFor } = pane([A, B])
    fireEvent.click(rowFor('CQ KM7CAB'), { detail: 1 })
    fireEvent.click(rowFor('CQ KM7CAB'), { detail: 2 })
    fireEvent.doubleClick(rowFor('CQ KM7CAB'), { detail: 2 })
    fireEvent.click(rowFor('CQ KB2DJJ'), { detail: 1 })
    fireEvent.click(rowFor('CQ KB2DJJ'), { detail: 2 })
    fireEvent.doubleClick(rowFor('CQ KB2DJJ'), { detail: 2 })
    expect(onCall.mock.calls.map((c) => c[0])).toEqual(['KM7CAB', 'KB2DJJ'])
  })
})
