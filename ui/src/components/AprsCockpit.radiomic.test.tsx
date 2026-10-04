// @vitest-environment jsdom
//
// APRS CAN'T SEND WHILE THE RADIO HAS THE MIC (operator ruling, 2026-10-04, "Refuse like the voice
// keyer"). With Nexus's own Flex client and native DAX audio on, the radio takes its own mic in FM
// while a packet goes out over DAX, so the radio would ignore the packet and the mic would carry
// the over. A beacon or a message says so in the status line, in the operator's language, and
// sends nothing; without that state the same sends go out as before.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react'
import { AprsCockpit } from './AprsCockpit'
import { aprsSendBeacon, aprsSendMessage } from '../api'
import { DE } from '../i18n/de'
import { EN, installCatalog, setLocale } from '../i18n'
import defaultSettings from './__fixtures__/defaultSettings.json'

vi.mock('./MapView', () => ({ MapView: () => <div data-testid="map" /> }))

vi.mock('../api', () => ({
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

/** The cockpit on 144.390 FM with TX on; `radioHasMic` as the snapshot reports it. */
async function cockpit(radioHasMic: boolean) {
  render(
    <AprsCockpit
      active
      theme="dark"
      myGrid="EM28"
      onTune={() => {}}
      radio={{
        dialMhz: 144.39,
        band: '2m',
        sideband: 'FM',
        txEnabled: true,
        flexRadioHasMic: radioHasMic,
      }}
    />,
  )
  await settle()
}

const statusLine = () => document.querySelector('.aprs-status')?.textContent ?? null

async function sendBeacon() {
  fireEvent.change(screen.getByLabelText(EN['aprs.beacon.lat.label']), { target: { value: '41.88' } })
  fireEvent.change(screen.getByLabelText(EN['aprs.beacon.lon.label']), { target: { value: '-87.63' } })
  await act(async () => {
    fireEvent.click(screen.getByRole('button', { name: EN['aprs.beacon.send'] }))
  })
  await settle()
}

async function sendMessage() {
  fireEvent.change(screen.getByPlaceholderText(EN['aprs.msg.to.placeholder']), {
    target: { value: 'N0CALL' },
  })
  fireEvent.change(screen.getByPlaceholderText(EN['aprs.msg.text.placeholder']), {
    target: { value: 'hello' },
  })
  await act(async () => {
    fireEvent.click(screen.getByRole('button', { name: EN['aprs.msg.send'] }))
  })
  await settle()
}

beforeEach(() => {
  vi.clearAllMocks()
})
afterEach(() => {
  cleanup()
  setLocale('en')
})

describe('APRS while the radio has the mic', () => {
  it('sends no beacon and says why', async () => {
    await cockpit(true)
    await sendBeacon()
    expect(aprsSendBeacon).not.toHaveBeenCalled()
    expect(statusLine()).toBe(EN['aprs.status.radioHasMic'])
  })

  it('sends no message, says why, and keeps what was typed', async () => {
    await cockpit(true)
    await sendMessage()
    expect(aprsSendMessage).not.toHaveBeenCalled()
    expect(statusLine()).toBe(EN['aprs.status.radioHasMic'])
    expect(
      (screen.getByPlaceholderText(EN['aprs.msg.text.placeholder']) as HTMLInputElement).value,
    ).toBe('hello')
  })

  it('says it in the operator’s language', async () => {
    installCatalog('de', DE)
    setLocale('de')
    expect(DE['aprs.status.radioHasMic']).toBeTruthy()
    render(
      <AprsCockpit
        active
        theme="dark"
        myGrid="EM28"
        onTune={() => {}}
        radio={{ dialMhz: 144.39, band: '2m', sideband: 'FM', txEnabled: true, flexRadioHasMic: true }}
      />,
    )
    await settle()
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: DE['aprs.beacon.send'] as string }))
    })
    await settle()
    expect(aprsSendBeacon).not.toHaveBeenCalled()
    expect(statusLine()).toBe(DE['aprs.status.radioHasMic'])
  })

  it('sends as before when the radio does not have the mic', async () => {
    await cockpit(false)
    await sendBeacon()
    expect(aprsSendBeacon).toHaveBeenCalledTimes(1)
    expect(statusLine()).toBe(EN['aprs.status.beacon.queued'])
    await sendMessage()
    expect(aprsSendMessage).toHaveBeenCalledWith('N0CALL', 'hello')
  })
})
