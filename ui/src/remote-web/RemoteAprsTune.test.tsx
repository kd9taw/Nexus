// @vitest-environment jsdom
// Remote parity batch 1: the APRS channel pick and Re-tune from a browser. Only an explicit pick
// moves the station's radio, and only while the station advertises aprsTuning; the cockpit's
// entry auto-tune stays local-only, so opening APRS in a browser never retunes anything.
import { afterEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { AprsCockpit } from '../components/AprsCockpit'
import { RemoteCollectionsContext, type RemoteCollections } from './collections'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { OperationClient } from './operation-client'
import { pendingControlStorage } from './control-storage'
import { pendingLogStorage, type ReceiptLock } from './operation-storage'
import type { OperationState } from './operation-protocol'
import type { ControlCapability } from './station-operation'
import { t } from '../i18n'

vi.mock('../components/MapView', () => ({ MapView: () => <div data-testid="map" /> }))
vi.mock('../api', () => ({
  aprsArm: vi.fn(async () => []),
  getAprsHeard: vi.fn(async () => []),
  getAprsHealth: vi.fn(async () => null),
  getAprsIsStatus: vi.fn(async () => null),
  getAprsStations: vi.fn(async () => ({ stations: [], ttlMin: 60, fadeAfterMin: 20 })),
  aprsAutoArm: vi.fn(async () => true),
  aprsSendBeacon: vi.fn(async () => {}),
  aprsSendMessage: vi.fn(async () => {}),
  getSettings: vi.fn(async () => { throw new Error('applicationUnsupported') }),
  setSettings: vi.fn(async () => ({})),
}))

afterEach(() => { cleanup(); vi.restoreAllMocks() })

const live = {
  health: { arm: 'explicit', audioPeak: 0.3, lastAudioUnix: null, drains: 0, framesSeen: 0, framesDecoded: 0, lastDecodeUnix: null,
    lastFrameSeenUnix: null, framePeak: 0, maxFramePeak: 0, frameClippedSamples: 0, radioName: 'IC-9700', bandRadioCount: 1 },
  isStatus: { enabled: false, connected: false, verified: false, packets: 0, lastPacketUnix: null, uplinkEnabled: false, uploaded: 0, gateRejected: 0, lastReject: null },
  settings: { mygrid: 'FN31', aprsChannelMhz: 144.39, aprsComment: '', aprsPath: [], aprsSymbolTable: '/', aprsSymbolCode: '-', aprsIsEnabled: false, aprsIsRadiusKm: 100, aprsIsWatchCalls: [] },
}

function operations(capabilities: ControlCapability[]) {
  const values = new Map<string, string>(), sent: string[] = []
  const data = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const lock: ReceiptLock = async (_k, run) => run()
  const client = new OperationClient(s => sent.push(s), true, () => 1000, pendingLogStorage(() => data, 'station', lock), 3, pendingControlStorage(() => data, 'station', lock))
  client.open()
  const state: OperationState = { stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(), revision: 1,
    commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false,
    controls: { context: { radioId: 1, radioConnection: 1, ampConnection: null, ampReadSequence: null }, capabilities } }
  client.receive({ type: 'operationResponse', requestId: JSON.parse(sent[sent.length - 1]).request.requestId, value: state })
  return client
}

async function cockpit(capabilities: ControlCapability[]) {
  const tuned: number[] = []
  const source = {
    client: { invoke: vi.fn(async (command: string) => { if (command === 'get_remote_aprs_state') return live; throw new Error('applicationUnsupported') }) },
    page: vi.fn(async () => { throw new Error('applicationUnavailable') }),
  } as unknown as RemoteCollections
  const radio = { dialMhz: 145.5, band: '2m', sideband: 'FM', transmitting: false } as never
  render(<StationControlContext.Provider value={false}><StationDataContext.Provider value={true}>
    <RemoteOperationsContext.Provider value={operations(capabilities)}>
      <RemoteCollectionsContext.Provider value={source}>
        <AprsCockpit active theme="dark" myGrid="FN31" radio={radio} onTune={(f) => tuned.push(f)} />
      </RemoteCollectionsContext.Provider>
    </RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>)
  const select = () => screen.getByTitle(t('aprs.channel.title')) as HTMLSelectElement
  await waitFor(() => expect(select().value).toBe('144.39'))
  await act(async () => { for (let i = 0; i < 8; i++) await Promise.resolve() })
  return { tuned, select, retune: () => screen.getByText(t('aprs.retune.label')).closest('button') as HTMLButtonElement }
}

it('never retunes on entry from a browser, and an explicit pick sends exactly one tune while the station advertises it', async () => {
  const view = await cockpit(['aprsTuning'])
  // The station's channel is known and the view is active: the local entry auto-tune would have
  // fired by now. A browser opening APRS must not move the radio.
  expect(view.tuned).toEqual([])
  expect(view.select().disabled).toBe(false)
  expect(view.retune().disabled).toBe(false)
  // Positive control: the operator's explicit pick is the one gesture that tunes.
  fireEvent.change(view.select(), { target: { value: '144.8' } })
  expect(view.tuned).toEqual([144.8])
  fireEvent.click(view.retune())
  expect(view.tuned).toEqual([144.8, 144.8])
})

it('keeps the channel pick and Re-tune dead without the station hint', async () => {
  const view = await cockpit(['decoder', 'frequency', 'repeaterTuning'])
  expect(view.select().disabled).toBe(true)
  expect(view.retune().disabled).toBe(true)
  fireEvent.change(view.select(), { target: { value: '144.8' } })
  fireEvent.click(view.retune())
  expect(view.tuned).toEqual([])
})
