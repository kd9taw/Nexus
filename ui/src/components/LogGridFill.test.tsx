// @vitest-environment jsdom
//
// BLANK GRIDS — the Logbook's offer to fill a grid Nexus knows into contacts logged with none.
// It changes nothing until the operator ticks contacts and confirms; it then sends exactly the
// ticked ones, each with its grid and where the grid came from, so a callbook grid is filled as the
// callbook's (for the distance and the map only). Each row says where its grid came from — heard
// on the air, or the home QTH from the callbook — and whether the contact was already uploaded.
// Driving another station, nothing that changes the log can be pressed.
import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest'
import { render, cleanup, fireEvent, screen, waitFor } from '@testing-library/react'
import type { LogGridOffer } from '../api'

const api = vi.hoisted(() => ({
  findLogGridOffers: vi.fn(),
  fillLogGrids: vi.fn(),
  undoHistoryRun: vi.fn(),
}))
vi.mock('../api', () => api)

import { LogGridFill } from './LogGridFill'

beforeEach(() => {
  for (const f of Object.values(api)) f.mockReset()
})
afterEach(cleanup)

const offers: LogGridOffer[] = [
  { id: 'a', call: 'W1AW', whenUnix: 1_767_268_800, band: '20m', mode: 'FT8', grid: 'FN31', source: 'heard', uploaded: true },
  { id: 'b', call: 'K1ABC', whenUnix: 1_767_268_800, band: '17m', mode: 'FT8', grid: 'FN42ab', source: 'callbook', uploaded: false },
]
const made = { changed: 1, run: 'run-1', gone: 0, changedSince: 0, needsChoice: 0, distinct: 0 }

describe('blank grids', () => {
  it('lists each offer with where its grid came from, and changes nothing by itself', async () => {
    api.findLogGridOffers.mockResolvedValue({ blank: 5, offers })
    render(<LogGridFill onChanged={() => {}} afar={false} />)
    fireEvent.click(screen.getByRole('button', { name: 'Find blank grids' }))
    await screen.findByText('With a grid known: 2 (of 5 contacts with no grid)')
    expect(screen.getByText('heard on the air · already uploaded: nothing is sent again')).toBeTruthy()
    expect(screen.getByText('home QTH (callbook)')).toBeTruthy()
    expect((screen.getByRole('button', { name: 'Fill the ticked: 0' }) as HTMLButtonElement).disabled).toBe(true)
    expect(api.fillLogGrids).not.toHaveBeenCalled()
  })

  it('fills only the ticked, each with its grid and source, after asking', async () => {
    api.findLogGridOffers.mockResolvedValue({ blank: 2, offers })
    api.fillLogGrids.mockResolvedValue(made)
    const onChanged = vi.fn()
    render(<LogGridFill onChanged={onChanged} afar={false} />)
    fireEvent.click(screen.getByRole('button', { name: 'Find blank grids' }))
    await screen.findByText('home QTH (callbook)')
    const boxes = screen.getAllByRole('checkbox') as HTMLInputElement[]
    fireEvent.click(boxes[2])
    fireEvent.click(screen.getByRole('button', { name: 'Fill the ticked: 1' }))
    expect(api.fillLogGrids).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'Yes, fill them' }))
    await waitFor(() => expect(api.fillLogGrids).toHaveBeenCalledWith([
      { id: 'b', grid: 'FN42ab', source: 'callbook' },
    ]))
    await waitFor(() => expect(onChanged).toHaveBeenCalled())
  })

  it('driving another station, nothing can be filled', async () => {
    api.findLogGridOffers.mockResolvedValue({ blank: 2, offers })
    render(<LogGridFill onChanged={() => {}} afar />)
    fireEvent.click(screen.getByRole('button', { name: 'Find blank grids' }))
    await screen.findByText('home QTH (callbook)')
    fireEvent.click(screen.getByRole('checkbox', { name: 'Tick all: 2' }))
    expect((screen.getByRole('button', { name: 'Fill the ticked: 2' }) as HTMLButtonElement).disabled).toBe(true)
  })
})
