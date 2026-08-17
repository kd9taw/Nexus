// @vitest-environment jsdom
//
// THE SATELLITES PASS CONSOLE — the structure, the scroll ownership, and the
// one thing the operator actually asked for.
//
// Operator, 2026-08-03: "think about all the items on the main sat page when you
// track, I want my list, but the schedule next list is long, and that should be
// made smaller and scrollable to free up more real estate. […] I need to see the
// top next and best 24 hours, like teh first 10 lines of the schedule -
// favorites, next 48, my screen with my LOS and AOS […] It also should contain
// the qso logging area along with the frequencies and slections of what to
// select as a prominent feature. […] that main window should have all contained,
// without any scrolling for normal operation. It also would be great to bring
// the other map to the main screen, the main screen that shows now with los
// overall could overall be reduced in size."
//
// THE ACCEPTANCE TEST IS A NO-SCROLL CLAIM, AND jsdom COMPUTES NO LAYOUT, so
// this file does NOT pretend to measure pixels. It asserts the two things that
// a no-scroll claim actually rests on and that a screenshot could not pin:
//
//   1. EVERY SURFACE HE NAMED IS IN THE DOCUMENT AT ONCE, in the region that
//      makes it reachable without scrolling — the pass lists and the schedule in
//      the bounded planning column, the sky view, the log, the frequencies and
//      the transponder chooser, and the globe. A layout can only be "all
//      contained" if the things are there to contain.
//   2. THE SCROLL OWNERSHIP IS EXACTLY WHAT THE HEIGHT BUDGET ASSUMES. The
//      budget closes because column 1 is bounded with one inner scroller and
//      column 2 is a single scroller — no third owner, and nothing nested
//      inside either beyond the one pre-existing case the section already
//      shipped knowingly (`.sats-favmgr ul`).
//
// The pixel half of the claim lives in `panel-overflow.test.ts` (the caps, the
// floors and the cascade winners, computed) and in the CSS comments that carry
// the measured numbers. Neither file can see a rendered box; both say so.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, act, waitFor } from '@testing-library/react'
import { SatellitesView } from './SatellitesView'
import type { AppSnapshot, SatDetail, SatTrackStatus, SatTransponderHeld, SatView } from '../types'

const api = vi.hoisted(() => ({
  getSatellites: vi.fn((): Promise<SatView | null> => Promise.resolve(null)),
  getSatPassNeeds: vi.fn(() => Promise.resolve([])),
  getSatDetail: vi.fn(),
  getSettings: vi.fn(),
  setSettings: vi.fn(() => Promise.resolve({} as never)),
  setPegLock: vi.fn(() => Promise.resolve()),
  confirmSatUplink: vi.fn(() => Promise.resolve()),
  setSatTransponder: vi.fn(() => Promise.resolve()),
  getSatTransponder: vi.fn((): Promise<SatTransponderHeld | null> => Promise.resolve(null)),
  startSatTrack: vi.fn(() => Promise.resolve(null)),
  stopSatTrack: vi.fn(() => Promise.resolve()),
  getSatTrackStatus: vi.fn((): Promise<SatTrackStatus | null> => Promise.resolve(null)),
  fetchTlesNow: vi.fn(() => Promise.resolve(null)),
  fdLogManual: vi.fn(async () => ({})),
  logQso: vi.fn(async () => ({})),
  getLog: vi.fn(async () => []),
  lookupPark: vi.fn(async () => null),
  lookupParkLive: vi.fn(async () => null),
  qrzLookup: vi.fn(async () => null),
  resolveEntity: vi.fn(async () => null),
  searchParks: vi.fn(async () => []),
  setCwPeerInfo: vi.fn(async () => {}),
}))
vi.mock('../api', () => api)
vi.mock('./MapView', () => ({
  // The globe's own machinery is MapView's; what matters here is that the box
  // is rendered where the operator can see it without scrolling.
  MapView: () => <div data-testid="mapview-stub" />,
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: async <T,>(action: () => Promise<T>) => action(),
}))

const NOW = Math.floor(Date.now() / 1000)
const AOS = NOW - 300
const LOS = NOW + 300

const detail = (over: Partial<SatDetail> = {}): SatDetail =>
  ({
    name: 'RS-44',
    norad: 44909,
    status: 'alive',
    transmitters: [
      { description: 'CW beacon', alive: true, kind: 'Transmitter', invert: false },
      {
        description: 'SSB/CW linear transponder',
        alive: true,
        kind: 'Transponder',
        invert: true,
        downlinkMode: 'USB',
        uplinkMode: 'LSB',
        downlinkLowHz: 435_640_000,
        downlinkHighHz: 435_660_000,
        uplinkLowHz: 145_965_000,
        uplinkHighHz: 145_985_000,
      },
    ],
    dataFetchedAt: 1_760_000_000,
    pass: {
      name: 'RS-44',
      aosUnix: AOS,
      losUnix: LOS,
      maxElDeg: 62,
      aosAzDeg: 100,
      losAzDeg: 260,
      status: 'alive',
    },
    passTrack: [
      [AOS, 100, 0],
      [NOW, 180, 62],
      [LOS, 260, 0],
    ],
    ...over,
  }) as unknown as SatDetail

const status = (over: Partial<SatTrackStatus> = {}): SatTrackStatus =>
  ({
    name: 'RS-44',
    state: 'tracking',
    mode: 'rotor+doppler',
    dopplerDownlink: true,
    dopplerUplink: true,
    uplinkOffer: 'none',
    uplinkOfferMap: null,
    uplinkRadio: 'IC-9700',
    uplinkRadioId: 1,
    azDeg: 141,
    elDeg: 46,
    aosAzDeg: 100,
    maxElDeg: 45,
    satAzDeg: 143,
    satElDeg: 47,
    rangeKm: 812,
    rangeRateKmS: -5.42,
    downlinkHz: 435_643_320,
    uplinkHz: 145_962_680,
    downlinkShiftHz: -2310,
    uplinkShiftHz: 770,
    transponder: 'SSB/CW linear transponder',
    transponderIndex: 1,
    inverting: true,
    offsetHz: 3200,
    halfWidthHz: 12_500,
    elementAgeDays: 1.2,
    elementEpochUnix: 1_785_442_400,
    aosUnix: AOS,
    losUnix: LOS,
    ...over,
  }) as SatTrackStatus

const held = (): SatTransponderHeld =>
  ({
    name: 'RS-44',
    index: 1,
    description: 'SSB/CW linear transponder',
    binding: {
      radioId: 1,
      radioName: 'IC-9700',
      band: '70cm',
      fm: false,
      downlinkMhz: 435.64332,
      uplinkMhz: 145.96268,
      pendingDownlinkMhz: null,
      pendingUplinkMhz: null,
      simplex: false,
      note: null,
    },
  }) as unknown as SatTransponderHeld

const snap = (): AppSnapshot =>
  ({
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    hunt: null,
    fieldDay: null,
    link: { tier: 'TempoFast' },
    radio: {
      dialMhz: 435.64332,
      band: '70cm',
      rigMode: 'USB',
      sideband: 'USB',
      catOk: true,
      transmitting: false,
      txEnabled: true,
      txAllowed: true,
    },
  }) as unknown as AppSnapshot

/** A catalog with two workable passes, so the Next/Best strip renders — the
 *  operator's "top next and best 24 hours" is half of what has to be on screen,
 *  and a fixture without it would let the strip quietly disappear. */
const view = (): SatView =>
  ({
    tleAgeDays: 1,
    usableCount: 300,
    agingCount: 0,
    heldBackCount: 0,
    tleFetchedAt: NOW,
    tleSource: 'mirror',
    birds: [
      { name: 'RS-44', norad: 44909, lat: 0, lon: 0, altKm: 1200, footprintKm: 4000, track: [], status: 'alive', amateur: true },
      { name: 'AO-91', norad: 43017, lat: 0, lon: 0, altKm: 700, footprintKm: 3000, track: [], status: 'alive', amateur: true },
    ],
    passes: [
      { name: 'RS-44', norad: 44909, aosUnix: AOS, losUnix: LOS, maxElDeg: 62, aosAzDeg: 100, losAzDeg: 260 },
      { name: 'AO-91', norad: 43017, aosUnix: NOW + 2400, losUnix: NOW + 3000, maxElDeg: 41, aosAzDeg: 10, losAzDeg: 190 },
    ],
    excluded: [],
  }) as unknown as SatView

const settings = () => ({
  mygrid: 'EN52',
  rotatorModel: 2,
  rotatorHost: '',
  satDopplerOff: false,
  satVfoMap: 'main-down-sub-up',
})

beforeEach(() => {
  localStorage.clear()
  api.getSatellites.mockReset()
  api.getSatellites.mockImplementation(() => Promise.resolve(view()))
  api.getSatDetail.mockReset()
  api.getSatDetail.mockImplementation(() => Promise.resolve(detail()))
  api.getSettings.mockReset()
  api.getSettings.mockImplementation(() => Promise.resolve(settings()))
  api.getSatTrackStatus.mockReset()
  api.getSatTrackStatus.mockImplementation(() => Promise.resolve(status()))
  api.getSatTransponder.mockReset()
  api.getSatTransponder.mockImplementation(() => Promise.resolve(held()))
  api.setSatTransponder.mockClear()
  api.stopSatTrack.mockClear()
})
afterEach(cleanup)

/** Render the console mid-pass, with a track armed and a transponder held. */
async function console_() {
  const r = render(<SatellitesView focusSat="RS-44" snap={snap()} />)
  await screen.findByTestId('sat-rail')
  return r
}

describe('everything needed to work a pass is on the first screen', () => {
  it('renders all six surfaces the operator named, at once', async () => {
    await console_()
    const q = (sel: string) => document.querySelector(sel)

    // 1. THE PASS LISTS — the Next/Best strip and the 48 h schedule, both in
    //    the bounded planning column.
    const plan = q('.sats-plan')!
    expect(plan, 'the planning column is missing').not.toBeNull()
    expect(plan.querySelector('.sats-sched'), 'the schedule left the planning column').not.toBeNull()

    // 2. HIS SKY VIEW WITH AOS/LOS. Both rim plates and the DOM readout.
    expect(q('.sat-sky'), 'the sky dome is gone').not.toBeNull()
    expect(screen.getByTestId('sat-aos-tag')).toBeTruthy()
    expect(screen.getByTestId('sat-los-tag')).toBeTruthy()
    expect(q('.sat-dome-readout')!.textContent).toMatch(/Rise \/ set/)

    // 3. THE QSO LOGGING AREA.
    expect(screen.getByPlaceholderText('Call')).toBeTruthy()

    // 4. THE FREQUENCIES AND THE TRANSPONDER SELECTION, "as a prominent
    //    feature" — which here means: in the planning column, permanently
    //    visible, NOT inside the one surface that scrolls.
    const radio = q('.sats-radio')!
    expect(radio, 'the radio quadrant is gone').not.toBeNull()
    expect(radio.closest('.sats-plan'), 'the radio quadrant left the bounded column').not.toBeNull()
    expect(radio.querySelector('.sat-doppler'), 'no Doppler readout in the quadrant').not.toBeNull()
    expect(radio.querySelector('.sat-pb'), 'no passband strip in the quadrant').not.toBeNull()
    expect(
      radio.querySelector('[data-testid="sat-tp-list"]'),
      'the transponder chooser is not in the quadrant',
    ).not.toBeNull()

    // 5. THE OTHER MAP, on the main screen — beside the dome, not two screens
    //    below it.
    const graphics = screen.getByTestId('sats-pass-graphics')
    expect(graphics.querySelector('.sat-sky'), 'the dome left the graphics row').not.toBeNull()
    expect(
      graphics.querySelector('[data-testid="sat-globe-box"]'),
      'the globe is not beside the dome',
    ).not.toBeNull()

    // 6. THE ARM BAR — what is armed and what it is driving, above both columns.
    const bar = screen.getByTestId('sats-armbar')
    expect(bar.querySelector('.sats-arm-id')!.textContent).toMatch(/RS-44/)
    expect(bar.querySelector('[data-testid="sat-rail"]'), 'the rail left the bar').not.toBeNull()
    expect(bar.querySelector('[data-testid="sat-radio-binding"]')).not.toBeNull()
    expect(bar.querySelector('[data-testid="sat-lockon"]')).not.toBeNull()
    expect(bar.querySelector('.sats-detail-close'), 'the ✕ left the bar').not.toBeNull()
  })

  it('the two columns hold the split the height budget assumes', async () => {
    await console_()
    // Planning column: lists + radio. Pass column: the pass, the log, Birds.
    // Anything that drifts across this line breaks the arithmetic in
    // styles.css's header comment, because the two columns are budgeted apart.
    for (const sel of ['.sats-best', '.sats-sched', '.sats-radio']) {
      expect(document.querySelector(sel)!.closest('.sats-plan'), `${sel} is not in column 1`).not.toBeNull()
    }
    for (const sel of ['.sats-detail', '.sats-log', '.sats-favmgr']) {
      expect(document.querySelector(sel)!.closest('.sats-side'), `${sel} is not in column 2`).not.toBeNull()
    }
    // The arm bar spans both and is a child of neither.
    const bar = screen.getByTestId('sats-armbar')
    expect(bar.closest('.sats-plan')).toBeNull()
    expect(bar.closest('.sats-side')).toBeNull()
    expect(bar.parentElement!.className).toBe('sats-view')
  })
})

describe('the schedule is capped and scrolls INSIDE ITS OWN BOX', () => {
  it('the disclosure strip never scrolls; the rows do, in one inner scroller', async () => {
    await console_()
    const sched = document.querySelector('.sats-sched')!
    const strip = sched.querySelector('.sats-sched-strip')!
    const scroller = sched.querySelector('.sats-sched-scroll')!

    // The table lives in the scroller; the h2 and the "Other birds overhead"
    // disclosure chip live outside it. A 126-row schedule must never carry the
    // chip away — that chip is the discovery band's whole control surface.
    expect(scroller.querySelector('table'), 'the table left the scroller').not.toBeNull()
    expect(strip.querySelector('.sats-more-chip'), 'the disclosure chip left the strip').not.toBeNull()
    expect(strip.querySelector('table')).toBeNull()
    expect(scroller.contains(strip)).toBe(false)
  })

  it('what bounds it is the radio quadrant beneath it, not a cap on the list', async () => {
    await console_()
    // The operator asked for the list to be "made smaller and scrollable to free
    // up more real estate". The reclaimed real estate has to go somewhere NAMED
    // or the cap rots — and here it goes to the frequencies and the transponder
    // chooser, in the same bounded column, directly below.
    const plan = document.querySelector('.sats-plan')!
    const kids = Array.from(plan.children).map((c) => c.className.split(' ')[0])
    expect(kids, 'the planning column is not [strip, schedule, radio]').toEqual([
      'sats-best',
      'sats-sched',
      'sats-radio',
    ])
  })
})

describe('scroll ownership is exactly what the budget assumes', () => {
  it('two sibling owners, and nothing new nested inside either', async () => {
    await console_()
    // These are the DOM's structural facts; the CSS half (which of them declares
    // overflow, and whether that declaration wins) is computed in
    // panel-overflow.test.ts. Together they are the claim.
    const schedScroll = document.querySelector('.sats-sched-scroll')!
    const side = document.querySelector('.sats-side')!

    // OWNER #1 owns column 1 and contains no second scroller. `.sats-favmgr ul`
    // — the one pre-existing nested case — is in the OTHER column.
    expect(schedScroll.querySelector('.sats-favmgr')).toBeNull()
    expect(schedScroll.querySelector('.sats-side')).toBeNull()

    // OWNER #2 owns column 2. The pass card, the log and Birds are all inside
    // it, and the radio quadrant — which must never scroll — is not.
    expect(side.querySelector('.sats-radio')).toBeNull()
    expect(side.querySelector('.sats-sched-scroll')).toBeNull()

    // Neither owner is inside the other.
    expect(side.contains(schedScroll)).toBe(false)
    expect(schedScroll.contains(side)).toBe(false)
  })
})

describe('every control still does what it did', () => {
  it('the ■ stop in the arm bar still disarms the track', async () => {
    await console_()
    const bar = screen.getByTestId('sats-armbar')
    const stop = Array.from(bar.querySelectorAll('button')).find((b) => /■ stop/.test(b.textContent ?? ''))!
    expect(stop, 'the rail lost its stop control in the move').toBeTruthy()
    await act(async () => {
      fireEvent.click(stop)
    })
    await waitFor(() => expect(api.stopSatTrack).toHaveBeenCalled())
  })

  it('the badge and the ✕ are emitted BEFORE the rail — one bar line, both columns', async () => {
    // A HEIGHT INVARIANT THAT ONLY DOCUMENT ORDER CAN CARRY. `.sat-rail` is
    // `flex: 1 1 100%` — a deliberate wrap point, so the readiness rail always
    // takes its own bar line and ANYTHING after it starts a third. With the
    // badge and the ✕ last (how this shipped), measured at the 1024×768 floor:
    // 99.2 effective px and four flex lines. Emitted before the rail: 79.9 px
    // and three. The bar spans BOTH columns, so that ~16.5 px is paid twice —
    // once by the schedule's row count and once by the pass column's fold.
    // No CSS rule can state this; `order` would fix the paint and leave the tab
    // order wrong. So it is asserted where it lives: in the DOM.
    await console_()
    const bar = screen.getByTestId('sats-armbar')
    const kids = Array.from(bar.children)
    const at = (sel: string) => kids.findIndex((k) => k.matches(sel))
    const rail = at('[data-testid="sat-rail"]')
    expect(rail, 'the readiness rail left the arm bar').toBeGreaterThan(-1)
    for (const sel of ['.sats-tracking-badge', '.sats-detail-close']) {
      const i = at(sel)
      expect(i, `${sel} left the arm bar`).toBeGreaterThan(-1)
      expect(
        i,
        `${sel} is emitted after \`.sat-rail\`, whose \`flex: 1 1 100%\` gives it a bar line of ` +
          'its own — that is a third line at every window size, and both columns pay for it',
      ).toBeLessThan(rail)
    }
  })

  it('a transponder card in the quadrant still runs the pick', async () => {
    await console_()
    const cards = screen.getByTestId('sat-tp-list')
    const beacon = cards.querySelector<HTMLInputElement>('input[aria-label="Work CW beacon"]')!
    await act(async () => {
      fireEvent.click(beacon)
    })
    await waitFor(() => expect(api.setSatTransponder).toHaveBeenCalledWith('RS-44', 0, false))
  })

  it('the ✕ closes the detail without disarming, and the badge is the way back', async () => {
    await console_()
    const bar = screen.getByTestId('sats-armbar')
    await act(async () => {
      fireEvent.click(bar.querySelector<HTMLButtonElement>('.sats-detail-close')!)
    })
    await waitFor(() => expect(document.querySelector('.sats-detail')).toBeNull())
    // The track is untouched — closing is navigation, never a disarm — and the
    // bar still renders, from `track` alone, carrying the way back in.
    expect(api.stopSatTrack).not.toHaveBeenCalled()
    const badge = document.querySelector('.sats-tracking-badge')!
    expect(badge, 'the way back to a live pass vanished with the detail').not.toBeNull()
    expect(badge.closest('[data-testid="sats-armbar"]'), 'the badge left the bar').not.toBeNull()
    await act(async () => {
      fireEvent.click(badge.querySelector<HTMLButtonElement>('.sats-badge-open')!)
    })
    await waitFor(() => expect(document.querySelector('.sats-detail')).not.toBeNull())
  })
})

describe('the rail’s pick fix lands the operator on the chooser', () => {
  it('focuses the held card instead of a scroll that is now a no-op', async () => {
    // It used to be a bare `scrollIntoView`, which was right while the chooser
    // sat far down a scrolling column. The chooser is in the planning quadrant
    // now — permanently on screen at md+ — so that scroll does nothing there,
    // and a fix button that visibly does nothing reads as broken.
    await console_()
    const rail = screen.getByTestId('sat-rail')
    const pick = Array.from(rail.querySelectorAll('button')).find((b) =>
      /^(pick|change)$/.test(b.textContent?.trim() ?? ''),
    )!
    expect(pick, 'the Transponder gate lost its fix button').toBeTruthy()
    await act(async () => {
      fireEvent.click(pick)
    })
    const held = screen
      .getByTestId('sat-tp-list')
      .querySelector<HTMLInputElement>('input[type="radio"]:checked')!
    expect(held, 'nothing is held in the fixture').toBeTruthy()
    expect(document.activeElement, 'the pick fix did not land on the chooser').toBe(held)
  })
})

describe('the transponder chooser is bounded by a row count, never a scroller', () => {
  const many = () =>
    detail({
      transmitters: Array.from({ length: 9 }, (_, i) => ({
        description: `TX ${i}`,
        alive: true,
        kind: 'Transponder',
        invert: false,
      })),
    } as Partial<SatDetail>)

  it('caps the workable cards and discloses the remainder', async () => {
    api.getSatDetail.mockImplementation(() => Promise.resolve(many()))
    api.getSatTransponder.mockImplementation(() => Promise.resolve(null))
    render(<SatellitesView focusSat="RS-44" snap={snap()} />)
    const list = await screen.findByTestId('sat-tp-list')
    // 4 workable + the "None" card. A ninth card would come straight out of the
    // schedule's row count in the bounded column above it.
    await waitFor(() => expect(list.querySelectorAll('.sat-tp-card').length).toBe(5))
    const more = screen.getByRole('button', { name: /show all 9/ })
    await act(async () => {
      fireEvent.click(more)
    })
    await waitFor(() => expect(list.querySelectorAll('.sat-tp-card').length).toBe(10))
  })

  it('never hides the HELD card behind the cap', async () => {
    // A chooser that hides the current selection reads as "None" — and "None"
    // is a consent statement in this section, not an empty slot.
    api.getSatDetail.mockImplementation(() => Promise.resolve(many()))
    api.getSatTransponder.mockImplementation(() =>
      Promise.resolve({ name: 'RS-44', index: 7, description: 'TX 7', binding: null } as unknown as SatTransponderHeld),
    )
    render(<SatellitesView focusSat="RS-44" snap={snap()} />)
    const list = await screen.findByTestId('sat-tp-list')
    await waitFor(() =>
      expect(list.querySelector('input[aria-label="Work TX 7"]')).not.toBeNull(),
    )
    expect(list.querySelectorAll('.sat-tp-card').length).toBe(5)
    expect(
      list.querySelector<HTMLInputElement>('input[aria-label="Work TX 7"]')!.checked,
      'the held card is shown but not selected',
    ).toBe(true)
  })
})
