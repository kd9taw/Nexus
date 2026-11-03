// @vitest-environment jsdom
//
// The space-weather chip stack on a narrow map: at most two chips, the rest behind one "+N more"
// chip that opens them all in a list (a portaled dialog), and no such chip when everything fits.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'

const api = vi.hoisted(() => ({ getArrivalOutlook: vi.fn() }))
vi.mock('../../api', () => api)

import { WxChipStack, WX_STACK_NARROW_SHOWN } from './WxChipStack'
import { t } from '../../i18n'
import type { ArrivalOutlook } from '../../types'

const T0 = 1_793_404_800 // 2026-10-31 00:00 UTC
const NONE: ArrivalOutlook = { watch: null, model: null, l1: null, rationale: null }
const WATCH = {
  serial: 1134,
  code: 'WATA20',
  issued: T0 - 5 * 3600,
  days: [],
  comment: null,
  window: [T0, T0 + 2 * 86_400] as [number, number],
  maxG: 1,
}
const MODEL = {
  runAt: T0 - 3600,
  fetchedAt: T0 - 1800,
  peak: { at: T0 + 20 * 3600, speed: 517.4, density: 3, cloud: false },
  arrival: null,
}

afterEach(() => {
  cleanup()
  vi.useRealTimers()
  api.getArrivalOutlook.mockReset()
})

const chip = (name: string) => ({ key: name, node: <div className="test-chip">{name}</div> })
const FOUR = [chip('flare'), chip('cap'), chip('aurora'), chip('extra')]

async function mount(compact: boolean, chips: Parameters<typeof WxChipStack>[0]['chips'], outlook: ArrivalOutlook = NONE) {
  vi.useFakeTimers({ toFake: ['Date'] })
  vi.setSystemTime((T0 - 21 * 3600) * 1000)
  api.getArrivalOutlook.mockResolvedValue(outlook)
  const r = render(<WxChipStack compact={compact} chips={chips} />)
  await act(async () => {})
  return r
}

const stack = (c: HTMLElement) => c.querySelector('.wx-chip-stack') as HTMLElement
const inStack = (c: HTMLElement) => [...stack(c).querySelectorAll('.test-chip')].map((e) => e.textContent)
const moreChip = (c: HTMLElement) => stack(c).querySelector('.wx-chip-more') as HTMLButtonElement | null

describe('WxChipStack on a narrow map', () => {
  it('shows the first two chips and counts the rest on one "+N more" chip', async () => {
    const { container } = await mount(true, FOUR)
    expect(WX_STACK_NARROW_SHOWN).toBe(2)
    expect(inStack(container)).toEqual(['flare', 'cap'])
    const more = moreChip(container)
    expect(more, 'the overflow chip').not.toBeNull()
    expect(more!.textContent).toBe(t('map.wx.more', { n: 2 }))
    expect(more!.getAttribute('aria-label')).toBe(t('map.wx.more.aria', { n: 2 }))
    // It is the stack's last child, under the chips it stands for.
    expect(stack(container).lastElementChild).toBe(more)
  })

  it('opens the chips it hides in a portaled list, and gives the keyboard back when closed', async () => {
    const { container } = await mount(true, FOUR)
    const more = moreChip(container)!
    expect(more.getAttribute('aria-haspopup')).toBe('dialog')
    expect(more.getAttribute('aria-expanded')).toBe('false')
    act(() => more.focus())
    fireEvent.click(more)
    const dialog = await screen.findByRole('dialog')
    expect(container.contains(dialog), 'the list is portaled out of the map').toBe(false)
    expect(dialog.classList.contains('wx-more-dialog')).toBe(true)
    expect(more.getAttribute('aria-expanded')).toBe('true')
    const listed = [...dialog.querySelectorAll('.test-chip')].map((e) => e.textContent)
    expect(listed).toEqual(['aurora', 'extra'])
    expect(dialog.textContent).toContain(t('map.wx.more.title'))
    // Escape closes it, and the keyboard goes back to the chip that opened it.
    fireEvent.keyDown(document.activeElement!, { key: 'Escape' })
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    await waitFor(() => expect(document.activeElement).toBe(more))
  })

  it('a mouse open that did not focus the chip (macOS WebKit) still gives the keyboard back to it', async () => {
    const { container } = await mount(true, FOUR)
    const more = moreChip(container)!
    // WebKit does not focus a button on click: the keyboard is still on the page when it opens.
    expect(document.activeElement).toBe(document.body)
    fireEvent.click(more)
    await screen.findByRole('dialog')
    fireEvent.keyDown(document.activeElement!, { key: 'Escape' })
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    await waitFor(() => expect(document.activeElement).toBe(more))
  })

  it('shows no "+N more" chip when everything fits', async () => {
    const { container } = await mount(true, FOUR.slice(0, 2))
    expect(inStack(container)).toEqual(['flare', 'cap'])
    expect(moreChip(container)).toBeNull()
  })

  it('skips the chips that are not showing (false / null) before it counts', async () => {
    const { container } = await mount(true, [chip('flare'), false, null, chip('cap')])
    expect(inStack(container)).toEqual(['flare', 'cap'])
    expect(moreChip(container)).toBeNull()
  })

  it('counts the storm-watch chip when it has something to say, and lists it whole', async () => {
    const { container } = await mount(true, FOUR.slice(0, 2), { ...NONE, watch: WATCH, model: MODEL })
    // Still two chips on the map; the watch is the one behind "+1 more".
    expect(inStack(container)).toEqual(['flare', 'cap'])
    expect(stack(container).querySelector('.arrival-chip')).toBeNull()
    const more = moreChip(container)!
    expect(more.textContent).toBe(t('map.wx.more', { n: 1 }))
    fireEvent.click(more)
    const dialog = await screen.findByRole('dialog')
    const arrival = dialog.querySelector('.arrival-chip')!
    expect(arrival.textContent).toContain('G1 watch')
    // In the list it is the full chip (every line), not the narrow map's headline only.
    expect(arrival.classList.contains('compact')).toBe(false)
    expect(arrival.textContent).toContain('517 km/s')
  })

  it('a quiet storm-watch feed adds nothing to the count', async () => {
    const { container } = await mount(true, FOUR.slice(0, 2), NONE)
    expect(moreChip(container)).toBeNull()
  })
})

describe('WxChipStack on a wide map', () => {
  it('shows every chip and no "+N more" chip', async () => {
    const { container } = await mount(false, FOUR, { ...NONE, watch: WATCH })
    expect(inStack(container)).toEqual(['flare', 'cap', 'aurora', 'extra'])
    expect(stack(container).querySelector('.arrival-chip:not(.compact)')).not.toBeNull()
    expect(moreChip(container)).toBeNull()
  })
})
