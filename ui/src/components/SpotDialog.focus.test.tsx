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

// THE DIALOG HOLDS THE KEYBOARD while it is up. Tab once walked out past Spot onto the page behind
// the backdrop, Shift+Tab out of the call box onto the rows behind it, and Esc did nothing once the
// keyboard was on Cancel or Spot. A keydown the dialog does not cancel is one the browser acts on, so
// a Tab at either end must be cancelled AND land the keyboard on the other end.
const dialog = () => screen.getByRole('dialog', { name: t('spots.post.aria') })

it('Tab from the last control stays inside the dialog: on to the first', async () => {
  render(<Cockpit />)
  await openFromSpot()
  const call = document.activeElement
  const submit = screen.getByRole('button', { name: t('spots.post.submit') })
  act(() => submit.focus())
  const moved = !fireEvent.keyDown(submit, { key: 'Tab' })
  expect(moved, 'the browser does not move the keyboard out').toBe(true)
  expect(document.activeElement).toBe(call)
  expect(dialog().contains(document.activeElement)).toBe(true)
})

it('Shift+Tab from the first control stays inside the dialog: back to the last', async () => {
  render(<Cockpit />)
  await openFromSpot()
  const moved = !fireEvent.keyDown(document.activeElement!, { key: 'Tab', shiftKey: true })
  expect(moved, 'the browser does not move the keyboard out').toBe(true)
  expect(document.activeElement).toBe(screen.getByRole('button', { name: t('spots.post.submit') }))
})

it('the keyboard sent behind it (a click on the page) comes straight back into the dialog', async () => {
  render(<Cockpit />)
  const spot = await openFromSpot()
  const call = document.activeElement
  act(() => spot.focus())
  expect(document.activeElement).toBe(call)
})

it('CONTROL: Tab between two controls inside is left to the browser', async () => {
  render(<Cockpit />)
  await openFromSpot()
  expect(fireEvent.keyDown(document.activeElement!, { key: 'Tab' }), 'not cancelled').toBe(true)
})

it('Esc from Cancel closes it, back to where it was opened from', async () => {
  render(<Cockpit />)
  const spot = await openFromSpot()
  const cancel = screen.getByRole('button', { name: t('spots.post.cancel') })
  act(() => cancel.focus())
  fireEvent.keyDown(cancel, { key: 'Escape' })
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  expect(document.activeElement).toBe(spot)
  expect(postSpot).not.toHaveBeenCalled()
})

it('Esc from Spot closes it and sends nothing', async () => {
  render(<Cockpit />)
  await openFromSpot()
  const submit = screen.getByRole('button', { name: t('spots.post.submit') })
  act(() => submit.focus())
  fireEvent.keyDown(submit, { key: 'Escape' })
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  expect(postSpot).not.toHaveBeenCalled()
})

it('the opener gone while it is up (a list refreshed or scrolled away under it): back to its neighbour, never the page', async () => {
  function List() {
    const [open, setOpen] = useState(false)
    const [rows, setRows] = useState(['K1A', 'K2B', 'K3C'])
    return (
      <>
        <ul>
          {rows.map((c) => (
            <li key={c}>
              <button type="button" className="more" onClick={() => setOpen(true)}>
                {`More ${c}`}
              </button>
            </li>
          ))}
        </ul>
        <button type="button" onClick={() => setRows((r) => r.filter((c) => c !== 'K2B'))}>
          drop
        </button>
        <SpotDialog open={open} onClose={() => setOpen(false)} initialCall="K2B" freqMhz={14.074} defaultComment="" />
      </>
    )
  }
  render(<List />)
  const opener = screen.getByRole('button', { name: 'More K2B' })
  act(() => opener.focus())
  fireEvent.click(opener)
  await screen.findByRole('dialog', { name: t('spots.post.aria') })
  act(() => fireEvent.click(screen.getByRole('button', { name: 'drop', hidden: true })))
  expect(opener.isConnected).toBe(false)
  fireEvent.keyDown(document.activeElement!, { key: 'Escape' })
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  expect(document.activeElement).not.toBe(document.body)
  expect(document.activeElement).toBe(screen.getByRole('button', { name: 'More K3C' }))
})
