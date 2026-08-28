// @vitest-environment jsdom
//
// THE CALLSIGN CARD IS REACHABLE FROM THE FT COCKPIT (#168).
//
// The bug was never that the card did not work — `RecallPanel` has shown a photo, a
// distance/bearing line, the prior-contact list and the new-one badges in the CW and Phone
// log strips since 2026-07-31. The bug was that it had exactly ONE caller (LogEntry), and
// Operate hosts no LogEntry, so a roster click in the cockpit an FT8 operator actually sits
// in produced nothing but an armed Spot button. So this file asserts REACHABILITY FROM THIS
// SURFACE, not the card's own rendering (RecallPanel.test.tsx owns that): render the real
// OperateCockpit, hand it a selected call the way App does, and look for the card.
//
// The pre-fix state is pinned as the negative half of the same claim — with no call
// selected there is no card in the document at all, which is both what shipped before and
// the "costs the rail nothing until you click" property the placement rests on. A file that
// only asserted presence would go green on a card that is always there.
//
// BOTH LAYOUTS, every time. Operate renders two different lower regions (Classic's
// three-column grid and Roster's rail), and one of the two silently missing the card is
// exactly the drift OperateCockpit.structure.test.tsx exists to catch elsewhere.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor, within } from '@testing-library/react'
import { OperateCockpit } from './OperateCockpit'
import { RecallPanel } from './RecallPanel'
import { distanceLabel, bearingLabel } from '../grid'
import type { AppSnapshot, LoggedQso, QrzLookup } from '../types'
import type { OperatePanelId, PanelLayoutApi, PanelState } from '../features/panelState'

const PHOTO = 'https://cdn-xfer.qrz.com/x/w1abc/photo.jpg'
const MY_GRID = 'EN61'
/** What the CALLBOOK says — finer than the decoded square, and it must win. */
const BOOK_GRID = 'FN31'
/** What the DECODE said. The fallback when there is no callbook answer at all. */
const DECODED_GRID = 'FN42'

const resolved: QrzLookup = {
  call: 'W1ABC',
  name: 'Alice Example',
  nickname: null,
  qth: 'Hartford, CT',
  grid: BOOK_GRID,
  state: 'CT',
  country: 'United States',
  dxcc: 291,
  cqZone: 5,
  ituZone: 8,
  image: PHOTO,
}

// Two prior contacts with W1ABC, on bands that are NOT the live one (20 m) — so the
// entity is worked, this band is not, and the New band-slot badge is a real derivation
// from this log rather than a flag handed in. One is confirmed, so the ✓ badge has
// something to count; the newest carries the operator's private note.
const priorQsos = [
  {
    call: 'W1ABC',
    country: 'United States',
    grid: BOOK_GRID,
    band: '40m',
    freqMhz: 7.074,
    mode: 'FT8',
    rstSent: '-08',
    rstRcvd: '-12',
    notes: 'runs 5 W to an attic dipole',
    whenUnix: Date.UTC(2026, 2, 14) / 1000, // 14 Mar 26
    confirmed: true,
  },
  {
    call: 'W1ABC',
    country: 'United States',
    grid: BOOK_GRID,
    band: '15m',
    freqMhz: 21.074,
    mode: 'FT4',
    rstSent: '-15',
    rstRcvd: '-11',
    whenUnix: Date.UTC(2025, 10, 2) / 1000, // 02 Nov 25
    confirmed: false,
  },
] as unknown as LoggedQso[]

const qrzLookup = vi.fn(async () => resolved)
const getLog = vi.fn(async () => priorQsos)

// The structure suite's api surface for this cockpit, plus the three commands the card
// itself needs. vi.mock replaces the whole module, so anything omitted is `undefined` at
// the call site — `resolveEntity` learned that the hard way in CockpitRecall.test.tsx.
vi.mock('../api', () => ({
  getLog: (...a: unknown[]) => getLog(...(a as [])),
  qrzLookup: (...a: unknown[]) => qrzLookup(...(a as [])),
  resolveEntity: vi.fn(async () => 'United States'),
  getSettings: vi.fn(() => Promise.resolve({})),
  setSettings: vi.fn(async () => null),
  openPanelWindow: vi.fn(async () => null),
  notifyErase: vi.fn(async () => null),
  pointRotatorAtCall: vi.fn(async () => null),
  redecode: vi.fn(async () => null),
  startCq: vi.fn(async () => null),
  startQsoRecording: vi.fn(async () => null),
  stopQsoRecording: vi.fn(async () => null),
  setSkipTx1: vi.fn(async () => null),
  getDeclination: vi.fn(async () => null),
  getSatTrackStatus: vi.fn(async () => null),
  readRotator: vi.fn(async () => null),
  stopRotator: vi.fn(async () => null),
  stopSatTrack: vi.fn(async () => null),
  openQrzPage: vi.fn(async () => null),
  postSpot: vi.fn(async () => null),
  setFrequency: vi.fn(async () => null),
  setRit: vi.fn(async () => null),
  setXit: vi.fn(async () => null),
  setVfo: vi.fn(async () => null),
  getSpectrumRow: vi.fn(async () => null),
  setDecodeDepth: vi.fn(async () => null),
  atuTune: vi.fn(async () => null),
  setMsk144Period: vi.fn(async () => null),
}))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall-stub" /> }))
vi.mock('./OperateDecodes', async (importOriginal) => {
  const real = await importOriginal<typeof import('./OperateDecodes')>()
  return { ...real, OperateDecodes: () => <div data-testid="od-pane" /> }
})

function makeSnap(dxcall: string | null = null): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    mygrid: MY_GRID,
    // The station is on screen because we decoded it, and the frame carried a square.
    stations: [
      {
        call: 'W1ABC',
        grid: DECODED_GRID,
        snr: -7,
        lastHeardSlot: 0,
        heardCount: 3,
        presence: 'live',
        worked: true,
        country: 'United States',
      },
    ],
    recentDecodes: [],
    conversations: [],
    highlights: [],
    harqRescues: 0,
    clearTick: 0,
    qso: dxcall ? ({ dxcall } as unknown as AppSnapshot['qso']) : null,
    link: { tier: 'FT8' },
    radio: {
      dialMhz: 14.074,
      band: '20m',
      sideband: 'USB',
      slot: 0,
      source: 'native',
      sourceLabel: 'Native',
      nextSlotMs: 5000,
      rxOffsetHz: 1500,
      txOffsetHz: 1500,
      txLevel: 0.5,
      txEven: true,
      txCycleAuto: true,
      txEnabled: false,
      txAllowed: true,
      transmitting: false,
      tuning: false,
      atu: true,
      qsoRecording: false,
      catOk: true,
      splitTxMhz: null,
    },
  } as unknown as AppSnapshot
}

function panelsApi(): PanelLayoutApi<OperatePanelId> {
  const state: Partial<Record<OperatePanelId, PanelState>> = {}
  return {
    layout: { v: 1, state, share: {} },
    stateOf: (id) => state[id] ?? 'docked',
    setPanelState: vi.fn(),
    shareOf: () => 1,
    setShare: vi.fn(),
    setShares: vi.fn(),
    undo: vi.fn(),
    canUndo: false,
    undoRemoves: [],
    reset: vi.fn(),
  }
}

function renderCockpit(
  selectedCall: string | null,
  layoutMode: 'classic' | 'roster' = 'classic',
  dxcall: string | null = null,
) {
  const noop = () => {}
  return render(
    <OperateCockpit
      snap={makeSnap(dxcall)}
      theme="dark"
      tier="FT8"
      onTierChange={noop}
      bandPlan={[]}
      onSetFrequency={noop}
      onSourceChange={noop}
      onTune={noop}
      onCall={noop}
      onSetTxLevel={noop}
      onSetMode={noop}
      onSetTxEven={noop}
      onSetTxCycleAuto={noop}
      onResend={noop}
      onFreetext={noop}
      onLog={noop}
      onOverrideTx={noop}
      onHaltTx={noop}
      roster={<div data-testid="stations-roster" />}
      needByCall={new Map()}
      selectedCall={selectedCall}
      onSelect={noop}
      layoutMode={layoutMode}
      onLayoutMode={noop}
      panels={panelsApi()}
      active={false}
    />,
  )
}

/** The card, once the debounced callbook answer has landed. */
async function card(): Promise<HTMLElement> {
  await waitFor(() => expect(document.querySelector('.recall-card')).not.toBeNull())
  return document.querySelector('.recall-card') as HTMLElement
}

beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
  qrzLookup.mockClear()
  qrzLookup.mockResolvedValue(resolved)
  getLog.mockClear()
})
afterEach(cleanup)

describe('the FT cockpit shows the callsign card for the selected station (#168)', () => {
  it('renders NOTHING until a call is selected — the shipped state, and the placement’s cost claim', async () => {
    renderCockpit(null)
    // Give the effects the same room the positive case gets, so this is a real negative
    // and not a race the assertion won because it ran first.
    await new Promise((r) => setTimeout(r, 600))
    expect(document.querySelector('.recall-card')).toBeNull()
    expect(qrzLookup, 'a callbook request with nothing selected').not.toHaveBeenCalled()
  })

  it.each(['classic', 'roster'] as const)(
    '%s: the card rides the side rail’s scroller, never a shell-level sibling',
    async (layoutMode) => {
      const { container } = renderCockpit('W1ABC', layoutMode)
      const c = await card()

      // THE LAYOUT CONTRACT, as an assertion. `.cockpit-side` is the one container in this
      // cockpit that is already an interposed scroller; a card hung off the shell would sit
      // above `.cockpit-lower` (flex:1, min-height:0) and crush the operating region — the
      // pre-overhaul failure this cockpit's four-child shell exists to prevent.
      const rail = container.querySelector('aside.cockpit-side')
      expect(rail, 'no side rail rendered').not.toBeNull()
      expect(rail!.contains(c), 'the card is not in the side rail').toBe(true)
      const shell = container.querySelector('main.layout.single.operate-cockpit')!
      expect(Array.from(shell.children).includes(c), 'the card is a shell-level sibling').toBe(false)
      expect(c.closest('.cockpit-qso'), 'the card is inside the always-visible TX strip').toBeNull()
    },
  )

  it('shows the set the operator was promised: history, badges, geography and the note', async () => {
    renderCockpit('W1ABC')
    // The card is up on the history alone (the decoded square is already on screen); this
    // waits for the DEBOUNCED callbook answer to land on top of it.
    await waitFor(() =>
      expect(document.querySelector('.recall-card')?.textContent).toContain('Alice Example'),
    )
    const c = await card()

    // Identity + the callbook photo (an <img> that hides itself on error, initials beneath).
    expect(c.textContent).toContain('Hartford, CT')
    expect((c.querySelector('.recall-avatar-img') as HTMLImageElement | null)?.src).toBe(PHOTO)

    // Previous contacts as a REAL LIST with band and mode — never a "worked 3×" count
    // (the 2026-07-26 regression report, and the reason RecallPanel keeps a list at all).
    const list = c.querySelector('.recall-log-list')!
    expect(list, 'no prior-contact list').not.toBeNull()
    expect(within(list as HTMLElement).getAllByRole('listitem')).toHaveLength(2)
    expect(list.textContent).toContain('14 Mar 26')
    expect(list.textContent).toContain('40m FT8')
    expect(list.textContent).toContain('02 Nov 25')
    expect(list.textContent).toContain('15m FT4')

    // What is unconfirmed: the ✓ badge counts the confirmed prior QSOs against the total.
    const ok = c.querySelector('.recall-badge.ok')
    expect(ok, 'no confirmed badge').not.toBeNull()
    expect(ok!.getAttribute('title')).toBe('1 of 2 prior QSOs confirmed')

    // The new-one flag, DERIVED from this log: the entity is worked on 40 m and 15 m, the
    // rig is on 20 m, so this band is a new slot for it.
    const need = c.querySelector('.recall-badge.need')
    expect(need, 'no new-one badge').not.toBeNull()
    expect(need!.textContent).toContain('New band-slot')

    // Distance + bearing from the operator's own square to the CALLBOOK's — the finer of
    // the two positions wins over the decoded one.
    expect(c.querySelector('.recall-geo')!.textContent).toBe(
      `${distanceLabel(MY_GRID, BOOK_GRID)} · ${bearingLabel(MY_GRID, BOOK_GRID)}`,
    )

    // The operator's own private note on this station.
    expect(c.querySelector('.recall-note')!.textContent).toContain('runs 5 W to an attic dipole')
  })

  it('degrades to history + the DECODED square when the callbook cannot answer', async () => {
    // No QRZ subscription, no credentials, a call the callbook does not know — all ordinary,
    // and none of them may cost the operator the card. What survives is what the app knows
    // by itself: who they are, what we have worked, and where the decode said they were.
    qrzLookup.mockRejectedValue(new Error('no callbook credentials configured'))
    renderCockpit('W1ABC')
    const c = await card()

    expect(c.textContent).toContain('W1ABC')
    expect(c.querySelector('.recall-log-list')!.textContent).toContain('40m FT8')
    expect(c.querySelector('.recall-avatar-img'), 'a photo with no callbook answer').toBeNull()
    expect(c.querySelector('.recall-geo')!.textContent).toBe(
      `${distanceLabel(MY_GRID, DECODED_GRID)} · ${bearingLabel(MY_GRID, DECODED_GRID)}`,
    )
    // …and it must not tell the operator to press a Lookup button. There isn't one in this
    // cockpit: that line is the log strip's instruction, and it is the whole of `hasLookup`.
    expect(c.textContent).not.toContain('press Lookup')
  })

  it('follows the WORKED station when nothing has been clicked', async () => {
    // Most of an FT8 session never selects a peer: a double-click on a decode starts a QSO
    // through `call_station_ctx` without going through the roster's select. Reading only
    // `selectedCall` left the card blank through every contact the operator actually ran —
    // which is the state the reporter was in.
    renderCockpit(null, 'classic', 'W1ABC')
    const c = await card()
    expect(c.textContent).toContain('W1ABC')
    expect(c.querySelector('.recall-log-list')!.textContent).toContain('40m FT8')
  })

  it('lets an explicit click WIN over the station being worked', async () => {
    // The other half, and the reason this is an ordered fallback rather than a merge: a
    // click means "show me this one", and it must not be yanked back to the sequencer's
    // station on the next snapshot poll while the operator is reading it.
    renderCockpit('K9XYZ', 'classic', 'W1ABC')
    await waitFor(() => expect(document.querySelector('.recall-card')?.textContent).toContain('K9XYZ'))
    expect(document.querySelector('.recall-card')!.textContent).not.toContain('W1ABC')
  })

  it('carries the rail bound HERE, and the default leaves every other host uncapped', async () => {
    // `.cockpit-recall` (cockpit-panes.css) caps the card at a share of the rail so it stops
    // taking the Stations roster down to ~2 rows at 1024×768. It is scoped by being APPLIED,
    // not by a descendant selector — that sheet's flat-selector guard refuses one — so the
    // scoping is only as good as the default staying off. Both halves are asserted: a flipped
    // default would silently cap the CW and Phone log cards, where the operator asked for the
    // FULL card back on 2026-07-31 and the pane body is already the scroller.
    renderCockpit('W1ABC')
    expect((await card()).classList.contains('cockpit-recall')).toBe(true)
    cleanup()

    const hist = { qsos: [], count: 0, workedBefore: false, dupeThisBand: false, lastUnix: null,
      confirmedCount: 0, bandsWorked: [], modesWorked: [] } as unknown as Parameters<typeof RecallPanel>[0]['hist']
    render(<RecallPanel call="W1ABC" hist={hist} />)
    const plain = document.querySelector('.recall-card')!
    expect(plain, 'the bare card did not render').not.toBeNull()
    expect(
      plain.classList.contains('cockpit-recall'),
      'the rail cap became the DEFAULT — every log-strip card is now capped and scrolls inside itself',
    ).toBe(false)
  })

  it('re-reads the logbook on each selection — the sequencer logs behind the operator', async () => {
    // The one deliberate difference from LogEntry, and it is not tidiness: Operate files a
    // contact the moment the exchange completes, with no click. A read-once card would tell
    // an operator they had never worked a station they worked ten minutes ago.
    const { rerender } = renderCockpit('W1ABC')
    await card()
    const first = getLog.mock.calls.length
    expect(first).toBeGreaterThan(0)
    rerender(<div />) // unmount path is irrelevant here; the count is what is asserted
    cleanup()
    renderCockpit('K9XYZ')
    await waitFor(() => expect(getLog.mock.calls.length).toBeGreaterThan(first))
  })
})
