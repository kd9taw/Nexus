// @vitest-environment jsdom
//
// THE SPOT DIALOG GIVES THE KEYBOARD BACK TO WHERE IT WAS OPENED FROM — cancelled, escaped or
// posted. It used to be left on the page itself.

import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { useEffect, useState } from 'react'
import { SpotDialog } from './SpotDialog'
import { ConfirmHost } from '../confirm'
import { postSpot } from '../api'
import { t } from '../i18n'

vi.mock('../api', () => ({ postSpot: vi.fn(async () => {}) }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

beforeEach(() => vi.clearAllMocks())
afterEach(() => cleanup())

function Cockpit() {
  const [open, setOpen] = useState(false)
  return (
    <>
      <button type="button" onClick={() => setOpen(true)}>
        Spot this station
      </button>
      <SpotDialog open={open} onClose={() => setOpen(false)} initialCall="JA2DEF" freqMhz={14.074} defaultComment="" />
      <ConfirmHost />
    </>
  )
}
async function openFromSpot() {
  const spot = screen.getByRole('button', { name: 'Spot this station' })
  act(() => spot.focus())
  fireEvent.click(spot)
  await screen.findByRole('dialog', { name: t('spots.post.aria') })
  expect(document.activeElement?.tagName, 'the call box has the keyboard').toBe('INPUT')
  return spot
}

it('cancelled: back to Spot', async () => {
  render(<Cockpit />)
  const spot = await openFromSpot()
  fireEvent.click(screen.getByRole('button', { name: t('spots.post.cancel') }))
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  expect(document.activeElement).toBe(spot)
})

it('escaped: back to Spot', async () => {
  render(<Cockpit />)
  const spot = await openFromSpot()
  fireEvent.keyDown(document.activeElement!, { key: 'Escape' })
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  expect(document.activeElement).toBe(spot)
})

it('posted: back to Spot', async () => {
  render(<Cockpit />)
  const spot = await openFromSpot()
  fireEvent.click(screen.getByRole('button', { name: t('spots.post.submit') }))
  await waitFor(() => expect(postSpot).toHaveBeenCalledTimes(1))
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  expect(document.activeElement).toBe(spot)
})


it('opened from an item of a menu: back to the menu’s button, since the item goes with the menu', async () => {
  function FromMenu() {
    const [open, setOpen] = useState(false)
    // The menu goes a render after the dialog opens, as a closing Radix menu does.
    const [menuGone, setMenuGone] = useState(false)
    useEffect(() => setMenuGone(open), [open])
    return (
      <>
        <button type="button" id="more">More</button>
        {!menuGone && (
          <div role="menu" aria-labelledby="more">
            <div role="menuitem" tabIndex={-1} onClick={() => setOpen(true)}>Spot</div>
          </div>
        )}
        <SpotDialog open={open} onClose={() => setOpen(false)} initialCall="JA2DEF" freqMhz={14.074} defaultComment="" />
      </>
    )
  }
  render(<FromMenu />)
  const item = screen.getByRole('menuitem')
  act(() => item.focus())
  fireEvent.click(item)
  await screen.findByRole('dialog', { name: t('spots.post.aria') })
  fireEvent.keyDown(document.activeElement!, { key: 'Escape' })
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  expect(document.activeElement).toBe(screen.getByRole('button', { name: 'More' }))
})
