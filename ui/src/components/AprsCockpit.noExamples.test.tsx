// @vitest-environment jsdom
//
// APRS ▸ internet: the Watched calls box is empty when nothing is watched — no sample callsigns
// in grey, which read as calls already being watched.
import { it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react'
import { AprsCockpit } from './AprsCockpit'
import { t } from '../i18n'
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

it('the Watched calls box shows no sample callsigns', async () => {
  render(<AprsCockpit active theme="dark" myGrid="EM28" onTune={() => {}} />)
  await settle()
  fireEvent.click(document.querySelector('.aprs-inet') as HTMLElement)
  const box = screen
    .getByText(t('aprs.inet.watch.label'))
    .closest('label')!
    .querySelector('input') as HTMLInputElement
  expect(box.value).toBe('')
  expect(box.getAttribute('placeholder')).toBeNull()
})
