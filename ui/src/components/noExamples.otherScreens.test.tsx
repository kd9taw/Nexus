// @vitest-environment jsdom
//
// NO MADE-UP SAMPLE VALUES IN AN EMPTY BOX, outside Settings and the log line: the POTA/SOTA
// activation box, the spot dialog's comment, the hide-calls filter, and the catalog entries the
// other boxes read. A sample in grey (K-1234, W7A/MN-001, VP8* R0* K1ABC, "e.g. FT8 up 2") reads as
// a value already filled in. Checked by value, box by box.
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import type { AppSnapshot } from '../types'
import { EN, t } from '../i18n'

const api = vi.hoisted(() => ({
  getOtaSpots: vi.fn(async () => []),
  getActivation: vi.fn(async () => null),
  parksCount: vi.fn(async () => 0),
  huntedParksCount: vi.fn(async () => 0),
}))
vi.mock('../api', () => ({
  ...api,
  selfSpot: vi.fn(), clearHuntTarget: vi.fn(), openPanelWindow: vi.fn(), setHuntTarget: vi.fn(), setActivation: vi.fn(),
  clearActivation: vi.fn(), downloadParks: vi.fn(), importParksCsv: vi.fn(), importHuntedParksCsv: vi.fn(),
  setActivationState: vi.fn(), postSpot: vi.fn(),
}))
vi.mock('../toast', async (original) => ({ ...(await original<typeof import('../toast')>()), pushToast: vi.fn() }))

import { PotaSotaView } from './PotaSotaView'
import { SpotDialog } from './SpotDialog'
import { HideCallsPicker } from './HideCallsPicker'

afterEach(() => { cleanup(); localStorage.clear() })

it('the POTA/SOTA activation box shows no sample reference, for either programme', async () => {
  render(<PotaSotaView snap={{ hunt: null, radio: { dialMhz: 14.285 } } as unknown as AppSnapshot} />)
  const box = (await screen.findByText(t('ota.activation.label')))
    .parentElement!.querySelector('input.pota-act-ref') as HTMLInputElement
  expect(box.getAttribute('placeholder')).toBeNull()
  fireEvent.change(box.parentElement!.querySelector('select.pota-act-prog')!, { target: { value: 'SOTA' } })
  expect(box.getAttribute('placeholder')).toBeNull()
})

it('the spot dialog\'s comment box shows no sample comment', () => {
  render(<SpotDialog open onClose={() => {}} initialCall="JA2DEF" freqMhz={14.074} defaultComment="" />)
  const box = screen.getByText(t('spots.post.comment.label')).closest('label')!.querySelector('input') as HTMLInputElement
  expect(box.value).toBe('')
  expect(box.getAttribute('placeholder')).toBeNull()
})

it('the hide-calls box shows no sample calls', async () => {
  render(<HideCallsPicker />)
  fireEvent.keyDown(screen.getByRole('button', { name: new RegExp(t('hideCalls.chip.label')) }), { key: 'Enter' })
  const box = (await screen.findByText(t('hideCalls.head'))).parentElement!.querySelector('input') as HTMLInputElement
  expect(box.getAttribute('placeholder')).toBeNull()
})

it('the catalog carries no sample in a box\'s words', () => {
  const cat = EN as unknown as Record<string, unknown>
  // Words that stay, with the sample cut off.
  expect(cat['program.origin.place.placeholder']).toBe('Grid or city')
  expect(cat['settings.alerts.geo.entities.placeholder']).toBe('type to search')
  expect(cat['settings.rigControl.serialPort.placeholder']).toBe('Select or type')
  expect(cat['settings.rigControl.pttPort.placeholder']).toBe('blank = use the CAT port')
  expect(cat['settings.rotator.external.placeholder']).toBe('host:port')
  // Entries that were nothing but a sample are gone; their boxes are empty.
  for (const key of [
    'hideCalls.placeholder',
    'watchlist.add.value.placeholder.call',
    'watchlist.add.value.placeholder.grid',
    'watchlist.add.value.placeholder.dxcc',
    'spots.post.comment.placeholder',
    'settings.station.opName.placeholder',
    'settings.profiles.save.placeholder',
    'settings.fdWho.position.placeholder',
    'settings.fdClub.eventName.placeholder',
    'settings.confirmations.eqsl.qthNickname.placeholder',
  ]) {
    expect(cat[key], key).toBeUndefined()
  }
})
