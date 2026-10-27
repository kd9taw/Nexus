// @vitest-environment jsdom
//
// THE FT STRIP'S STOP TX ON THE DESKTOP: the button is live whatever the readings say, and a
// press halts.
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { FtStopControl } from './FtStopControl'

afterEach(cleanup)

it('is live on the desktop, and a press halts', () => {
  const onHaltTx = vi.fn()
  render(<FtStopControl onHaltTx={onHaltTx} />)
  const stop = screen.getByRole('button', { name: 'Stop TX' }) as HTMLButtonElement
  expect(stop.disabled).toBe(false)
  fireEvent.click(stop)
  expect(onHaltTx).toHaveBeenCalledTimes(1)
})
