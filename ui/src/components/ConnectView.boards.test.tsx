// @vitest-environment jsdom
//
// CONNECT'S SPOTS, POTA/SOTA AND NEEDED BOXES ARE THE BOARDS THEMSELVES (plan piece H8; Needed 2026-10-07).
//
// The REAL ConnectView, the REAL SpotsPanel and the REAL PotaSotaView are mounted; only the backend
// is stubbed. A stubbed box would prove that a prop reaches it, and nothing about what is on screen:
// that the rows are the board's rows, that its filters are the board's filters and that they are
// the box's own copy, and that a screen with no board to lend says so rather than drawing a list
// with no Work behind it. The Work paths themselves are proven where they live (App and the
// dashboard window own the handlers the boxes are lent) — see App.connectBoards.test.tsx.
//
// jsdom does not lay out. The widths a box can be (200 to 720 px) are measured in a real browser;
// what IS here is the width class a box stamps from its own width, and the thresholds.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { AppSnapshot, NeedAlert, OtaSpot, SpotRow } from '../types'
import { t } from '../i18n'

const api = vi.hoisted(() => ({
  getOtaSpots: vi.fn(async (_program: string, _cached?: boolean): Promise<unknown[]> => []),
}))
vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  getBandOutlook: vi.fn(async () => ({ bands: [], asOf: 0 })),
  getGettingOut: vi.fn(async () => null),
  getPathOutlook: vi.fn(async () => null),
  getSpaceWxScales: vi.fn(async () => ({ scales: null, alerts: [] })),
  getKc2gMuf: vi.fn(async () => []),
  getXrayNow: vi.fn(async () => null),
  getDxpedWindows: vi.fn(async () => []),
  getAurora: vi.fn(async () => null),
  getDeclination: vi.fn(async () => null),
  getPca: vi.fn(async () => null),
  getSatellites: vi.fn(async () => null),
  getLogStats: vi.fn(async () => null),
  getOtaMapSpots: vi.fn(async () => []),
  getContests: vi.fn(async () => []),
  getOtaSpots: api.getOtaSpots,
  getActivation: vi.fn(async () => ({ program: null, reference: null, qsoCount: 0 })),
  parksCount: vi.fn(async () => 0),
  huntedParksCount: vi.fn(async () => 0),
  setHuntTarget: vi.fn(async () => null),
}))

import { ConnectView } from './ConnectView'
import { SpotsPanel } from './SpotsPanel'
import { PotaSotaView } from './PotaSotaView'
import { classifyBoxFit } from './connect/SpotsBox'
import { DEFAULT_SLOTS, type PaneId, type SlotId } from '../features/connectConfig'
import type { NeededBoard, OtaBoard, SpotsFeed } from './connect/paneContext'
import { pastTheSwitch } from './ConnectView.testkit'

// THE BUDGET (2026-10-09). The slowest case here, "is the Spots board in the slot: its rows, every mode on…", takes
// 0.40 s and 0.37 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const spot = (call: string, band: string, freqMhz: number, mode: string, submode: string | null = null): SpotRow =>
  ({
    call, entity: 'Somewhere', zone: 5, state: null, band, freqMhz, mode, submode,
    spotter: 'W3LPL', corroborators: [], ageSecs: 30, comment: '', licensed: true, spotterLocal: true,
  }) as SpotRow

// Four modes on three bands: the Phone pane of this board would show ONE of these (voice, on the
// radio's band); the view — and so the box — shows all four.
const ROWS: SpotRow[] = [
  spot('K1CW', '20m', 14.025, 'CW', 'CW'),
  spot('W2SSB', '40m', 7.2, 'Phone'),
  spot('JA1FT', '15m', 21.074, 'Digital', 'FT8'),
  spot('DL1RY', '20m', 14.083, 'Digital', 'RTTY'),
]

const ota = (activator: string, reference: string, over: Partial<OtaSpot> = {}): OtaSpot => ({
  program: 'POTA', reference, name: 'Test park', activator, freqKhz: 14_285, mode: 'SSB',
  spotter: null, comment: null, grid: null, newPark: false, bandOpen: false, huntedToday: false, ...over,
})

const baseProps = {
  myGrid: 'EN52',
  theme: 'dark' as const,
  stations: [],
  prop: null,
  selectedCall: null,
  onSelectCall: () => {},
  needByCall: new Map(),
  needAlerts: [],
  amp: null,
}

function board(): SpotsFeed['board'] {
  return { bandPlan: [], selectedCall: null, myGrid: 'EN52', onSelect: vi.fn(), onWork: vi.fn(), needAlerts: [] }
}
// Two needs in two modes on two bands: the box opens, as the Needed view does, on every mode.
const need = (call: string, band: string, freqMhz: number, mode: string): NeedAlert =>
  ({
    call, entity: 'Somewhere', band, zone: 5, tags: ['NewBand'], priority: 50,
    headline: `New band — Somewhere ${band}`, mode, freqMhz,
  }) as NeedAlert
const NEEDS: NeedAlert[] = [need('K1CW', '20m', 14.025, 'CW'), need('W2SSB', '40m', 7.2, 'SSB')]
function neededBoard(): NeededBoard {
  return { alerts: NEEDS, bandPlan: [], selectedCall: null, myGrid: 'EN52', onQsy: vi.fn(), onSelect: vi.fn(), onWork: vi.fn() }
}
const needRows = (el: HTMLElement) => [...el.querySelectorAll<HTMLElement>('.np-row')]
const needCallsIn = (el: HTMLElement) =>
  needRows(el).flatMap((r) => NEEDS.filter((n) => r.querySelector('.np-call')?.textContent?.includes(n.call)).map((n) => n.call))

function otaBoard(): OtaBoard {
  return { snap: { hunt: null, radio: { dialMhz: 14.285 }, logTick: 1 } as unknown as AppSnapshot, onHunt: vi.fn(), onSnap: vi.fn() }
}

/** Put panes into slots the way a stored layout does (the one-time Chase move already done). */
function seed(slots: Partial<Record<SlotId, PaneId>>) {
  localStorage.setItem('nexus.connect.chaseDefault.v1', '1')
  localStorage.setItem('nexus.connect.config', JSON.stringify({ slots: { ...DEFAULT_SLOTS, ...slots }, overlays: {} }))
}

async function mount(extra: Partial<Parameters<typeof ConnectView>[0]> = {}) {
  let r!: ReturnType<typeof render>
  await act(async () => {
    r = render(<ConnectView {...baseProps} {...extra} />)
  })
  return r
}

const frameOf = (c: HTMLElement, pane: string) => c.querySelector(`.pane-frame[data-pane="${pane}"]`) as HTMLElement | null
const callsIn = (el: HTMLElement) => [...el.querySelectorAll('.sp-row .np-call')].map((n) => n.textContent)

beforeEach(() => {
  localStorage.clear()
  pastTheSwitch()
  sessionStorage.clear()
  api.getOtaSpots.mockReset()
  api.getOtaSpots.mockResolvedValue([])
  window.history.replaceState(null, '', '/')
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(() => {
  cleanup()
  window.history.replaceState(null, '', '/')
})

describe('the Spots box', () => {
  it('is the Spots board in the slot: its rows, every mode on every band, its own filter bar', async () => {
    seed({ left1: 'spots' })
    const { container } = await mount({ spotsFeed: { rows: ROWS, board: board() } })
    const box = frameOf(container, 'spots')
    expect(box, 'the Spots box is in the slot it was put in').not.toBeNull()
    expect(box!.getAttribute('data-slot')).toBe('left1')
    expect(box!.querySelector('.cn-spots .np-board'), 'the board, hosted as a pane').not.toBeNull()
    // The view's opening filters — every mode, every band — not the Phone pane's (voice, this band).
    expect(callsIn(box!).sort()).toEqual(['DL1RY', 'JA1FT', 'K1CW', 'W2SSB'])
    // The board's own head: its count, search and Filter toggle; no heading of its own (the frame's
    // head names it) and no pop-out (a box is not a view).
    expect(within(box!).getByRole('searchbox', { name: t('spots.search.label') })).toBeTruthy()
    expect(within(box!).getByRole('button', { name: new RegExp(t('spots.filter.toggle.idle')) })).toBeTruthy()
    expect(box!.querySelector('h2')).toBeNull()
    expect(within(box!).queryByRole('button', { name: t('spots.popOut.label') })).toBeNull()
  })

  it("keeps its own copy of the board's filters: a mode chip in the box leaves the Spots view as it was", async () => {
    seed({ left1: 'spots' })
    const { container } = await mount({ spotsFeed: { rows: ROWS, board: board() } })
    const box = frameOf(container, 'spots')!
    fireEvent.click(within(box).getByRole('button', { name: new RegExp(t('spots.filter.toggle.idle')) }))
    fireEvent.click(within(box).getByRole('button', { name: 'CW' }))
    expect(callsIn(box).sort(), 'the chip hid CW in the box').toEqual(['DL1RY', 'JA1FT', 'W2SSB'])
    // The box keeps the view's RULE (a hidden set: a mode that turns up later shows) in its OWN copy.
    expect(JSON.parse(sessionStorage.getItem('nexus.spots.hiddenModes.connect') ?? 'null')).toEqual(['CW'])
    expect(sessionStorage.getItem('nexus.spots.hiddenModes'), "the view's own copy was never written").toBeNull()
    cleanup()
    // …and the Spots view, opened after, still lists CW.
    const view = render(<SpotsPanel spots={ROWS} {...board()} />)
    expect(callsIn(view.container).sort()).toEqual(['DL1RY', 'JA1FT', 'K1CW', 'W2SSB'])
  })

  it('on a screen with no Spots board to lend it says so in one line, and draws no list and no Work', async () => {
    seed({ left1: 'spots' })
    const { container } = await mount()
    const box = frameOf(container, 'spots')!
    expect(box.querySelector('.pane-basic')?.textContent).toBe(t('connect.pane.spots.basic'))
    expect(box.querySelector('.np-board')).toBeNull()
    expect(box.querySelectorAll('.sp-row').length).toBe(0)
  })

  it('stamps its column set from its OWN width, and keeps it while hidden', async () => {
    seed({ left1: 'spots' })
    const width = { px: 200 }
    // jsdom defines clientWidth on Element.prototype (always 0); shadow it on HTMLElement for the
    // box alone, and take the shadow away again afterwards.
    Object.defineProperty(HTMLElement.prototype, 'clientWidth', {
      configurable: true,
      get(this: HTMLElement) {
        return this.classList.contains('cn-spots') ? width.px : 0
      },
    })
    try {
      const { container } = await mount({ spotsFeed: { rows: ROWS, board: board() } })
      expect(container.querySelector('.cn-spots')!.getAttribute('data-fit')).toBe('s')
      cleanup()
      width.px = 720
      const again = await mount({ spotsFeed: { rows: ROWS, board: board() } })
      expect(again.container.querySelector('.cn-spots')!.getAttribute('data-fit')).toBe('l')
      cleanup()
      width.px = 0 // a hidden (0-wide) box claims no class rather than the narrowest one
      const hidden = await mount({ spotsFeed: { rows: ROWS, board: board() } })
      expect(hidden.container.querySelector('.cn-spots')!.hasAttribute('data-fit')).toBe(false)
    } finally {
      delete (HTMLElement.prototype as { clientWidth?: number }).clientWidth
    }
  })

  it('the column sets split where the rail widths need them', () => {
    // A Connect rail runs 200–720 px (features/connectRails RAIL_MIN / RAIL_MAX). The middle set starts
    // where every one of its headings fits whole in all five languages (SpotsBox `classifyBoxFit`).
    expect([200, 529, 530, 639, 640, 720].map(classifyBoxFit)).toEqual(['s', 's', 'm', 'm', 'l', 'l'])
  })
})

describe('the POTA/SOTA box', () => {
  it("is the POTA/SOTA board's list with its HUNT, and none of the view's chrome", async () => {
    api.getOtaSpots.mockResolvedValue([ota('K1ABC', 'US-0001'), ota('W9XYZ', 'US-0002')])
    seed({ right1: 'pota' })
    const { container } = await mount({ otaBoard: otaBoard() })
    const box = frameOf(container, 'pota')!
    expect(box, 'the POTA/SOTA box is in the slot it was put in').not.toBeNull()
    await waitFor(() => expect(box.querySelectorAll('.pota-spot-v2').length).toBe(2))
    expect(within(box).getByRole('button', { name: t('ota.hunt.button.aria', { call: 'K1ABC' }) })).toBeTruthy()
    // Its program tabs and Hide worked are the board's own.
    expect(within(box).getByRole('tab', { name: 'SOTA' })).toBeTruthy()
    // Left to the view: the heading, the pop-out, the activation strip, the park directory, the source line.
    expect(box.querySelector('h2')).toBeNull()
    expect(within(box).queryByRole('button', { name: t('ota.popOut.label') })).toBeNull()
    expect(box.querySelector('.pota-activation')).toBeNull()
    expect(box.querySelector('.pota-parklist')).toBeNull()
    expect(box.querySelector('.pota-source-hint')).toBeNull()
  })

  it('opens its band, mode and sort rows on its Filter button', async () => {
    api.getOtaSpots.mockResolvedValue([ota('K1ABC', 'US-0001')])
    seed({ right1: 'pota' })
    const { container } = await mount({ otaBoard: otaBoard() })
    const box = frameOf(container, 'pota')!
    await waitFor(() => expect(box.querySelectorAll('.pota-spot-v2').length).toBe(1))
    expect(within(box).queryByRole('group', { name: t('ota.filter.band.aria') })).toBeNull()
    const filter = within(box).getByRole('button', { name: t('spots.filter.toggle.idle') })
    expect(filter.getAttribute('aria-expanded')).toBe('false')
    fireEvent.click(filter)
    expect(within(box).getByRole('group', { name: t('ota.filter.band.aria') })).toBeTruthy()
    expect(within(box).getByRole('group', { name: t('ota.sort.aria') })).toBeTruthy()
  })

  it('keeps its own filter record: SOTA picked in the box leaves the POTA/SOTA view on POTA', async () => {
    seed({ right1: 'pota' })
    const { container } = await mount({ otaBoard: otaBoard() })
    const box = frameOf(container, 'pota')!
    fireEvent.click(within(box).getByRole('tab', { name: 'SOTA' }))
    expect(localStorage.getItem('nexus.connect.ota.program')).toBe('SOTA')
    expect(localStorage.getItem('nexus.ota.program'), "the view's record was never written").toBeNull()
    cleanup()
    render(<PotaSotaView snap={otaBoard().snap} />)
    expect(screen.getByRole('tab', { name: 'POTA' }).getAttribute('aria-selected')).toBe('true')
  })

  it('on a screen with no POTA/SOTA board to lend it says so in one line, and offers no HUNT', async () => {
    seed({ right1: 'pota' })
    const { container } = await mount()
    const box = frameOf(container, 'pota')!
    expect(box.querySelector('.pane-basic')?.textContent).toBe(t('connect.pane.pota.basic'))
    expect(box.querySelector('.pota-hunt-btn')).toBeNull()
    expect(api.getOtaSpots, 'with nothing to lend, the box does not fetch the feed').not.toHaveBeenCalled()
  })
})

describe('the Needed box', () => {
  it("is the Needed board in the slot: its rows, every mode, its own filter bar, none of the view's chrome", async () => {
    seed({ left1: 'needed' })
    const { container } = await mount({ neededBoard: neededBoard() })
    const box = frameOf(container, 'needed')
    expect(box, 'the Needed box is in the slot it was put in').not.toBeNull()
    expect(box!.getAttribute('data-slot')).toBe('left1')
    expect(box!.querySelector('.cn-needed .np-board'), 'the board, hosted as a pane').not.toBeNull()
    expect(needCallsIn(box!).sort()).toEqual(['K1CW', 'W2SSB'])
    expect(within(box!).getByRole('button', { name: new RegExp(t('needed.filter.toggle.idle')) })).toBeTruthy()
    // The frame's head names it, and a box is not a view: no heading, no pop-out.
    expect(box!.querySelector('h2')).toBeNull()
    expect(within(box!).queryByRole('button', { name: new RegExp(t('needed.popOut.label')) })).toBeNull()
  })

  it("keeps its own copy of the board's filters: a mode chip in the box leaves the Needed view as it was", async () => {
    seed({ left1: 'needed' })
    const { container } = await mount({ neededBoard: neededBoard() })
    const box = frameOf(container, 'needed')!
    fireEvent.click(within(box).getByRole('button', { name: new RegExp(t('needed.filter.toggle.idle')) }))
    fireEvent.click(within(box).getByRole('button', { name: 'CW' }))
    // A mode chip turns its mode off (the board opens with every mode on).
    expect(needCallsIn(box), 'the chip narrowed the box').toEqual(['W2SSB'])
    expect(localStorage.getItem('nexus.connect.neededFilters'), "the box's own record").not.toBeNull()
    expect(localStorage.getItem('neededFilters'), "the view's own record was never written").toBeNull()
  })

  it("works a row through the board's own handlers: the select, then the Work", async () => {
    seed({ left1: 'needed' })
    const lent = neededBoard()
    const { container } = await mount({ neededBoard: lent })
    const box = frameOf(container, 'needed')!
    const row = needRows(box).find((r) => r.querySelector('.np-call')?.textContent?.includes('K1CW'))!
    fireEvent.click(row)
    expect(lent.onSelect).toHaveBeenCalledWith('K1CW')
    expect(lent.onWork).toHaveBeenCalledWith(NEEDS[0])
  })

  it('on a screen with no Needed board to lend it says so in one line, and draws no list and no Work', async () => {
    seed({ left1: 'needed' })
    const { container } = await mount()
    const box = frameOf(container, 'needed')!
    expect(box.querySelector('.pane-basic')?.textContent).toBe(t('connect.pane.needed.basic'))
    expect(box.querySelector('.np-board')).toBeNull()
  })
})

describe('the default layout does not change', () => {
  it('no board is in a default slot: each is one pick away in every slot', async () => {
    for (const p of ['spots', 'pota', 'needed']) expect(Object.values(DEFAULT_SLOTS)).not.toContain(p)
    const { container } = await mount({ spotsFeed: { rows: ROWS, board: board() }, otaBoard: otaBoard(), neededBoard: neededBoard() })
    for (const pick of container.querySelectorAll('select.pane-pick')) {
      const values = [...(pick as HTMLSelectElement).options].map((o) => o.value)
      expect(values).toEqual(expect.arrayContaining(['spots', 'pota', 'needed']))
    }
    for (const p of ['spots', 'pota', 'needed']) expect(frameOf(container, p)).toBeNull()
  })
})
