// @vitest-environment jsdom
//
// #330: the APRS map centres on the station grid, which lives in Settings ▸ Station, and the Lat
// and Lon boxes beside the beacon look as if they should move it. The beacon block now says where
// the grid is set, and links there.
import { it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react'
import { AprsCockpit } from './AprsCockpit'
import { EN } from '../i18n/en'
import defaultSettings from './__fixtures__/defaultSettings.json'

vi.mock('./MapView', () => ({ MapView: () => <div data-testid="map" /> }))

vi.mock('../api', () => ({
  // The map's Street choice reads the street map's state (features/streetMaps): not in a shell here.
  isTauri: () => false,
  aprsArm: vi.fn(async () => []),
  getAprsHeard: vi.fn(async () => []),
  getAprsHealth: vi.fn(async () => ({
    arm: 'explicit' as const,
    audioPeak: 0.3,
    lastAudioUnix: Math.floor(Date.now() / 1000),
    framesSeen: 1,
    framesDecoded: 1,
    lastDecodeUnix: Math.floor(Date.now() / 1000),
  })),
  getAprsIsStatus: vi.fn(async () => ({
    enabled: false,
    connected: false,
    verified: false,
    packets: 0,
    lastPacketUnix: null,
    uplinkEnabled: false,
    uploaded: 0,
    gateRejected: 0,
    lastReject: null,
  })),
  getAprsStations: vi.fn(async () => ({ stations: [], ttlMin: 60, fadeAfterMin: 20 })),
  getAprsTxNotice: vi.fn(async () => null),
  aprsAutoArm: vi.fn(async () => true),
  aprsSendBeacon: vi.fn(async () => {}),
  aprsSendMessage: vi.fn(async () => {}),
  getSettings: vi.fn(async () => defaultSettings),
  setSettings: vi.fn(async () => ({})),
}))

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 12; i++) await Promise.resolve()
  })
}

afterEach(cleanup)

it('the beacon block names where the grid that centres the map is set, and opens it', async () => {
  const open = vi.fn()
  render(<AprsCockpit active theme="dark" myGrid="EM28" onTune={() => {}} onOpenSettings={open} />)
  await settle()
  const note = document.querySelector('.aprs-beacon-note') as HTMLElement
  expect(note).toBeTruthy()
  expect(EN['aprs.beacon.gridNote']).toMatch(/Settings ▸ Station/)
  fireEvent.click(screen.getByRole('button', { name: /Settings ▸ Station/ }))
  expect(open).toHaveBeenCalledWith('operator-radio')
})

it('without a way to open Settings, the place is named, not linked', async () => {
  render(<AprsCockpit active theme="dark" myGrid="EM28" onTune={() => {}} />)
  await settle()
  const note = document.querySelector('.aprs-beacon-note') as HTMLElement
  expect(note.textContent).toMatch(/Settings ▸ Station/)
  expect(note.querySelector('button')).toBeNull()
})
