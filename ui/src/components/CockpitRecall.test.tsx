// @vitest-environment jsdom
//
// THE FULL RECALL CARD IN THE OPERATING COCKPITS (operator, 2026-07-31: "full info back on
// logging when you resolve a call? It had the QRZ photo, bearing, etc.").
//
// 0.18.0's `compactRecall` cut the card to one line because the pre-overhaul cockpits had no
// interposed scroller — the card's height crushed the operating panes directly. The pane grid
// changed the structure: the log pane is a FILL pane whose `.pane-body` is
// `flex:1; min-height:0; overflow:auto` (CockpitPaneFrame), so a tall card scrolls INSIDE the
// log column and cannot squeeze the cockpit. These tests render the REAL cockpits with the REAL
// LogEntry (only scope/header/keyer chrome stubbed) and pin both halves of that claim:
//   - resolving a call shows the FULL card — photo <img> with the callbook URL, distance/bearing
//     derived from the operator's own grid, the prior-QSO list;
//   - the layout guard — the card lives inside the log pane's `.pane-body` (the scroller),
//     never as a shell-level sibling;
//   - the compact variant is gone from the rendered DOM (its classes died with their last
//     caller; the styles.css census lives in RecallPanel.test.tsx).
//
// Run RED before the change (both cockpits passed `compactRecall`, so the resolved call drew
// `.recall-compact` and no `.recall-card`), GREEN after the prop's removal.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, waitFor, within } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import { CwCockpit } from './CwCockpit'
import { distanceLabel, bearingLabel } from '../grid'
import type { AppSnapshot, LoggedQso, QrzLookup } from '../types'

const PHOTO = 'https://cdn-xfer.qrz.com/x/w1abc/photo.jpg'

const resolved: QrzLookup = {
  call: 'W1ABC',
  name: 'Alice Example',
  nickname: null,
  qth: 'Hartford, CT',
  grid: 'FN31',
  state: 'CT',
  country: 'United States',
  dxcc: 291,
  cqZone: 5,
  ituZone: 8,
  image: PHOTO,
}

const priorQsos = [
  {
    call: 'W1ABC',
    grid: 'FN31',
    band: '20m',
    freqMhz: 14.25,
    mode: 'SSB',
    rstSent: '59',
    rstRcvd: '57',
    whenUnix: Date.UTC(2026, 2, 14) / 1000, // 14 Mar 26
  },
  {
    call: 'W1ABC',
    grid: 'FN31',
    band: '15m',
    freqMhz: 21.3,
    mode: 'CW',
    rstSent: '599',
    rstRcvd: '579',
    whenUnix: Date.UTC(2025, 10, 2) / 1000, // 02 Nov 25
  },
] as LoggedQso[]

const decodeState = {
  text: 'CQ CQ DE KD9TAW',
  wpm: 22,
  sent: [] as string[],
  keyerError: null as string | null,
  candidates: [] as { call: string; best: boolean }[],
  state: 'listening',
  headline: '',
  prompt: '',
  recommended: null,
  workedCall: null,
  rst: null,
  name: null,
}

// Union of the two cockpits' api surfaces (the structure-test mocks) + LogEntry's own:
// getLog feeds the prior-contact history, qrzLookup is the call resolution under test.
vi.mock('../api', () => ({
  // LogEntry
  fdLogManual: vi.fn(async () => ({})),
  logQso: vi.fn(async () => ({})),
  getLog: vi.fn(async () => priorQsos),
  lookupPark: vi.fn(async () => null),
  lookupParkLive: vi.fn(async () => null),
  qrzLookup: vi.fn(async () => resolved),
  // LogEntry resolves the award identity from cty.dat (local, no network) rather than
  // trusting the QRZ country spelling. vi.mock replaces the whole module, so this has
  // to be listed or the lookup throws and the card never renders.
  resolveEntity: vi.fn(async () => null),
  searchParks: vi.fn(async () => []),
  setCwPeerInfo: vi.fn(async () => {}),
  // PhoneCockpit
  setPtt: vi.fn(async () => {}),
  setRfPower: vi.fn(async () => {}),
  setMicGain: vi.fn(async () => {}),
  startQsoRecording: vi.fn(async () => ({})),
  stopQsoRecording: vi.fn(async () => ({})),
  setSplit: vi.fn(async () => ({})),
  setSidebandOverride: vi.fn(async () => ({})),
  // CwCockpit
  getSettings: vi.fn(async () => ({ macros: { cwProfiles: [], activeCwProfile: 0 } })),
  setSettings: vi.fn(async () => ({})),
  sendCw: vi.fn(async () => {}),
  setCwKeyer: vi.fn(async () => null),
  setCwWpm: vi.fn(async () => {}),
  stopCw: vi.fn(async () => {}),
  cwDecode: vi.fn(async () => decodeState),
  cwClear: vi.fn(async () => {}),
  setAiCw: vi.fn(async () => {}),
  selectPeer: vi.fn(async () => null),
  previewCw: vi.fn(async (t: string) => t),
  pointRotatorAtCall: vi.fn(async () => 0),
  // shared
  setRigFunc: vi.fn(async () => ({})),
  setFilterWidth: vi.fn(async () => ({})),
  setNrLevel: vi.fn(async () => {}),
  setAgc: vi.fn(async () => ({})),
  setScopeSpan: vi.fn(async () => ({})),
  setScopeRef: vi.fn(async () => {}),
  setFlexPanSpan: vi.fn(async () => ({})),
  setFlexPanRef: vi.fn(async () => ({})),
  openPanelWindow: vi.fn(async () => {}),
  setTune: vi.fn(async () => ({})),
  setFrequency: vi.fn(async () => ({})),
  haltTx: vi.fn(async () => ({})),
}))

// Structure-irrelevant heavy chrome → stubs (the structure-test set, minus LogEntry: LogEntry
// and RecallPanel are the components under test and render REAL).
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="vk-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

/** Snap superset serving both cockpits; `mygrid` anchors the distance/bearing derivation. */
function makeSnap(rigMode: 'USB' | 'CW'): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    hunt: null,
    radio: {
      dialMhz: rigMode === 'CW' ? 14.05 : 14.2,
      band: '20m',
      catOk: true,
      sideband: 'USB',
      sidebandOverride: null,
      rigMode,
      transmitting: false,
      txEnabled: true,
      txAllowed: true,
      qsoRecording: false,
      cwWpm: 22,
      cwKeyer: 'cat',
      rfPower: null,
      micGain: null,
      nrLevel: 0.3,
      agc: 'fast',
      nb: true,
      nr: true,
      notch: null,
      comp: null,
      vox: null,
      filterWidthHz: 500,
      splitTxMhz: null,
      smeterDb: null,
      rxLevel: 0,
      phoneSegLo: null,
      phoneSegHi: null,
    },
  } as unknown as AppSnapshot
}

/** Type + blur a call in the log pane and wait for the QRZ resolution to land. */
async function resolveCall(): Promise<HTMLElement> {
  const logPane = document.querySelector('[data-pane="log"]') as HTMLElement
  expect(logPane, 'no log pane').not.toBeNull()
  const callInput = within(logPane).getByPlaceholderText('Call')
  await act(async () => {
    fireEvent.change(callInput, { target: { value: 'w1abc' } })
    fireEvent.blur(callInput) // the silent on-blur lookup path
  })
  await waitFor(() => expect(document.querySelector('.recall-card')).not.toBeNull())
  return logPane
}

function assertFullCard(logPane: HTMLElement) {
  const card = document.querySelector('.recall-card') as HTMLElement

  // LAYOUT GUARD — the reason the full card is safe at all: it renders inside the log pane's
  // .pane-body (the frame's internal scroller), never as a shell-level sibling that could
  // squeeze the cockpit the way the pre-overhaul card did.
  expect(card.closest('.pane-body'), 'card is not inside a .pane-body scroller').not.toBeNull()
  expect(logPane.contains(card), 'card escaped the log pane').toBe(true)
  const shell = document.querySelector('main.layout.single')!
  expect(Array.from(shell.children).includes(card), 'card is a shell-level sibling').toBe(false)

  // The 0.18.0-dropped inventory, back: photo …
  const img = card.querySelector('.recall-avatar-img') as HTMLImageElement
  expect(img, 'no callbook photo').not.toBeNull()
  expect(img.src).toBe(PHOTO)
  // … identity + QTH …
  expect(card.textContent).toContain('Alice Example')
  expect(card.textContent).toContain('Hartford, CT')
  // … distance/bearing derived from MY grid (snap.mygrid EN52 → their FN31) …
  const geo = card.querySelector('.recall-geo')
  expect(geo, 'no distance/bearing line').not.toBeNull()
  expect(geo!.textContent).toBe(`${distanceLabel('EN52', 'FN31')} · ${bearingLabel('EN52', 'FN31')}`)
  // … and the real prior-QSO history (the 2026-07-26 regression stays fixed).
  const list = card.querySelector('.recall-log-list')
  expect(list, 'no prior-contact list').not.toBeNull()
  expect(list!.textContent).toContain('14 Mar 26')
  expect(list!.textContent).toContain('02 Nov 25')

  // The compact variant is dead — nothing may render it.
  expect(document.querySelector('.recall-compact')).toBeNull()
  expect(document.querySelector('.recall-line')).toBeNull()
}

describe('PhoneCockpit — full recall card on call resolution', () => {
  it('shows photo, distance/bearing and history inside the log pane body', async () => {
    render(<PhoneCockpit snap={makeSnap('USB')} theme="dark" onWorkSpot={() => {}} spots={[]} />)
    await act(async () => {
      await Promise.resolve()
    })
    assertFullCard(await resolveCall())
  })
})

describe('CwCockpit — full recall card on call resolution', () => {
  it('shows photo, distance/bearing and history inside the log pane body', async () => {
    render(<CwCockpit snap={makeSnap('CW')} theme="dark" onWorkSpot={() => {}} spots={[]} />)
    // Let the mount-time getSettings / cwDecode / previewCw promises land.
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })
    assertFullCard(await resolveCall())
  })
})
