// @vitest-environment jsdom
//
// The period a band or mode change caught while it was being decoded (the snapshot's
// `lateDecodes`). WSJT-X shows it, coloured for the band it was received on; so does the pane,
// after the wipe the change made, under a separator naming the band, mode and dial it was heard
// on, never the new ones. Nothing in it can be worked or selected: answering a line heard on the
// band or mode just left would start a QSO where that station was never heard.
import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, cleanup, fireEvent, screen } from '@testing-library/react'
import { OperateDecodes } from './OperateDecodes'
import type { DecodeRow, LateDecodes, Tier } from '../types'

afterEach(cleanup)

function row(message: string, freqHz: number): DecodeRow {
  return {
    from: message.split(' ')[1],
    snr: -10,
    dtSec: 0.1,
    freqHz,
    message,
    isCq: true,
    directedToMe: false,
    worked: false,
    tier: 'FT8',
    rv: -1,
  }
}

/** FT8 on 20 m at 14.074, the period that started at 00:02:00 (boundary slot 9). */
const late: LateDecodes = {
  band: '20m',
  dialMhz: 14.074,
  tier: 'FT8',
  periodStartMs: 8 * 15_000,
  slot: 9,
  rows: [row('CQ W9XYZ EN52', 1200)],
}

const live = row('CQ JA1XYZ PM95', 900)

function pane(decodes: DecodeRow[], lateDecodes: LateDecodes | null, tier: Tier = 'FT8') {
  const onCall = vi.fn()
  const onSelectDecode = vi.fn()
  const props = { rxOffsetHz: 1500, band: '40m', tier, harqRescues: 0, onCall, onSelectDecode }
  const view = render(<OperateDecodes decodes={decodes} late={lateDecodes} slot={11} {...props} />)
  const rerender = (next: DecodeRow[], nextLate: LateDecodes | null) =>
    view.rerender(<OperateDecodes decodes={next} late={nextLate} slot={11} {...props} />)
  const rowFor = (message: string) => {
    const el = [...view.container.querySelectorAll('.decode-row')].find((r) =>
      r.textContent?.includes(message),
    )
    expect(el, `no row for ${message}`).toBeDefined()
    return el as HTMLElement
  }
  return { onCall, onSelectDecode, rerender, rowFor }
}

describe('the period in flight at a band or mode change', () => {
  it('is shown under the band, mode and dial it was heard on', () => {
    for (const tier of ['FT8', 'FT4'] as const) {
      pane([], late, tier)
      const sep = screen.getByRole('separator', {
        name: 'Period 000200 UTC, heard on 20m FT8 before the change',
      })
      expect(sep.textContent).toBe('00020020m · FT8 · 14.074 MHz')
      expect(screen.getByText('CQ W9XYZ EN52')).toBeTruthy()
      cleanup()
    }
  })

  it('cannot be worked or selected, where a live row can', () => {
    const { onCall, onSelectDecode, rowFor } = pane([live], late)
    const heard = rowFor('CQ W9XYZ EN52')
    expect(heard.title).toBe(
      'Heard on 20m FT8 before the band or mode change. Shown only: it cannot be worked from here.',
    )
    fireEvent.click(heard)
    fireEvent.doubleClick(heard)
    fireEvent.doubleClick(heard, { ctrlKey: true })
    expect(onSelectDecode).not.toHaveBeenCalled()
    expect(onCall).not.toHaveBeenCalled()
    // The control: the same gestures on a row of the band now selected.
    fireEvent.click(rowFor('CQ JA1XYZ PM95'))
    fireEvent.doubleClick(rowFor('CQ JA1XYZ PM95'))
    expect(onSelectDecode).toHaveBeenCalledWith('JA1XYZ', 'PM95', 'CQ JA1XYZ PM95', -10)
    expect(onCall).toHaveBeenCalledWith('JA1XYZ', undefined, 'CQ JA1XYZ PM95', -10, 900)
  })

  it('stays on screen, and stays inert, once the next period replaces it in the feed', () => {
    const { onCall, rerender, rowFor } = pane([], late)
    rerender([live], null)
    fireEvent.doubleClick(rowFor('CQ W9XYZ EN52'))
    expect(onCall).not.toHaveBeenCalled()
    // The live period below it gets its own separator, under the band now selected.
    expect(screen.getByRole('separator', { name: 'Period 000230 UTC' }).textContent).toBe('00023040m')
  })
})
