// @vitest-environment jsdom
//
// WHAT A SNAPSHOT POLL COSTS THE DECODE PANES. The snapshot lands at ~3 Hz and re-offers the same
// decode list for a whole period, and both panes (Band Activity, Rx Frequency) re-render with it
// whether Operate is on screen or not. Each render used to filter and sort the whole history (up
// to 3,000 rows) and rebuild every drawn row, and the ingest effect then forced a SECOND render
// that did it all again — a steady CPU load on a busy band with nothing new to show.
//
// Pinned here, by counting rather than timing:
//   • a poll that changes nothing: one render (the host's), no filter/sort pass, no row rebuilt;
//   • a poll that brings something: the pass runs once and only the rows that changed rebuild;
//   • and the display is the same — the new row appears, a flag moving under a row redraws it,
//     and double-click still works the station through the LATEST handler.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { Profiler } from 'react'
import { render, fireEvent, cleanup } from '@testing-library/react'
import type { DecodeRow } from '../types'

const counts = { order: 0, rows: 0 }
vi.mock('../api', () => ({ openQrzPage: vi.fn() }))
// Count the filter/sort pass (orderEntries runs once per pass) and row builds (resolveDecodeNeeds
// runs once per row drawn), passing through to the real functions.
vi.mock('../decodeHistory', async (orig) => {
  const real = await orig<typeof import('../decodeHistory')>()
  return {
    ...real,
    orderEntries: (...a: Parameters<typeof real.orderEntries>) => {
      counts.order++
      return real.orderEntries(...a)
    },
  }
})
vi.mock('../features/decodeNeeds', async (orig) => {
  const real = await orig<typeof import('../features/decodeNeeds')>()
  return {
    ...real,
    resolveDecodeNeeds: (...a: Parameters<typeof real.resolveDecodeNeeds>) => {
      counts.rows++
      return real.resolveDecodeNeeds(...a)
    },
  }
})

import { OperateDecodes } from './OperateDecodes'
import { DecodeHistory } from '../decodeHistory'

const decode = (over: Partial<DecodeRow> = {}): DecodeRow => ({
  from: 'W1AW',
  snr: -12,
  dtSec: 0.2,
  freqHz: 1200,
  message: 'CQ W1AW FN31',
  isCq: true,
  directedToMe: false,
  worked: false,
  tier: 'FT8',
  rv: 0,
  ...over,
})

/** One period's decodes, as a FRESH array of FRESH rows — what every snapshot poll delivers. */
const period = (): DecodeRow[] => [
  decode({ from: 'W1AW', message: 'CQ W1AW FN31', freqHz: 1200 }),
  decode({ from: 'JA1XYZ', message: 'CQ DX JA1XYZ PM95', freqHz: 900, snr: -18 }),
  decode({ from: 'K1ABC', message: 'K1ABC DL1QQ R-07', freqHz: 1500, isCq: false }),
]

type Props = React.ComponentProps<typeof OperateDecodes>
let commits = 0
function pane(p: Partial<Props> = {}) {
  return (
    <Profiler id="pane" onRender={() => commits++}>
      <OperateDecodes
        decodes={period()}
        slot={100}
        rxOffsetHz={1200}
        band="20m"
        tier="FT8"
        harqRescues={0}
        onCall={() => {}}
        {...p}
      />
    </Profiler>
  )
}
const reset = () => {
  counts.order = 0
  counts.rows = 0
  commits = 0
}
const calls = () =>
  Array.from(document.querySelectorAll('.decode-row')).map(
    (r) => (r.getAttribute('aria-label') ?? '').split(',')[0],
  )

beforeEach(() => {
  localStorage.clear()
  reset()
})
afterEach(cleanup)

describe('a snapshot poll that changes nothing costs the pane nothing', () => {
  it('one render (the host’s own), no filter/sort pass, no row rebuilt', () => {
    const view = render(pane())
    expect(calls()).toEqual(['W1AW', 'JA1XYZ', 'K1ABC'])
    reset()
    view.rerender(pane()) // the next poll: same period, a fresh but equal list
    view.rerender(pane())
    expect(commits).toBe(2) // one per poll — no second, ingest-forced render
    expect(counts.order).toBe(0)
    expect(counts.rows).toBe(0)
    expect(calls()).toEqual(['W1AW', 'JA1XYZ', 'K1ABC'])
  })

  it('a poll that brings a decode: one pass, and only the new row is built', () => {
    const view = render(pane())
    reset()
    view.rerender(pane({ decodes: [...period(), decode({ from: 'G4XYZ', message: 'CQ G4XYZ IO91', freqHz: 2400 })] }))
    expect(counts.order).toBe(1)
    expect(counts.rows).toBe(1)
    expect(calls()).toEqual(['W1AW', 'JA1XYZ', 'K1ABC', 'G4XYZ'])
  })

  it('a flag moving under a row on screen redraws that row, and only it', () => {
    const view = render(pane())
    expect(document.querySelectorAll('.b4-chip')).toHaveLength(0)
    reset()
    const next = period()
    next[1] = { ...next[1], worked: true } // JA1XYZ just got worked
    view.rerender(pane({ decodes: next }))
    expect(counts.rows).toBe(1)
    expect(document.querySelectorAll('.b4-chip')).toHaveLength(1)
  })

  it('a filter change re-runs the pass (the memo is keyed on the inputs, not frozen)', () => {
    const view = render(pane())
    reset()
    fireEvent.click(view.getByRole('button', { name: 'CQ' }))
    expect(counts.order).toBeGreaterThan(0)
    expect(calls()).toEqual(['W1AW', 'JA1XYZ'])
  })

  it('double-click works the station through the handler of the LATEST render', () => {
    const first = vi.fn()
    const latest = vi.fn()
    const view = render(pane({ onCall: first }))
    view.rerender(pane({ onCall: latest }))
    fireEvent.doubleClick(document.querySelectorAll('.decode-row')[1])
    expect(first).not.toHaveBeenCalled()
    expect(latest).toHaveBeenCalledWith('JA1XYZ', undefined, 'CQ DX JA1XYZ PM95', -18, 900)
  })

  it('a history shared by two panes shows a new decode in both on the same poll', () => {
    const history = new DecodeHistory()
    const two = (decodes: DecodeRow[]) => (
      <>
        {pane({ decodes, history, title: 'A' })}
        {pane({ decodes, history, title: 'B', compact: true })}
      </>
    )
    const view = render(two(period()))
    view.rerender(two([...period(), decode({ from: 'G4XYZ', message: 'CQ G4XYZ IO91', freqHz: 2400 })]))
    const panes = Array.from(document.querySelectorAll('.operate-decodes'))
    for (const p of panes) expect(p.querySelectorAll('.decode-row')).toHaveLength(4)
  })
})
