// @vitest-environment jsdom
//
// The transponder picker in the Satellites detail pane. What is pinned here is the wiring that
// decides WHERE THE RADIO TRANSMITS:
//
//  - `set_sat_transponder` takes the RAW index into the list `get_sat_detail` returned — dead
//    entries INCLUDED (the backend indexes that very list and refuses dead picks by name). The
//    chooser collapses dead entries behind "show N inactive", but the wire index never shifts:
//    an alive-relative index would select a different transponder — a different uplink — and
//    look fine on screen.
//  - INVERTING is per-transponder data and decides which way the uplink moves. It has to be
//    visible on the card, not buried in a tooltip.
//  - Nothing is selected until the operator picks, and a refused call never shows as selected.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SatellitesView } from './SatellitesView'
import type { SatDetail, SatTransmitter } from '../types'

const api = vi.hoisted(() => ({
  getSatellites: vi.fn(() => Promise.resolve(null)),
  getSatSchedule: vi.fn(() => Promise.resolve([])),
  getSatPassNeeds: vi.fn(() => Promise.resolve([])),
  getSatDetail: vi.fn(),
  getSettings: vi.fn(),
  setSettings: vi.fn(() => Promise.resolve({} as never)),
  setSatTransponder: vi.fn(() => Promise.resolve()),
  getSatTransponder: vi.fn((): Promise<import('../types').SatTransponderHeld | null> => Promise.resolve(null)),
  startSatTrack: vi.fn(() => Promise.resolve(null)),
  stopSatTrack: vi.fn(() => Promise.resolve()),
  getSatTrackStatus: vi.fn(() => Promise.resolve(null)),
}))
vi.mock('../api', () => api)
// The detail pane embeds the globe; it needs a canvas and nothing here is about the map.
vi.mock('./MapView', () => ({ MapView: () => null }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

/** RS-44 as SatNOGS actually describes a linear bird: a dead entry first, then a beacon, then
 * the inverting transponder people work. The dead-first order is the whole point. */
const detail = (): SatDetail => ({
  name: 'RS-44',
  norad: 44909,
  status: 'alive',
  transmitters: [
    {
      description: 'Retired FM repeater',
      alive: false,
      mode: 'FM',
      uplinkLowHz: 145_900_000,
      downlinkLowHz: 435_000_000,
      invert: false,
      uplinkHighHz: null,
      downlinkHighHz: null,
      uplinkMode: null,
      downlinkMode: null,
      kind: 'Transceiver',
    },
    {
      description: 'CW beacon',
      alive: true,
      mode: 'CW',
      uplinkLowHz: null,
      downlinkLowHz: 435_605_000,
      invert: false,
      uplinkHighHz: null,
      downlinkHighHz: null,
      uplinkMode: null,
      downlinkMode: 'CW',
      kind: 'Transmitter',
    },
    {
      description: 'SSB/CW linear transponder',
      alive: true,
      mode: 'LSB',
      uplinkLowHz: 145_965_000,
      downlinkLowHz: 435_640_000,
      invert: true,
      uplinkHighHz: 145_995_000,
      downlinkHighHz: 435_670_000,
      uplinkMode: 'LSB',
      downlinkMode: 'USB',
      kind: 'Transponder',
    },
  ],
  dataFetchedAt: 1_760_000_000,
  pass: null,
  passTrack: [],
})

const settings = (over: Record<string, unknown> = {}) => ({
  mygrid: 'EN52',
  rotatorModel: 0,
  rotatorHost: '',
  satDopplerOff: false,
  satVfoMap: 'off',
  ...over,
})

beforeEach(() => {
  localStorage.clear()
  api.getSatellites.mockClear()
  api.getSatDetail.mockReset()
  api.getSatDetail.mockImplementation(() => Promise.resolve(detail()))
  api.getSettings.mockReset()
  api.getSettings.mockImplementation(() => Promise.resolve(settings()))
  api.setSatTransponder.mockReset()
  api.setSatTransponder.mockImplementation(() => Promise.resolve())
})
afterEach(cleanup)

const linearRadio = () => screen.findByLabelText('Work SSB/CW linear transponder')
const noneRadio = () => screen.findByLabelText('Work no transponder — leave the dial to me')

describe('picking a transponder', () => {
  it('sends the ROW index of the list the operator was shown', async () => {
    // The fixture lists a DEAD transmitter before this one, which is the trap:
    // the command briefly indexed only the alive entries on its side, so this
    // row selected a different transponder — a different uplink — silently.
    // The backend now indexes the same list get_sat_detail returned and refuses
    // a dead pick by name, so the row number IS the wire index. Sending an
    // alive-relative index here would now select the wrong row.
    render(<SatellitesView focusSat="RS-44" />)
    fireEvent.click(await linearRadio())
    await waitFor(() => expect(api.setSatTransponder).toHaveBeenCalledWith('RS-44', 2, false))
  })

  it('collapses dead transmitters behind "show N inactive" and never offers them', async () => {
    render(<SatellitesView focusSat="RS-44" />)
    await screen.findByTestId('sat-tp-list')
    // Collapsed by default: the dead entry is not in the DOM, the count is.
    expect(screen.queryByText(/Retired FM repeater/)).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: /show 1 inactive/ }))
    // Expanded: shown for the record, marked dead, and NEVER a pickable radio.
    const card = (await screen.findByText(/Retired FM repeater/)).closest('.sat-tp-card')
    expect(card?.className).toMatch(/off/)
    expect(card?.textContent).toMatch(/reported dead/)
    expect(screen.queryByLabelText('Work Retired FM repeater')).toBeNull()
  })

  it('starts with nothing selected and hands the dial back when cleared', async () => {
    render(<SatellitesView focusSat="RS-44" />)
    expect(((await noneRadio()) as HTMLInputElement).checked).toBe(true)
    fireEvent.click(await linearRadio())
    await waitFor(() => expect(((linearRadioSync()) as HTMLInputElement).checked).toBe(true))
    fireEvent.click(await noneRadio())
    await waitFor(() => expect(api.setSatTransponder).toHaveBeenLastCalledWith('RS-44', null, false))
  })

  it('does not show a selection the backend refused', async () => {
    // The radio is either under Doppler control or it is not. A control that latches on a
    // failed call tells the operator their uplink is being steered when it is not.
    api.setSatTransponder.mockImplementation(() => Promise.reject(new Error('no transponder #1')))
    render(<SatellitesView focusSat="RS-44" />)
    fireEvent.click(await linearRadio())
    await waitFor(() => expect(api.setSatTransponder).toHaveBeenCalled())
    expect(((await linearRadio()) as HTMLInputElement).checked).toBe(false)
    expect(((await noneRadio()) as HTMLInputElement).checked).toBe(true)
  })
})

describe('a bird with no transmitters on screen (#269)', () => {
  it('says "not fetched yet" while the SatNOGS cache has never covered the bird', async () => {
    // CO-57 opened after the favourites fetch: the snapshot has a fetch time but was never asked
    // about this bird. "None listed" there is false — SatNOGS lists plenty for it.
    api.getSatDetail.mockImplementation(() =>
      Promise.resolve({ ...detail(), transmitters: [], transmittersCovered: false }),
    )
    render(<SatellitesView focusSat="RS-44" />)
    expect(await screen.findByText(/not fetched yet/)).toBeTruthy()
    expect(screen.queryByText(/no transmitters listed/)).toBeNull()
  })

  it('says "none listed" only once the cache covers the bird and it has none', async () => {
    api.getSatDetail.mockImplementation(() =>
      Promise.resolve({ ...detail(), transmitters: [], transmittersCovered: true }),
    )
    render(<SatellitesView focusSat="RS-44" />)
    expect(await screen.findByText(/no transmitters listed for this bird/)).toBeTruthy()
    expect(screen.queryByText(/not fetched yet/)).toBeNull()
  })
})

describe('what the row tells the operator', () => {
  it('marks the inverting transponder, and only that one', async () => {
    render(<SatellitesView focusSat="RS-44" />)
    const marks = await screen.findAllByText('INVERTING')
    expect(marks).toHaveLength(1)
    expect(marks[0].closest('.sat-tp-card')?.textContent).toMatch(/SSB\/CW linear transponder/)
  })

  it('shows the passband, not just its low edge', async () => {
    // A linear transponder is a band. "435.640" alone hides where in it you can work.
    render(<SatellitesView focusSat="RS-44" />)
    expect(await screen.findByText('435.640–435.670')).toBeTruthy()
    expect(await screen.findByText('145.965–145.995')).toBeTruthy()
    // A beacon has no uplink and says so, rather than showing a made-up frequency.
    const beaconCard = (await screen.findByText(/CW beacon/)).closest('.sat-tp-card')
    expect(beaconCard?.textContent).toMatch(/435\.605/)
    expect(beaconCard?.textContent).toMatch(/—/)
  })

  it('says plainly when a pick will not tune anything', async () => {
    // The operator's own off switch is the one thing that stops a pick tuning.
    // Without this line the picker looks like a dead control.
    api.getSettings.mockImplementation(() => Promise.resolve(settings({ satDopplerOff: true })))
    render(<SatellitesView focusSat="RS-44" />)
    fireEvent.click(await linearRadio())
    expect(
      await screen.findByText(/Doppler correction is off, so nothing is being tuned/),
    ).toBeTruthy()
  })

  it('out of the box, says when the tuning actually happens', async () => {
    // NOTHING configured — no switch flipped, no mapping chosen. A pick tunes
    // the downlink, and the line says so rather than pointing at Settings.
    render(<SatellitesView focusSat="RS-44" />)
    fireEvent.click(await linearRadio())
    expect(
      await screen.findByText(/tunes this transponder while auto-track is following the pass/),
    ).toBeTruthy()
  })

  it('with a confirmed mapping, says the same', async () => {
    api.getSettings.mockImplementation(() =>
      Promise.resolve(settings({ satVfoMap: 'main-down-sub-up' })),
    )
    render(<SatellitesView focusSat="RS-44" />)
    fireEvent.click(await linearRadio())
    expect(
      await screen.findByText(/tunes this transponder while auto-track is following the pass/),
    ).toBeTruthy()
  })
})

/** A transmitter record as SatNOGS lists it, trimmed. `baud` is deliberately not defaulted: a
 * record without it is how a station that predates the field sends one. */
const tx = (over: Partial<SatTransmitter>): SatTransmitter => ({
  description: '',
  alive: true,
  mode: null,
  uplinkLowHz: null,
  downlinkLowHz: 435_525_000,
  invert: false,
  uplinkHighHz: null,
  downlinkHighHz: null,
  uplinkMode: null,
  downlinkMode: null,
  kind: 'Transmitter',
  ...over,
})
const bird = (name: string, norad: number, transmitters: SatTransmitter[]): SatDetail => ({
  ...detail(),
  name,
  norad,
  transmitters,
})
/** The text of every chip on the card whose description is `description`. */
const chips = async (description: string) =>
  Array.from(
    (await screen.findByText(description)).closest('.sat-tp-card')?.querySelectorAll('.sat-tp-kind') ??
      [],
  ).map((c) => c.textContent)
/** The text of the ↓ and ↑ legs on the card whose description is `description`. */
const legs = async (description: string) =>
  Array.from(
    (await screen.findByText(description)).closest('.sat-tp-card')?.querySelectorAll('.sat-tp-leg') ??
      [],
  ).map((l) => l.textContent)

describe('the downlink data rate SatNOGS lists (baud)', () => {
  it('reads beside the mode, whole rates without a decimal and fractional ones kept', async () => {
    // KOSEN-1's own downlinks, as the live catalog gave them on 2026-09-23. The 45.45 is NORAD
    // 47721's RTTY beacon, here for its fractional rate: rounding it to 45 would misstate it.
    api.getSatDetail.mockImplementation(() =>
      Promise.resolve(
        bird('KOSEN-1', 49402, [
          tx({ description: 'Mode U - AFSK1k2', mode: 'AFSK', baud: 1200 }),
          tx({ description: 'Mode U - GMSK9k6', mode: 'GMSK', baud: 9600 }),
          tx({ description: 'Mode U - FSK RTTY', mode: 'FSK', baud: 45.45 }),
        ]),
      ),
    )
    render(<SatellitesView focusSat="KOSEN-1" />)
    expect(await chips('Mode U - AFSK1k2')).toEqual(['beacon', 'AFSK · 1200 bd'])
    expect(await chips('Mode U - GMSK9k6')).toEqual(['beacon', 'GMSK · 9600 bd'])
    expect(await chips('Mode U - FSK RTTY')).toEqual(['beacon', 'FSK · 45.45 bd'])
    // Once per card: the chip carries it, so the ↓ leg does not repeat it.
    expect(await legs('Mode U - AFSK1k2')).toEqual(['↓ 435.525', '↑ —'])
  })

  it('is left off entirely when the catalog does not know it — never "0 bd"', async () => {
    api.getSatDetail.mockImplementation(() =>
      Promise.resolve(
        bird('ISS', 25544, [
          tx({ description: 'Mode U - SSTV', mode: 'SSTV', baud: null }),
          // SatNOGS lists 0.0 on 31 entries, mostly CW beacons, this SSTV one among them. The
          // backend reads it as unknown, and the card must too if one ever arrives.
          tx({ description: 'Mode V Imaging', mode: 'SSTV', baud: 0 }),
          // No key at all: a station that predates the field, read by a newer Remote page.
          tx({
            description: 'Mode V/U FM - Voice Repeater',
            mode: 'FM',
            kind: 'Transceiver',
            uplinkLowHz: 145_990_000,
            downlinkLowHz: 437_800_000,
          }),
        ]),
      ),
    )
    render(<SatellitesView focusSat="ISS" />)
    expect(await chips('Mode U - SSTV')).toEqual(['beacon', 'SSTV'])
    expect(await chips('Mode V Imaging')).toEqual(['beacon', 'SSTV'])
    expect(await chips('Mode V/U FM - Voice Repeater')).toEqual(['FM repeater', 'FM'])
    expect(screen.getByTestId('sat-tp-list').textContent).not.toMatch(/\bbd\b/)
  })

  it('belongs to the downlink leg, never beside the uplink mode', async () => {
    // KOSEN-1's onboard-SDR transponder sends AFSK down at 1200 and takes CW up. A card with a
    // per-leg mode has no mode chip, so the rate rides the ↓ leg; beside "CW" it would describe
    // a 1200-baud CW uplink that does not exist.
    api.getSatDetail.mockImplementation(() =>
      Promise.resolve(
        bird('KOSEN-1', 49402, [
          tx({
            description: 'Mode HF/U - Onboard SDR',
            kind: 'Transponder',
            mode: 'AFSK',
            uplinkMode: 'CW',
            baud: 1200,
            uplinkLowHz: 21_125_000,
            uplinkHighHz: 21_150_000,
          }),
        ]),
      ),
    )
    render(<SatellitesView focusSat="KOSEN-1" />)
    expect(await legs('Mode HF/U - Onboard SDR')).toEqual(['↓ 435.525 1200 bd', '↑ 21.125–21.150 CW'])
    expect(await chips('Mode HF/U - Onboard SDR')).toEqual(['linear'])
  })
})

/** Sync lookup for the assertion inside waitFor (findBy* would race the re-render). */
function linearRadioSync() {
  return screen.getByLabelText('Work SSB/CW linear transponder')
}
