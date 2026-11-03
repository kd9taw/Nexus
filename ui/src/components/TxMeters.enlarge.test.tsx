// @vitest-environment jsdom
//
// #446: THE FT8 TX METERS, LARGER. On the FT screen the meters live in the TX strip's small
// fixed-width telemetry cell (`.cq-telemetry`). The cell now opens a dialog with the same live
// meters at full size: a click, or Enter/Space on it. Escape closes it and the keyboard goes back to
// the cell. It is a READOUT: no control of any kind in the dialog, nothing that keys or stops.
// The dialog's geometry is computed from the real sheets' cascade on the rendered dialog.
import { describe, it, expect, afterEach, beforeAll } from 'vitest'
import { render, cleanup, fireEvent, screen, act, waitFor } from '@testing-library/react'
import { TxMetersCell } from './TxMeters'
import type { RadioStatus } from '../types'
import { loadSheets, pxOf } from '../cssCascade.testkit'

beforeAll(() => loadSheets())
afterEach(cleanup)

const keyed = (over: Record<string, unknown> = {}) =>
  ({ rigKeyed: true, txSwr: 1.3, txAlc: 0.4, txPoW: 87, ratedWatts: 100, swrScaleVerified: true, ...over }) as unknown as RadioStatus

/** The cell as Operate mounts it. */
function mount(radio: RadioStatus) {
  const r = render(
    <div className="cq-telemetry">
      <TxMetersCell radio={radio} />
    </div>,
  )
  const opener = screen.getByRole('button', { name: /transmit meters larger/i })
  return { ...r, opener }
}
const dialog = () => screen.queryByRole('dialog')
const values = (root: Element) => [...root.querySelectorAll('.ph-txmeter')].map((r) => r.querySelector('.ph-txmeter-value')?.textContent)

describe('the Operate TX meters open larger (#446)', () => {
  it('the cell still shows the inline meters, and is a keyboard-reachable opener', () => {
    const { container, opener } = mount(keyed())
    expect(container.querySelector('.cq-telemetry .ph-txmeters.inline'), 'the cell lost its inline meters').not.toBeNull()
    expect(opener.contains(container.querySelector('.ph-txmeters.inline'))).toBe(true)
    expect(opener.tabIndex).toBe(0)
    expect(opener.getAttribute('aria-haspopup')).toBe('dialog')
    expect(dialog()).toBeNull()
  })

  it('a click opens the same live readings at full size', () => {
    const { opener, rerender } = mount(keyed())
    fireEvent.click(opener)
    const d = dialog()!
    expect(d).not.toBeNull()
    const big = d.querySelector('.ph-txmeters')!
    expect(big.classList.contains('inline'), 'the dialog drew the cell variant').toBe(false)
    expect(values(big)).toEqual(['1.3:1', '40%', '87 W', '—'])
    // Live: a new reading reaches the open dialog.
    rerender(
      <div className="cq-telemetry">
        <TxMetersCell radio={keyed({ txPoW: 52 })} />
      </div>,
    )
    expect(values(dialog()!.querySelector('.ph-txmeters')!)[2]).toBe('52 W')
  })

  it('Enter and Space open it; Escape closes it and gives the keyboard back to the cell', async () => {
    const { opener } = mount(keyed())
    for (const key of ['Enter', ' ']) {
      opener.focus()
      fireEvent.keyDown(opener, { key })
      expect(dialog(), `${JSON.stringify(key)} did not open it`).not.toBeNull()
      act(() => {
        fireEvent.keyDown(dialog()!, { key: 'Escape' })
      })
      expect(dialog()).toBeNull()
      await waitFor(() => expect(document.activeElement, 'focus was not returned to the cell').toBe(opener))
    }
    // CONTROL: another key opens nothing.
    fireEvent.keyDown(opener, { key: 'a' })
    expect(dialog()).toBeNull()
  })

  it('is display only: no button, slider or other control in the dialog', () => {
    const { opener } = mount(keyed({ compLevel: 0.5 }))
    fireEvent.click(opener)
    const d = dialog()!
    expect(d.querySelectorAll('button, input, select, textarea, [role="slider"], [role="switch"], [tabindex="0"]').length).toBe(0)
  })

  it('draws the bars full size: taller than the cell’s, in the dialog box only', () => {
    const { container, opener } = mount(keyed())
    const cellTrack = container.querySelector('.ph-txmeters.inline .ph-txmeter-track')!
    fireEvent.click(opener)
    const bigTrack = dialog()!.querySelector('.ph-txmeter-track')!
    const cellH = pxOf(cellTrack, 'height')
    const bigH = pxOf(bigTrack, 'height')
    expect(cellH).toBeGreaterThan(0)
    expect(bigH, `dialog bar ${bigH}px vs cell ${cellH}px`).toBeGreaterThanOrEqual(cellH * 2)
    // The cell itself is untouched by the dialog's rules.
    expect(pxOf(cellTrack, 'height')).toBe(cellH)
  })
})
