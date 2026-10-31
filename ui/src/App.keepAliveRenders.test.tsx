// @vitest-environment jsdom
//
// WHAT ONE SNAPSHOT POLL RENDERS, through the REAL App.
//
// App keeps seven cockpits mounted behind hidden hosts (Operate, RTTY, PSK, JTTY, SSTV, APRS, JS8) so
// their decoders keep accumulating, and polls the snapshot every 300 ms. A hidden cockpit must sit out
// a poll that changed nothing it shows — including a poll where only the per-poll meter fields moved,
// which is every poll while the audio engine runs — and must still render, with the new value, the
// moment anything it is given changes. The cockpit on screen renders on every poll as it always did.
// The cockpits here are counting stubs, so what is counted is exactly what App hands them.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor, act, fireEvent } from '@testing-library/react'
import type { AppSnapshot, Settings } from './types'
import defaults from './components/__fixtures__/defaultSettings.json'

const snapshot = {
  mycall: 'W9XYZ',
  mygrid: 'EN52',
  mode: 'Normal',
  radio: {
    dialMhz: 14.074,
    band: '20m',
    catOk: true,
    sideband: 'USB',
    transmitting: false,
    txEnabled: false,
    txAllowed: true,
    rxOffsetHz: 1500,
    txOffsetHz: 1500,
    txLevel: 0.5,
    slot: 0,
    nextSlotMs: 9000,
    rxLevel: 0.2,
    smeterDb: -100,
    clockAgeSecs: 4,
  },
  aiCw: { enabled: false, status: '', text: '' },
  link: { tier: 'FT8', periodSecs: 15, snrDb: 0, dtSec: 0, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
  stations: [{ call: 'K1ABC', grid: 'FN42', snr: -5, tier: 'FT8', lastSlot: 0 }],
  conversations: [],
  activePeer: null,
  qso: null,
  fieldDay: null,
  recentDecodes: [
    { from: 'K1ABC', snr: -5, dtSec: 0.1, freqHz: 1200, message: 'CQ K1ABC FN42', isCq: true, directedToMe: false, worked: false, tier: 'FT8', rv: 0 },
  ],
  harqRescues: 0,
  hunt: null,
  b4MatchMode: false,
} as unknown as AppSnapshot

/** A fresh copy, as the IPC bridge decodes one every poll, with `edit` applied. */
function poll(edit: (s: AppSnapshot) => void = () => {}): AppSnapshot {
  const s = structuredClone(snapshot)
  edit(s)
  return s
}

let deliver: ((s: AppSnapshot) => void) | null = null
/** The station's state as the backend would answer it now: the last poll sent. A command that answers
 *  with a snapshot (the mode switch a screen change makes) answers with this one. */
let latest: AppSnapshot = poll()

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    clientStatus: vi.fn(async () => null),
    askLog: vi.fn(async () => {
      throw new Error('no log in this test')
    }),
    getSnapshot: vi.fn(async () => structuredClone(latest)),
    subscribeSnapshot: vi.fn((fn: (s: AppSnapshot) => void) => {
      deliver = fn
      return () => {
        deliver = null
      }
    }),
    getAwards: vi.fn(async () => ({ achievements: [] })),
    getJourney: vi.fn(async () => ({ firsts: [], feats: [], ladders: [] })),
    getSettings: vi.fn(async () => structuredClone(defaults)),
    getBandPlan: vi.fn(async () => []),
    getLicensedBandPlan: vi.fn(async () => []),
    getFdRuleset: vi.fn(async () => null),
    logOperators: vi.fn(async () => []),
    logActivations: vi.fn(async () => []),
    radioLaunchInfo: vi.fn(async () => ({ showPicker: false })),
    uiStateLoad: vi.fn(async () => ({})),
    uiStateSave: vi.fn(async () => ({})),
    getAllSpots: vi.fn(async () => []),
    getNeedAlerts: vi.fn(async () => []),
    getPropagation: vi.fn(async () => null),
    getFeedHealth: vi.fn(async () => null),
    getXrayNow: vi.fn(async () => null),
    getDxpedWindows: vi.fn(async () => []),
    getSatSchedule: vi.fn(async () => []),
    getSatTrackStatus: vi.fn(async () => null),
    getIssPass: vi.fn(async () => null),
    getTleStatus: vi.fn(async () => null),
    getKpForecast: vi.fn(async () => null),
    setOperatingMode: vi.fn(async () => structuredClone(latest)),
    setArea: vi.fn(async () => structuredClone(latest)),
    haltTx: vi.fn(async () => structuredClone(latest)),
    setTier: vi.fn(async () => structuredClone(latest)),
    appVersion: vi.fn(async () => '0.0.0-test'),
    openPanelWindow: vi.fn(async () => {}),
    setContestEsm: vi.fn(async () => {}),
    js8SetGroups: vi.fn(async (groups: string[]) => groups),
  }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))

// Each cockpit: a stub that counts its renders and keeps the props of the last one.
type Props = Record<string, unknown>
const renders: Record<string, number> = {}
const last: Record<string, Props> = {}
function stub(name: string) {
  return (p: Props) => {
    renders[name] = (renders[name] ?? 0) + 1
    last[name] = p
    return <div data-stub={name} />
  }
}
vi.mock('./components/OperateCockpit', async (o) => ({ ...(await o<object>()), OperateCockpit: stub('operate') }))
vi.mock('./components/RttyCockpit', async (o) => ({ ...(await o<object>()), RttyCockpit: stub('rtty') }))
vi.mock('./components/PskCockpit', async (o) => ({ ...(await o<object>()), PskCockpit: stub('psk') }))
vi.mock('./components/JttyCockpit', async (o) => ({ ...(await o<object>()), JttyCockpit: stub('jtty') }))
vi.mock('./components/SstvView', async (o) => ({ ...(await o<object>()), SstvView: stub('sstv') }))
vi.mock('./components/AprsCockpit', async (o) => ({ ...(await o<object>()), AprsCockpit: stub('aprs') }))
vi.mock('./components/Js8Cockpit', async (o) => ({ ...(await o<object>()), Js8Cockpit: stub('js8') }))

import App from './App'

vi.setConfig({ testTimeout: 20_000 })

const COCKPITS = ['operate', 'rtty', 'psk', 'jtty', 'sstv', 'aprs', 'js8'] as const
const HIDDEN_FROM_OPERATE = COCKPITS.filter((c) => c !== 'operate')

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem(
    'nexus.features.v1',
    JSON.stringify({
      profile: 'custom',
      enabled: { rtty: true, psk: true, jtty: true, sstv: true, aprs: true, js8: true, logbook: true },
    }),
  )
  localStorage.setItem('nexus.workspace', 'dx')
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) =>
    ({
      matches: false,
      media: q,
      addEventListener() {},
      removeEventListener() {},
      addListener() {},
      removeListener() {},
    }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(() => {
  cleanup()
  deliver = null
  latest = poll()
})

/** Mount App on `view`, wait for settings and the first snapshot to reach every cockpit, settle. */
async function mount(view: string) {
  window.location.hash = `#${view}`
  render(<App />)
  await waitFor(() => {
    for (const c of COCKPITS) expect(last[c], c).toBeTruthy()
    expect(last.rtty.macros, 'settings loaded').toBeTruthy()
    expect(deliver).toBeTruthy()
  })
  // Let mount-time effects and their answers land before counting.
  await act(async () => {
    await new Promise((r) => setTimeout(r, 50))
  })
}
const zero = () => {
  for (const c of COCKPITS) renders[c] = 0
}
const send = async (s: AppSnapshot) => {
  latest = s
  await act(async () => {
    deliver!(s)
  })
}
const snapOf = (c: string) => last[c].snap as AppSnapshot
/** Press a section's button in the side nav, by its label. */
const go = async (label: string) => {
  const btn = [...document.querySelectorAll<HTMLButtonElement>('.mode-nav .mode-btn')].find(
    (b) => b.querySelector('.mode-label')?.textContent === label,
  )
  expect(btn, label).toBeTruthy()
  await act(async () => {
    fireEvent.click(btn!)
  })
}

describe('a snapshot poll renders only what it changed', () => {
  it('on Operate: hidden cockpits sit out unchanged and meter-only polls; Operate renders each one', async () => {
    await mount('operate')
    zero()
    const before = snapOf('operate')
    const blockedBefore = last.operate.blockedCalls
    await send(poll())
    await send(poll())
    for (const c of HIDDEN_FROM_OPERATE) expect(renders[c], c).toBe(0)
    // The screen on show still renders on every poll (its render-time clocks ride it) …
    expect(renders.operate).toBe(2)
    // … with the snapshot exactly as polled: a new decode list every poll, equal or not. The FT
    // decode panes re-ingest on a new list, which is what refills them after Erase or a UDP Clear.
    const after = snapOf('operate')
    expect(after).not.toBe(before)
    expect(after.recentDecodes).not.toBe(before.recentDecodes)
    expect(after.recentDecodes).toEqual(before.recentDecodes)
    // No block list loaded: still one array, so the decode panes' ignore set is not rebuilt per poll.
    expect(last.operate.blockedCalls).toBe(blockedBefore)

    // Every poll while the engine runs: only the meter fields move.
    zero()
    await send(
      poll((s) => {
        s.radio.nextSlotMs = 1200
        s.radio.rxLevel = 0.6
        s.radio.smeterDb = -73
        s.radio.clockAgeSecs = 9
      }),
    )
    for (const c of HIDDEN_FROM_OPERATE) expect(renders[c], c).toBe(0)
    expect(renders.operate).toBe(1)
    expect(snapOf('operate').radio.nextSlotMs).toBe(1200)
    expect(snapOf('operate').recentDecodes).not.toBe(after.recentDecodes)
  })

  it('a hidden cockpit renders, with the new value, when what it is given changes', async () => {
    await mount('operate')
    zero()
    await send(poll((s) => (s.radio.dialMhz = 7.074)))
    for (const c of HIDDEN_FROM_OPERATE) expect(renders[c], c).toBe(1)
    for (const c of ['rtty', 'psk', 'jtty', 'sstv', 'js8']) expect(snapOf(c).radio.dialMhz, c).toBe(7.074)
    expect((last.aprs.radio as AppSnapshot['radio']).dialMhz).toBe(7.074)
    // APRS's TX strip is an element App builds each render: its radio is the new one too.
    const strip = last.aprs.txStrip as { props: { radio: AppSnapshot['radio'] } }
    expect(strip.props.radio.dialMhz).toBe(7.074)

    zero()
    await send(poll((s) => ((s.radio.dialMhz = 7.074), (s.mygrid = 'FN31'))))
    expect(renders.aprs).toBe(1)
    expect(last.aprs.myGrid).toBe('FN31')

    // A meter field moving TOGETHER with a real change: the hidden cockpit gets both.
    zero()
    await send(poll((s) => ((s.radio.dialMhz = 3.573), (s.mygrid = 'FN31'), (s.radio.nextSlotMs = 300))))
    for (const c of HIDDEN_FROM_OPERATE) expect(renders[c], c).toBe(1)
    expect(snapOf('js8').radio.nextSlotMs).toBe(300)
  })

  it('on another screen every cockpit is hidden, and each follows its own data', async () => {
    await mount('logbook')
    zero()
    await send(poll())
    await send(poll((s) => ((s.radio.nextSlotMs = 100), (s.radio.rxLevel = 0.9))))
    for (const c of COCKPITS) expect(renders[c], c).toBe(0)

    // New decodes and a new roster reach the hidden FT cockpit: its decode history keeps
    // accumulating behind the host (the reason it is kept mounted at all).
    await send(
      poll((s) => {
        s.recentDecodes = [...s.recentDecodes, { ...s.recentDecodes[0], from: 'JA1XYZ', message: 'CQ JA1XYZ PM95' }]
      }),
    )
    expect(renders.operate).toBe(1)
    expect(snapOf('operate').recentDecodes.map((d) => d.from)).toEqual(['K1ABC', 'JA1XYZ'])

    zero()
    await send(
      poll((s) => {
        s.recentDecodes = [...s.recentDecodes, { ...s.recentDecodes[0], from: 'JA1XYZ', message: 'CQ JA1XYZ PM95' }]
        s.stations = [...s.stations, { ...s.stations[0], call: 'DL1QQ' }]
      }),
    )
    expect(renders.operate).toBe(1)
    // The roster is an element App builds each render; its stations are the new ones.
    const roster = last.operate.roster as { props: { stations: { call: string }[] } }
    expect(roster.props.stations.map((s) => s.call)).toEqual(['K1ABC', 'DL1QQ'])
    // The boxes the cockpit lends its panes carry the same stations.
    const boxes = last.operate.boxes as { stations: { call: string }[] }
    expect(boxes.stations.map((s) => s.call)).toEqual(['K1ABC', 'DL1QQ'])
  })

  it('a settings change reaches the hidden cockpits through the handlers they hold', async () => {
    await mount('logbook')
    zero()
    const macros = { ...(last.rtty.macros as Settings['macros']), rtty: [['CQ', 'CQ CQ DE W9XYZ']] }
    // The hidden RTTY cockpit saves its F-keys: App's mirror moves, and both keyboard cockpits get it.
    await act(async () => {
      ;(last.rtty.onMacrosSaved as (m: Settings['macros']) => void)(macros as Settings['macros'])
    })
    expect(last.rtty.macros).toEqual(macros)
    expect(last.psk.macros).toEqual(macros)
    expect(renders.rtty).toBeGreaterThan(0)
    expect(renders.psk).toBeGreaterThan(0)

    // JS8's saved messages: the hidden cockpit's own save handler, then its compose config shows them.
    await act(async () => {
      ;(last.js8.onSavedMessages as (m: string[]) => void)(['HELLO', 'BYE'])
    })
    expect((last.js8.compose as { savedMessages: string[] }).savedMessages).toEqual(['HELLO', 'BYE'])
    // JS8's group list, saved through the lists the hidden cockpit holds, comes back in those lists.
    await act(async () => {
      await (last.js8.lists as { setGroups: (g: string[]) => Promise<void> }).setGroups(['@HB'])
    })
    expect((last.js8.lists as { groups: string[] }).groups).toEqual(['@HB'])

    // An unchanged poll after all that still renders none of them.
    zero()
    await send(poll())
    for (const c of COCKPITS) expect(renders[c], c).toBe(0)
  })

  it('a cockpit shown again renders at once with everything it sat out, and hides again', async () => {
    await mount('logbook')
    zero()
    // Hidden: the meter fields move and the cockpit sits it out, so it still holds the old values.
    await send(poll((s) => ((s.radio.nextSlotMs = 4321), (s.radio.rxLevel = 0.7), (s.radio.clockAgeSecs = 30))))
    expect(renders.operate).toBe(0)
    expect(snapOf('operate').radio.nextSlotMs).toBe(9000)

    // Shown: it renders on the switch itself, before any further poll, with the newest snapshot.
    await go('FT')
    await waitFor(() => expect(last.operate.active).toBe(true))
    expect(renders.operate).toBeGreaterThan(0)
    expect(snapOf('operate').radio.nextSlotMs).toBe(4321)
    expect(snapOf('operate').radio.rxLevel).toBe(0.7)
    expect(snapOf('operate').radio.clockAgeSecs).toBe(30)
    // On screen it follows every poll, meter fields included.
    zero()
    await send(poll((s) => (s.radio.nextSlotMs = 1111)))
    expect(renders.operate).toBe(1)
    expect(snapOf('operate').radio.nextSlotMs).toBe(1111)

    // Another cockpit's screen: FT is told it is hidden (that render is delivered) …
    await go('RTTY')
    await waitFor(() => expect(last.operate.active).toBe(false))
    expect(last.rtty.active).toBe(true)
    // … then sits out meter-only polls again, while RTTY, now on screen, renders each one.
    zero()
    await send(poll((s) => (s.radio.nextSlotMs = 2222)))
    expect(renders.operate).toBe(0)
    expect(renders.rtty).toBe(1)
    expect(snapOf('rtty').radio.nextSlotMs).toBe(2222)
    // … and still renders for anything else that changes while it is hidden.
    await send(poll((s) => ((s.radio.nextSlotMs = 2222), (s.radio.rxOffsetHz = 900))))
    expect(renders.operate).toBe(1)
    expect(snapOf('operate').radio.rxOffsetHz).toBe(900)
  })

  it('the handlers a hidden cockpit holds are the ones App passes now', async () => {
    await mount('logbook')
    const held = { ...last.operate }
    await send(poll((s) => (s.radio.dialMhz = 21.074)))
    for (const k of ['onClearSelection', 'onPopOut', 'onOpenLogbook', 'onSnap', 'onToggleBlocked']) {
      expect(last.operate[k], k).toBe(held[k])
    }
    expect(last.operate.blockedCalls).toBe(held.blockedCalls)
    expect(last.rtty.onMacrosSaved).toBe(last.psk.onMacrosSaved)
  })
})
