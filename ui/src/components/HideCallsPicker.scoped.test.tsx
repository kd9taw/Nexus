// @vitest-environment jsdom
//
// #346 — A CALL HIDDEN FROM A ROW'S MENU ALWAYS HAS A WAY BACK. The menu's narrower hides (one band,
// or for a while) are not entries of the chip's text field, so the chip's popover lists them, each
// with what it covers and when it ends, and a ✕ that shows the call again. The chip counts them.
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, within } from '@testing-library/react'
import { HideCallsPicker } from './HideCallsPicker'
import { hideCall, loadScopedHides } from '../features/hideCalls'

beforeEach(() => localStorage.clear())
afterEach(cleanup)

const chip = () => screen.getByRole('button', { name: /Hide calls/ })
async function open() {
  fireEvent.pointerDown(chip(), { button: 0, ctrlKey: false })
  return screen.findByRole('menu')
}

describe('#346 the Hide calls popover lists the scoped hides', () => {
  it('the chip counts them, with the plain list', () => {
    hideCall('VP8*', null, null)
    hideCall('W1AW', '20m', null)
    render(<HideCallsPicker />)
    expect(chip().textContent).toContain('· 2')
  })

  it('names each one’s band and end, and its ✕ shows the call again', async () => {
    hideCall('W1AW', '20m', null)
    hideCall('K1ABC', null, Date.UTC(2099, 0, 1, 13, 5))
    render(<HideCallsPicker />)
    const menu = await open()
    expect(within(menu).getByText('W1AW on 20m')).not.toBeNull()
    expect(within(menu).getByText('until you bring it back')).not.toBeNull()
    expect(within(menu).getByText('K1ABC on every band')).not.toBeNull()
    expect(within(menu).getByText('until 2099-01-01 13:05 UTC')).not.toBeNull()
    fireEvent.click(within(menu).getByRole('button', { name: 'Show W1AW again' }))
    expect(loadScopedHides().map((h) => h.call)).toEqual(['K1ABC'])
    expect(within(menu).queryByText('W1AW on 20m')).toBeNull()
  })

  it('the control: with nothing scoped there is no such list', async () => {
    render(<HideCallsPicker />)
    const menu = await open()
    expect(within(menu).queryByText('Hidden from a row’s menu')).toBeNull()
  })
})
