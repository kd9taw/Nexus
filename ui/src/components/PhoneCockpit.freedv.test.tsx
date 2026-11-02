// @vitest-environment jsdom
//
// THE FREEDV PANE ON THE PHONE SCREEN — FreeDV's own app beside Nexus, receive only. The operator's
// pick: "A FreeDV pane (Recommended)": hidden until added; heard calls from FreeDV 2.4's broadcast;
// a click fills the call; nothing logs or keys by itself.
//
// Against the REAL panel record (`usePanelLayout(PHONE_PANELS)`, as App builds it) and the real ⊞
// menu, as the Spots and Needed panes are (PhoneCockpit.boards.test.tsx): the default is a property
// of the record, so a stub answering 'docked' for every id would prove nothing about who sees it.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, within } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import { PHONE_PANELS, panelStorageKey, usePanelLayout } from '../features/panelState'
import type { AppSnapshot, FreedvState } from '../types'

vi.setConfig({ testTimeout: 15_000 })

const api = vi.hoisted(() => ({
  getSettings: vi.fn(async () => ({})),
  setPtt: vi.fn(async () => {}),
  setRfPower: vi.fn(async () => {}),
  setMicGain: vi.fn(async () => {}),
  setNrLevel: vi.fn(async () => {}),
  setAgc: vi.fn(async () => ({})),
  setScopeSpan: vi.fn(async () => ({})),
  setScopeRef: vi.fn(async () => {}),
  setFlexPanSpan: vi.fn(async () => ({})),
  setFlexPanRef: vi.fn(async () => ({})),
  startQsoRecording: vi.fn(async () => ({})),
  stopQsoRecording: vi.fn(async () => ({})),
  setTune: vi.fn(async () => ({})),
  haltTx: vi.fn(async () => ({})),
  setTxEnabled: vi.fn(async () => ({})),
  setFrequency: vi.fn(async () => ({})),
  setSplit: vi.fn(async () => ({})),
  setRigFunc: vi.fn(async () => ({})),
  setSidebandOverride: vi.fn(async () => ({})),
  setFilterWidth: vi.fn(async () => ({})),
  openPanelWindow: vi.fn(async () => {}),
  openQrzPage: vi.fn(async () => {}),
  readRotator: vi.fn(async () => null),
  pointRotator: vi.fn(async () => {}),
  stopVoice: vi.fn(async () => ({})),
  playVoiceMessage: vi.fn(async () => ({})),
}))
vi.mock('../api', () => api)
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

// The header hosts the ⊞ menu, which the cockpit hands it as `actions`; the rest is stubbed.
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: ({ actions }: { actions?: unknown }) => (
    <header className="cockpit-header">{actions as never}</header>
  ),
}))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="vk-stub" /> }))
const logStrip = vi.hoisted(() => ({ work: [] as Array<{ call: string } | null | undefined> }))
vi.mock('./LogEntry', () => ({
  LogEntry: ({ pendingWork }: { pendingWork?: { call: string } | null }) => {
    logStrip.work.push(pendingWork)
    return <div data-testid="log-stub">{pendingWork?.call ?? ''}</div>
  },
}))
vi.mock('../remote-web/RemoteRecall', () => ({ RemoteRecallEntry: () => <div data-testid="recall-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

/** jsdom has no ResizeObserver; the region and its dividers each register one. */
beforeEach(() => {
  const live = new Set<() => void>()
  globalThis.ResizeObserver = class {
    cb: () => void
    constructor(cb: () => void) {
      this.cb = cb
      live.add(cb)
    }
    observe() {}
    disconnect() {
      live.delete(this.cb)
    }
    unobserve() {}
  } as unknown as typeof ResizeObserver
  Element.prototype.setPointerCapture = () => {}
  Element.prototype.scrollIntoView = () => {}
})
afterEach(() => {
  cleanup()
  localStorage.clear()
  sessionStorage.clear()
  for (const f of Object.values(api)) f.mockClear()
})

function makeSnap(band = '20m', dialMhz = 14.25): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    radio: {
      dialMhz,
      band,
      catOk: true,
      sideband: 'USB',
      sidebandOverride: null,
      rigMode: 'USB',
      transmitting: false,
      txEnabled: true,
      txAllowed: true,
      qsoRecording: false,
      rfPower: null,
      micGain: null,
      nrLevel: null,
      agc: null,
      nb: null,
      nr: null,
      notch: null,
      comp: null,
      vox: null,
      filterWidthHz: null,
      splitTxMhz: null,
      smeterDb: null,
      rxLevel: 0,
      phoneSegLo: null,
      phoneSegHi: null,
    },
  } as unknown as AppSnapshot
}

const FREEDV: FreedvState = {
  logPort: 2239,
  logError: null,
  aliveUnix: null,
  lastContact: 'W5FDV · 14.236 MHz',
  lastContactUnix: null,
  notice: null,
  heardPort: 7177,
  heardError: null,
  heard: [{ call: 'VK5DGR', mode: 'RADEV1', snr: 4, frequencyHz: 14_236_000, heardUnix: 0 }],
}

/** The cockpit with the real panel record, as App passes it on the desktop (`freedvPane`). */
function Live({ offered = true }: { offered?: boolean }) {
  const panels = usePanelLayout(PHONE_PANELS)
  return (
    <PhoneCockpit
      snap={{ ...makeSnap(), freedv: FREEDV } as AppSnapshot}
      theme="dark"
      onWorkSpot={() => {}}
      spots={[]}
      panels={panels}
      freedvPane={offered}
    />
  )
}

const PHONE_KEY = panelStorageKey('phone')
const record = () => JSON.parse(localStorage.getItem(PHONE_KEY) ?? '{"state":{},"share":{}}')
function openMenu() {
  const btn = screen.getByRole('button', { name: /Panels/ })
  if (btn.getAttribute('aria-expanded') !== 'true') fireEvent.click(btn)
  return screen.getByRole('group', { name: /panels on this screen/i })
}
const box = () => within(openMenu()).getByRole('checkbox', { name: /^FreeDV$/ }) as HTMLInputElement
const pane = () => document.querySelector<HTMLElement>('[data-pane="freedv"]')

describe('the FreeDV pane', () => {
  beforeEach(() => {
    logStrip.work.length = 0
  })

  it('ships hidden, and ⊞ Panels offers it unticked, with no warning', () => {
    render(<Live />)
    expect(pane(), 'the FreeDV pane appeared on a stock screen').toBeNull()
    expect(box().checked).toBe(false)
    expect(box().getAttribute('aria-describedby'), 'its hide ends nothing').toBeNull()
    const btn = screen.getByRole('button', { name: /Panels/ })
    expect(btn.textContent, 'a pane that ships hidden is not one the operator hid').toBe('⊞ Panels')
  })

  it('a tick docks it as a fill pane with its own ✕, stored in the Phone record', () => {
    render(<Live />)
    fireEvent.click(box())
    const frame = pane()
    expect(frame, 'ticking FreeDV did not dock it').not.toBeNull()
    expect(frame!.dataset.fit).toBe('fill')
    expect(within(frame!).getByText('Last contact: W5FDV · 14.236 MHz')).toBeTruthy()
    expect(within(frame!).getByRole('button', { name: 'Hide FreeDV' })).toBeTruthy()
    expect(record().state.freedv).toBe('docked')
  })

  it('a click on a heard call fills the log strip’s call, and nothing transmits, tunes or logs', () => {
    render(<Live />)
    fireEvent.click(box())
    expect(screen.getByTestId('log-stub').textContent).toBe('')
    fireEvent.click(within(pane()!).getByRole('button', { name: 'VK5DGR' }))
    expect(screen.getByTestId('log-stub').textContent).toBe('VK5DGR')
    for (const name of ['setPtt', 'setTune', 'setTxEnabled', 'setFrequency', 'haltTx', 'playVoiceMessage'] as const) {
      expect(api[name], name).not.toHaveBeenCalled()
    }
  })

  it('where the host does not offer it (the Remote), a tick shows nothing', () => {
    render(<Live offered={false} />)
    fireEvent.click(box())
    expect(pane()).toBeNull()
  })
})
