// @vitest-environment jsdom
//
// THE GETTING OUT LIST'S SORT BAR: Distance (the default, the station's own order), Call A–Z, Newest
// and SNR, as a named radio group over the list. Each box keeps its own choice across a restart (a
// remount here; the record is the window's storage), and a value this build does not know is Distance.
// Rendered through the one body renderer every box goes through (PaneBody), with the box's place as
// its instance, as Conditions, the dashboard rail and a cockpit's box hand it.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { PaneBody } from './PaneFrame'
import type { PaneContext } from './paneContext'
import type { GettingOut, HeardMe } from '../../types'

const r = (call: string, km: number, snr: number | null, ageSecs: number): HeardMe => ({
  call,
  grid: null,
  band: '20m',
  snr,
  bearingDeg: 0,
  km,
  octant: 'N',
  ageSecs,
})

// The station's order: most distant first.
const REPORTS = [r('VK4DD', 15000, null, 30), r('JA3CC', 10000, -5, 600), r('DL2BB', 7000, -20, 60), r('K1AA', 900, -12, 300)]
const getout: GettingOut = { count: REPORTS.length, maxKm: 15000, reports: REPORTS }

function mount(instance: string, g: GettingOut = getout) {
  const ctx = { getout: g, onSelectCall: vi.fn(), units: 'metric' } as unknown as PaneContext
  return render(<PaneBody pane="getout" ctx={ctx} instance={instance} />)
}
const shown = (c: HTMLElement) => [...c.querySelectorAll('.getout-list .go-call')].map((x) => x.textContent)

beforeEach(() => localStorage.clear())
afterEach(cleanup)

describe('the Getting Out sort bar', () => {
  it('is a named radio group of the four orders, Distance chosen, the list in the station order', () => {
    const { container } = mount('left1')
    const group = screen.getByRole('radiogroup', { name: 'Sort the stations hearing you' })
    const radios = within(group).getAllByRole('radio')
    expect(radios.map((x) => x.getAttribute('aria-label') ?? x.closest('label')?.textContent)).toEqual([
      'Distance',
      'Call A–Z',
      'Newest',
      'SNR',
    ])
    expect((within(group).getByRole('radio', { name: 'Distance' }) as HTMLInputElement).checked).toBe(true)
    expect(shown(container)).toEqual(['VK4DD', 'JA3CC', 'DL2BB', 'K1AA'])
  })

  it('stands above the list, inside the box', () => {
    const { container } = mount('left1')
    const bar = container.querySelector('[role="radiogroup"]')!
    const list = container.querySelector('.getout-list')!
    expect(bar.compareDocumentPosition(list) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  })

  it.each([
    ['Call A–Z', ['DL2BB', 'JA3CC', 'K1AA', 'VK4DD']],
    ['Newest', ['VK4DD', 'DL2BB', 'K1AA', 'JA3CC']],
    ['SNR', ['JA3CC', 'K1AA', 'DL2BB', 'VK4DD']],
    ['Distance', ['VK4DD', 'JA3CC', 'DL2BB', 'K1AA']],
  ])('%s reorders the list', (name, order) => {
    const { container } = mount('left1')
    fireEvent.click(screen.getByRole('radio', { name }))
    expect((screen.getByRole('radio', { name }) as HTMLInputElement).checked).toBe(true)
    expect(shown(container)).toEqual(order)
  })

  it('a live update keeps the chosen order', () => {
    const ctx = { getout, onSelectCall: vi.fn(), units: 'metric' } as unknown as PaneContext
    const { container, rerender } = render(<PaneBody pane="getout" ctx={ctx} instance="left1" />)
    fireEvent.click(screen.getByRole('radio', { name: 'Call A–Z' }))
    const more = [...REPORTS, r('EA1AA', 2000, -1, 10)]
    rerender(<PaneBody pane="getout" ctx={{ ...ctx, getout: { count: 5, maxKm: 15000, reports: more } }} instance="left1" />)
    expect(shown(container)).toEqual(['DL2BB', 'EA1AA', 'JA3CC', 'K1AA', 'VK4DD'])
  })

  it('each box keeps its own choice across a restart', () => {
    mount('left1')
    fireEvent.click(screen.getByRole('radio', { name: 'SNR' }))
    cleanup()
    const a = mount('left1')
    expect((screen.getByRole('radio', { name: 'SNR' }) as HTMLInputElement).checked).toBe(true)
    expect(shown(a.container)).toEqual(['JA3CC', 'K1AA', 'DL2BB', 'VK4DD'])
    cleanup()
    // Another box — a cockpit's — never took that choice.
    const b = mount('box2')
    expect((screen.getByRole('radio', { name: 'Distance' }) as HTMLInputElement).checked).toBe(true)
    expect(shown(b.container)).toEqual(['VK4DD', 'JA3CC', 'DL2BB', 'K1AA'])
  })

  it('a saved value this build does not know opens on Distance', () => {
    localStorage.setItem('nexus.connect.getout.sort', JSON.stringify({ left1: 'loudest' }))
    const { container } = mount('left1')
    expect((screen.getByRole('radio', { name: 'Distance' }) as HTMLInputElement).checked).toBe(true)
    expect(shown(container)).toEqual(['VK4DD', 'JA3CC', 'DL2BB', 'K1AA'])
  })

  it('is keyboard operable: one Tab stop, the arrows choose', () => {
    mount('left1')
    const radios = screen.getAllByRole('radio') as HTMLInputElement[]
    // Native radios of one named group: the browser gives the group one Tab stop and the arrows move
    // the choice. All four share a name, and only this box's group uses it.
    const names = new Set(radios.map((x) => x.name))
    expect(names.size).toBe(1)
    expect([...names][0]).not.toBe('')
    for (const x of radios) expect(x.type).toBe('radio')
  })

  it('two boxes on one screen never share a radio group', () => {
    const ctx = { getout, onSelectCall: vi.fn(), units: 'metric' } as unknown as PaneContext
    render(
      <>
        <PaneBody pane="getout" ctx={ctx} instance="left1" />
        <PaneBody pane="getout" ctx={ctx} instance="rail1" />
      </>,
    )
    const groups = screen.getAllByRole('radiogroup')
    const names = groups.map((g) => (within(g).getAllByRole('radio')[0] as HTMLInputElement).name)
    expect(names[0]).not.toBe(names[1])
  })

  it('shows no bar when nobody hears you', () => {
    mount('left1', { count: 0, maxKm: 0, reports: [] })
    expect(screen.queryByRole('radiogroup')).toBeNull()
  })
})
