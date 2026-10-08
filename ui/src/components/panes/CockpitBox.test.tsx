// @vitest-environment jsdom
//
// A BOX IN A COCKPIT (any pane in any area, 2026-10-07): one entry of the shared list in the cockpit
// pane frame, with the entry's role, a picker in its head, its ✕ and the Conditions box's own body —
// and a selection of the box's own, never the window's: a click in a box beside the log never
// re-targets the station a CW macro's `!` sends.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { act, cleanup, fireEvent, render, renderHook, screen } from '@testing-library/react'
import type { SpotRow } from '../../types'

vi.mock('../../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => null) : actual[k]
  return {
    ...auto,
    getSettings: vi.fn(async () => ({ rotatorModel: 0, rotatorHost: '' })),
    readRotatorState: vi.fn(async () => null),
    getOpeningsLog: vi.fn(async () => []),
    getContests: vi.fn(async () => []),
  }
})

import { CockpitBox, useBoxSelection, type BoxSelection, type BoxSource } from './CockpitBox'
import { SHARED_PANES } from '../../features/sharedPanes'
import { PANE_CATEGORIES, paneById } from '../connect/panes'
import { t } from '../../i18n'

afterEach(cleanup)

const SPOT = {
  call: 'K1CW', entity: 'United States', zone: 5, state: null, band: '20m', freqMhz: 14.025, mode: 'CW', submode: 'CW',
  spotter: 'W3LPL', corroborators: [], ageSecs: 30, comment: 'up 1', licensed: true, spotterLocal: true,
} as unknown as SpotRow

function source(over: Partial<BoxSource> = {}): BoxSource {
  return { myGrid: 'EN52', theme: 'dark', stations: [], prop: null, needByCall: new Map(), ...over }
}
function selection(over: Partial<BoxSelection> = {}): BoxSelection {
  return { selectedCall: null, onSelectCall: () => {}, focusBand: null, toggleFocusBand: () => {}, intent: 'dx', ...over }
}

async function box(entry: string, over: Partial<Parameters<typeof CockpitBox>[0]> = {}) {
  const props = { box: 'box1', entry, source: source(), selection: selection(), onScreen: () => false, onPick: () => {}, stacked: false, ...over }
  const r = render(<CockpitBox {...props} />)
  await act(async () => {})
  return r
}
const frame = () => document.querySelector('.pane-frame[data-pane="box1"]') as HTMLElement
const title = (entry: string) => paneById(SHARED_PANES.find((e) => e.id === entry)!.pane)!.title

describe('a box in a cockpit', () => {
  it('stands in the cockpit pane frame with its entry’s name and role: a reading is its own height, a list fills', async () => {
    await box('clock')
    expect(frame().getAttribute('aria-label')).toBe(title('clock'))
    expect(frame().getAttribute('data-fit')).toBe('content')
    cleanup()
    await box('getout')
    expect(frame().getAttribute('data-fit')).toBe('fill')
  })

  it('its body is the Conditions box’s own, one line included when it has nothing', async () => {
    await box('rotor')
    expect(frame().querySelector('.box-body')?.textContent).toBe(t('connect.pane.rotor.basic'))
  })

  it('its head picks from the whole shared list, in the Conditions picker’s groups, marking what is on screen elsewhere', async () => {
    await box('clock', { onScreen: (e: string) => e === 'spacewx' })
    const pick = screen.getByRole('combobox', { name: t('panels.box.pick.aria', { title: title('clock') }) }) as HTMLSelectElement
    expect([...pick.options].map((o) => o.value).sort()).toEqual(SHARED_PANES.map((e) => e.id).sort())
    expect([...pick.querySelectorAll('optgroup')]).toHaveLength(PANE_CATEGORIES.length)
    expect(pick.value).toBe('clock')
    const label = (v: string) => [...pick.options].find((o) => o.value === v)!.textContent
    expect(label('spacewx')).toBe(t('panels.box.pick.onScreen', { title: title('spacewx') }))
    expect(label('getout')).toBe(title('getout'))
    expect(label('clock'), 'what this box shows is not "elsewhere"').toBe(title('clock'))
  })

  it('picking hands the entry to the cockpit; ✕ hides the box, with no note: its hide ends nothing', async () => {
    const onPick = vi.fn()
    const onRemove = vi.fn()
    await box('clock', { onPick, onRemove })
    fireEvent.change(screen.getByRole('combobox'), { target: { value: 'pota' } })
    expect(onPick).toHaveBeenCalledWith('pota')
    const close = screen.getByRole('button', { name: t('pane.hide.aria', { title: title('clock') }) })
    expect(close.getAttribute('aria-describedby')).toBeNull()
    fireEvent.click(close)
    expect(onRemove).toHaveBeenCalledTimes(1)
  })

  it('in the stacking flow a list box is capped and scrolls inside itself; a reading never is', async () => {
    await box('getout', { stacked: true })
    expect(frame().querySelector('.box-body')?.classList.contains('box-body--stacked')).toBe(true)
    cleanup()
    await box('clock', { stacked: true })
    expect(frame().querySelector('.box-body')?.classList.contains('box-body--stacked')).toBe(false)
    cleanup()
    await box('getout', { stacked: false })
    expect(frame().querySelector('.box-body')?.classList.contains('box-body--stacked')).toBe(false)
  })
})

describe('⛔ a click in a box never changes the station the cockpit is working', () => {
  it('a Spots row selects in the box only: the window’s select is never called, the board’s Work is', async () => {
    const appSelect = vi.fn()
    const appWork = vi.fn()
    const boxSelect = vi.fn()
    await box('spotsBoard', {
      source: source({ spotsFeed: { rows: [SPOT], board: { bandPlan: [], selectedCall: null, myGrid: 'EN52', onSelect: appSelect, onWork: appWork } } }),
      selection: selection({ onSelectCall: boxSelect }),
    })
    const row = frame().querySelector('.sp-row') as HTMLElement
    expect(row, 'the Spots board drew no row').not.toBeNull()
    fireEvent.click(row)
    expect(boxSelect).toHaveBeenCalledWith('K1CW')
    expect(appSelect).not.toHaveBeenCalled()
  })

  it('the cockpit’s box selection is a selection of its own', () => {
    const { result } = renderHook(() => useBoxSelection())
    expect(result.current.selectedCall).toBeNull()
    act(() => result.current.onSelectCall('K1CW'))
    expect(result.current.selectedCall).toBe('K1CW')
    act(() => result.current.toggleFocusBand('20m'))
    expect(result.current.focusBand).toBe('20m')
    act(() => result.current.toggleFocusBand('20m'))
    expect(result.current.focusBand).toBeNull()
  })
})
