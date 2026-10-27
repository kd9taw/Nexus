// @vitest-environment jsdom
//
// The activator's strip as the field needs it: today's count per park POTA's way ("7 of 10 · 3
// to go", valid at ten), a two-fer's parks each counted, the P2P count, the grid the contacts carry
// (never Settings'), and a P2P badge on every POTA activator on the board while you activate.
// The station's counts are taken as given (they are the hot index's, tested there); the api is a
// mock and nothing here reaches a backend.
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { Activation, AppSnapshot, OtaSpot } from '../types'
import { t } from '../i18n'

const api = vi.hoisted(() => ({
  getOtaSpots: vi.fn(async (_program: string, _cached?: boolean): Promise<OtaSpot[]> => []),
  getActivation: vi.fn(async () => ({ program: null, reference: null, qsoCount: 0 })),
  setActivationGrid: vi.fn(),
  resumeActivation: vi.fn(),
  parksCount: vi.fn(async () => 0),
  huntedParksCount: vi.fn(async () => 0),
}))
vi.mock('../api', () => ({
  ...api,
  selfSpot: vi.fn(), clearHuntTarget: vi.fn(), openPanelWindow: vi.fn(), setHuntTarget: vi.fn(), setActivation: vi.fn(),
  setActivationState: vi.fn(), clearActivation: vi.fn(), downloadParks: vi.fn(), importParksCsv: vi.fn(),
  importHuntedParksCsv: vi.fn(),
}))
vi.mock('../toast', async (original) => ({ ...(await original<typeof import('../toast')>()), pushToast: vi.fn() }))

import { PotaSotaView } from './PotaSotaView'

const activation = (over: Partial<Activation> = {}): Activation => ({
  program: 'POTA',
  reference: 'US-1234',
  qsoCount: 7,
  states: [],
  myState: null,
  parks: [{ reference: 'US-1234', count: 7, p2p: 0 }],
  grid: 'EN52',
  utcDay: 20_694,
  ...over,
})
const snapWith = (a: Activation | null) =>
  ({ hunt: null, radio: { dialMhz: 14.285 }, activation: a }) as unknown as AppSnapshot

const spot = (activator: string, reference: string, program = 'POTA'): OtaSpot => ({
  program,
  reference,
  name: 'Test park',
  activator,
  freqKhz: 14_285,
  mode: 'SSB',
  spotter: null,
  comment: null,
  grid: null,
  newPark: false,
  bandOpen: false,
  huntedToday: false,
})

beforeEach(() => {
  api.getOtaSpots.mockReset()
  api.getOtaSpots.mockResolvedValue([])
  api.setActivationGrid.mockReset()
})
afterEach(() => {
  cleanup()
  localStorage.clear()
})

it('counts today per park: 7 of 10 with 3 to go, and valid at ten', () => {
  const { rerender } = render(<PotaSotaView snap={snapWith(activation())} />)
  const chip = screen.getByText((_, el) => el?.classList.contains('pota-act-progress') ?? false)
  expect(chip.textContent).toBe(
    `${t('ota.activation.progress', { reference: 'US-1234', count: 7 })} · ${t('ota.activation.toGo', { count: 3 })}`,
  )
  expect(chip.classList.contains('valid')).toBe(false)
  rerender(<PotaSotaView snap={snapWith(activation({ parks: [{ reference: 'US-1234', count: 10, p2p: 0 }] }))} />)
  const valid = document.querySelector('.pota-act-progress')!
  expect(valid.textContent).toBe(
    `${t('ota.activation.progress', { reference: 'US-1234', count: 10 })} · ${t('ota.activation.valid')}`,
  )
  expect(valid.classList.contains('valid')).toBe(true)
})

it('shows each park of a two-fer and the P2P count', () => {
  render(
    <PotaSotaView
      snap={snapWith(
        activation({
          reference: 'US-1234,US-5678',
          parks: [
            { reference: 'US-1234', count: 4, p2p: 2 },
            { reference: 'US-5678', count: 4, p2p: 3 },
          ],
        }),
      )}
    />,
  )
  const chips = [...document.querySelectorAll('.pota-act-progress:not(.p2p)')].map((c) => c.textContent)
  expect(chips).toEqual([
    `${t('ota.activation.progress', { reference: 'US-1234', count: 4 })} · ${t('ota.activation.toGo', { count: 6 })}`,
    `${t('ota.activation.progress', { reference: 'US-5678', count: 4 })} · ${t('ota.activation.toGo', { count: 6 })}`,
  ])
  expect(document.querySelector('.pota-act-progress.p2p')?.textContent).toBe(
    t('ota.activation.p2p.parks', { parks: 'US-1234 2 · US-5678 3' }),
  )
})

it('one park shows its P2P count alone', () => {
  render(<PotaSotaView snap={snapWith(activation({ parks: [{ reference: 'US-1234', count: 4, p2p: 2 }] }))} />)
  expect(document.querySelector('.pota-act-progress.p2p')?.textContent).toBe(t('ota.activation.p2p', { count: 2 }))
})

it('an activation taken up at launch shows as paused, with Still activating and Stop and no Spot me', async () => {
  api.resumeActivation.mockResolvedValue(activation())
  render(<PotaSotaView snap={snapWith(activation({ restored: true }))} />)
  expect(screen.getByText(t('ota.activation.paused', { reference: 'US-1234' }))).toBeTruthy()
  expect(screen.queryByRole('button', { name: t('ota.selfSpot.button') })).toBeNull()
  expect(screen.queryByRole('textbox', { name: t('ota.selfSpot.comment.label') })).toBeNull()
  expect(screen.getByRole('button', { name: new RegExp(t('ota.activation.stop.label')) })).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: t('ota.activation.resume.label') }))
  await waitFor(() => expect(api.resumeActivation).toHaveBeenCalledTimes(1))
})

it('an observed station: Still activating and Stop are each the station’s action, offered as Stop is', async () => {
  const observation = {
    feeds: [],
    activation: activation({ restored: true }),
    hunt: null,
    parkCount: 0,
    huntedCount: 0,
  }
  const remote = { stopActivation: vi.fn(), resumeActivation: vi.fn() }
  api.resumeActivation.mockClear()
  const { rerender } = render(<PotaSotaView snap={snapWith(null)} observation={observation} remote={remote} />)
  fireEvent.click(screen.getByRole('button', { name: t('ota.activation.resume.label') }))
  expect(remote.resumeActivation).toHaveBeenCalledTimes(1)
  expect(api.resumeActivation).not.toHaveBeenCalled()
  // Not offered by the station: neither button, as with Stop.
  rerender(<PotaSotaView snap={snapWith(null)} observation={observation} remote={{}} />)
  expect(screen.queryByRole('button', { name: t('ota.activation.resume.label') })).toBeNull()
  expect(screen.queryByRole('button', { name: new RegExp(t('ota.activation.stop.label')) })).toBeNull()
})

it('a live activation offers no Still activating', () => {
  render(<PotaSotaView snap={snapWith(activation())} />)
  expect(screen.queryByRole('button', { name: t('ota.activation.resume.label') })).toBeNull()
  expect(screen.getByRole('button', { name: t('ota.selfSpot.button') })).toBeTruthy()
})

it('the snapshot is live: the count follows it, and no activation shows the Start form', () => {
  const { rerender } = render(<PotaSotaView snap={snapWith(activation())} />)
  rerender(<PotaSotaView snap={snapWith(activation({ parks: [{ reference: 'US-1234', count: 8, p2p: 0 }] }))} />)
  expect(document.querySelector('.pota-act-progress')!.textContent).toContain(
    t('ota.activation.progress', { reference: 'US-1234', count: 8 }),
  )
  rerender(<PotaSotaView snap={snapWith(null)} />)
  expect(document.querySelector('.pota-act-progress')).toBeNull()
  expect(screen.getByRole('button', { name: t('ota.activation.start') })).toBeTruthy()
})

it('the grid box shows the activation’s grid and sends a typed one on Enter, only when changed', async () => {
  api.setActivationGrid.mockResolvedValue(activation({ grid: 'EN52WA' }))
  render(<PotaSotaView snap={snapWith(activation())} />)
  const box = screen.getByDisplayValue('EN52') as HTMLInputElement
  fireEvent.keyDown(box, { key: 'Enter' })
  expect(api.setActivationGrid).not.toHaveBeenCalled()
  fireEvent.change(box, { target: { value: 'en52wa' } })
  fireEvent.keyDown(box, { key: 'Enter' })
  await waitFor(() => expect(api.setActivationGrid).toHaveBeenCalledWith('EN52WA'))
})

it('with no park grid known the strip says so beside the box, and a grid takes the note away', () => {
  const { rerender } = render(<PotaSotaView snap={snapWith(activation({ grid: null }))} />)
  const note = screen.getByText(t('ota.activation.grid.noPark'))
  const box = screen.getByRole('textbox', { name: t('ota.activation.grid.label') })
  expect(box.getAttribute('aria-describedby')).toBe(note.id)
  rerender(<PotaSotaView snap={snapWith(activation({ program: 'SOTA', reference: 'W7A/MN-001', parks: [], grid: null }))} />)
  expect(screen.getByText(t('ota.activation.grid.noSummit'))).toBeTruthy()
  expect(screen.queryByText(t('ota.activation.grid.noPark'))).toBeNull()
  rerender(<PotaSotaView snap={snapWith(activation({ grid: 'EN52' }))} />)
  expect(screen.queryByText(t('ota.activation.grid.noPark'))).toBeNull()
  expect(screen.getByRole('textbox', { name: t('ota.activation.grid.label') }).getAttribute('aria-describedby')).toBeNull()
})

it('badges every POTA activator P2P while you activate, and none when you do not', async () => {
  api.getOtaSpots.mockImplementation(async (program: string) =>
    program === 'POTA' ? [spot('K1ABC', 'US-0001')] : [spot('W7XYZ', 'W7A/MN-001', 'SOTA')],
  )
  const { rerender } = render(<PotaSotaView snap={snapWith(activation())} />)
  await screen.findByText('K1ABC')
  expect(document.querySelectorAll('.pota-badge-p2p')).toHaveLength(1)
  rerender(<PotaSotaView snap={snapWith(null)} />)
  expect(document.querySelectorAll('.pota-badge-p2p')).toHaveLength(0)
})
