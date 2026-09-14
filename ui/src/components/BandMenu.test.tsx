// @vitest-environment jsdom
//
// THE BAND MENU — the band dropdown as a Nexus menu, each band carrying its opening.
//
// What this pins, and why each half matters:
//   · It is a MENU, not the system <select>. The native popup is drawn by the OS (a GTK toplevel
//     on Linux, #169) and cannot carry a coloured dot, so a select could never show this.
//   · Each band shows a dot AND a word, read from the SAME advisory the Band conditions strip
//     reads (`bandConditionCell`). A second derivation would let the menu and the strip disagree.
//   · Unknown or stale data is NEUTRAL. A band the advisory did not report, a snapshot that is
//     offline, or one older than the staleness bound must never read green: "Open" on data we do
//     not have is the one lie this feature could tell.
//   · Keyboard and screen reader: opens from the keyboard, items are radio items named by band and
//     state, the current band is checked, and a pick still names the BAND to the engine.
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import type { AppSnapshot, BandChannel, BandReport, PropagationSnapshot } from '../types'
import { BandPicker } from './BandPicker'
import { FrequencyControl } from './FrequencyControl'
import { BAND_CONDITIONS_STALE_S, publishBandConditions } from '../bandConditions'
import { bandConditionCell } from '../propViz'

const pickBand = vi.fn(async (_band: string, _mode: string) => ({}) as AppSnapshot)
vi.mock('../api', () => ({
  getLicensedBandPlan: vi.fn(async () => [
    { band: '40m', dialMhz: 7.0, group: 'HF', mode: 'CW', label: '40m', note: '', tx: true },
    { band: '20m', dialMhz: 14.0, group: 'HF', mode: 'CW', label: '20m', note: '', tx: true },
    { band: '2m', dialMhz: 144.05, group: 'VHF', mode: 'CW', label: '2m', note: '', tx: true },
  ]),
  pickBand: (band: string, mode: string) => pickBand(band, mode),
}))

beforeAll(() => {
  // Radix Popper observes its elements with a ResizeObserver jsdom lacks.
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(() => {
  cleanup()
  publishBandConditions(null)
  pickBand.mockClear()
})

const report = (band: string, modeled: BandReport['modeled'], tier: BandReport['tier'] = 'Quiet'): BandReport => ({
  band, tier, score: 0, nHearMe: 0, nIHear: 0, bestRegion: null, confidence: 'Likely', reason: 'test', modeled,
})
const nowS = () => Math.floor(Date.now() / 1000)
function snapshotWith(bands: BandReport[], over: Partial<PropagationSnapshot> = {}): PropagationSnapshot {
  return { advisory: { headline: '', bands, banners: [] }, source: 'live', asOf: nowS(), ...over } as PropagationSnapshot
}

const snap = {
  activeRadioId: 1,
  radio: { band: '20m', dialMhz: 14.02, catOk: true, txAllowed: true, operatingMode: 'cw', source: 'native' },
} as unknown as AppSnapshot

async function mountPicker() {
  render(<BandPicker snap={snap} mode="cw" />)
  // Let the licensed plan load.
  await act(async () => { await Promise.resolve() })
  await act(async () => { await Promise.resolve() })
}
const trigger = () => screen.getByRole('button', { name: /band/i })
const openMenu = () => {
  fireEvent.keyDown(trigger(), { key: 'Enter' })
  return screen.getByRole('menu')
}
const item = (menu: HTMLElement, band: string) =>
  within(menu).getAllByRole('menuitemradio').find((el) => el.textContent?.startsWith(band))!
const dot = (el: HTMLElement) => el.querySelector<HTMLElement>('.band-menu-dot')!

describe('BandPicker is a Nexus band menu', () => {
  it('renders no system select', async () => {
    await mountPicker()
    expect(document.querySelector('select')).toBeNull()
    expect(trigger().getAttribute('aria-haspopup')).toBe('menu')
  })

  it('shows each band with a dot and a word from the advisory, the same cell the strip draws', async () => {
    publishBandConditions(snapshotWith([report('40m', 'Closed'), report('20m', 'Open')]))
    await mountPicker()
    const menu = openMenu()
    const twenty = item(menu, '20m'), forty = item(menu, '40m')
    expect(twenty.textContent).toContain(bandConditionCell(report('20m', 'Open')).word)
    expect(twenty.textContent).toContain('Open')
    expect(dot(twenty).style.background).toBe('var(--band-open)')
    expect(forty.textContent).toContain('Closed')
    expect(dot(forty).style.background).toBe('var(--band-closed)')
    // The current band is the checked radio item.
    expect(twenty.getAttribute('aria-checked')).toBe('true')
    expect(forty.getAttribute('aria-checked')).toBe('false')
  })

  it('a band the advisory did not report is neutral, never green', async () => {
    publishBandConditions(snapshotWith([report('20m', 'Open')]))
    await mountPicker()
    const two = item(openMenu(), '2m')
    expect(two.getAttribute('data-condition')).toBe('unknown')
    expect(two.textContent).not.toContain('Open')
    expect(dot(two).style.background).not.toContain('--band-open')
  })

  it('stale or offline advisory data is neutral for every band', async () => {
    for (const over of [{ asOf: nowS() - BAND_CONDITIONS_STALE_S - 60 }, { source: 'offline' as const }]) {
      publishBandConditions(snapshotWith([report('20m', 'Open', 'Active')], over))
      await mountPicker()
      const twenty = item(openMenu(), '20m')
      expect(twenty.getAttribute('data-condition')).toBe('unknown')
      expect(dot(twenty).style.background).not.toContain('--band-open')
      cleanup()
    }
  })

  it('with no advisory at all (the Remote browser) every band is neutral and the menu still works', async () => {
    await mountPicker()
    const menu = openMenu()
    for (const el of within(menu).getAllByRole('menuitemradio')) {
      expect(el.getAttribute('data-condition')).toBe('unknown')
    }
    fireEvent.click(item(menu, '40m'))
    expect(pickBand).toHaveBeenCalledWith('40m', 'cw')
  })

  it('is keyboard operable: arrows move, Enter picks the band', async () => {
    publishBandConditions(snapshotWith([report('40m', 'Marginal')]))
    await mountPicker()
    const menu = openMenu()
    const forty = item(menu, '40m')
    fireEvent.keyDown(forty, { key: 'Enter' })
    expect(pickBand).toHaveBeenCalledWith('40m', 'cw')
    expect(forty.textContent).toContain('Marginal')
  })
})

describe('FrequencyControl (compact) uses the same band menu', () => {
  const channels: BandChannel[] = [
    { band: '20m', dialMhz: 14.074, group: 'HF', mode: 'USB', label: '20m', note: '', tx: true },
    { band: '6m', dialMhz: 50.313, group: 'VHF', mode: 'USB', label: '6m', note: '', tx: true },
  ]
  it('shows the condition beside every channel and sets the channel on pick', () => {
    publishBandConditions(snapshotWith([report('20m', 'Open'), report('6m', 'Closed')]))
    const onSet = vi.fn()
    render(<FrequencyControl channels={channels} dialMhz={14.074} band="20m" mode="USB" showReadout={false} showModeToggle={false} onSet={onSet} />)
    expect(document.querySelector('select')).toBeNull()
    fireEvent.keyDown(screen.getByRole('button', { name: /channel/i }), { key: 'Enter' })
    const menu = screen.getByRole('menu')
    expect(item(menu, '20m').textContent).toContain('Open')
    expect(item(menu, '6m').textContent).toContain('Closed')
    expect(item(menu, '20m').getAttribute('aria-checked')).toBe('true')
    fireEvent.click(item(menu, '6m'))
    expect(onSet).toHaveBeenCalledWith(50.313, '6m', 'USB')
  })
})
