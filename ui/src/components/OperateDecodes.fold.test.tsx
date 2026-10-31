// @vitest-environment jsdom
//
// A SHORT Band Activity folds its filter row behind one "Filters" button in the title row, so
// the decode list keeps its rows. At the 1024×768 floor, FT's Roster rail gives the pane about
// ten lines of height, and the wrapped chip row took three of them: the list was left with one
// row. These pin what the operator gets in both states — the chips folded away only when the pane
// is short, every one of them still working once the button opens them, and a fold that never
// hides the fact that a filter is lit.
//
// jsdom lays nothing out, so the pane's height comes from a stand-in ResizeObserver that reports
// the height each case names. Real-Chrome geometry is measured separately.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, fireEvent, cleanup, act } from '@testing-library/react'
import { OperateDecodes } from './OperateDecodes'
import { DECODE_FILTER_KEY } from '../operateFilters'
import type { DecodeRow } from '../types'

vi.mock('../api', () => ({ openQrzPage: vi.fn() }))

const decode = (over: Partial<DecodeRow> = {}): DecodeRow => ({
  from: 'W1AW',
  snr: -12,
  dtSec: 0.2,
  freqHz: 1200,
  message: 'CQ W1AW FN31',
  isCq: true,
  directedToMe: false,
  worked: false,
  tier: 'FT8',
  rv: 0,
  ...over,
})

// The pane's font size in jsdom is the 16px default, so 1em = 16px below.
const EM = 16
let paneHeight = 0
const observers: { cb: ResizeObserverCallback; el: Element }[] = []

class FakeResizeObserver {
  constructor(private cb: ResizeObserverCallback) {}
  observe(el: Element) {
    observers.push({ cb: this.cb, el })
    this.cb([], this as unknown as ResizeObserver)
  }
  unobserve() {}
  disconnect() {}
}

/** Resize every observed pane to `h` px and deliver the notice, as a window resize would. */
function resizeTo(h: number) {
  paneHeight = h
  act(() => {
    for (const o of observers) o.cb([], {} as ResizeObserver)
  })
}

function mount(props: Partial<React.ComponentProps<typeof OperateDecodes>> = {}) {
  return render(
    <OperateDecodes
      decodes={[decode()]}
      slot={100}
      rxOffsetHz={1200}
      band="20m"
      tier="FT8"
      harqRescues={0}
      onCall={() => {}}
      {...props}
    />,
  )
}

const fold = () => screen.queryByRole('button', { name: /^Filters/ })
const chip = (label: string) => screen.queryByRole('button', { name: label })

beforeEach(() => {
  localStorage.clear()
  observers.length = 0
  paneHeight = 0
  vi.stubGlobal('ResizeObserver', FakeResizeObserver)
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockImplementation(function (this: HTMLElement) {
    return this.classList.contains('operate-decodes') ? paneHeight : 0
  })
})
afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

describe('Band Activity folds its filter row only when the pane is short', () => {
  it('a tall pane shows every chip and no Filters button', () => {
    paneHeight = 30 * EM
    mount()
    expect(fold()).toBeNull()
    expect(chip('All')).not.toBeNull()
    expect(chip('Erase')).not.toBeNull()
    expect(screen.getByRole('combobox', { name: /sort/i })).toBeTruthy()
  })

  it('a short pane folds the chips behind a closed Filters button in the title row', () => {
    paneHeight = 10 * EM
    const { container } = mount()
    const b = fold()
    expect(b).not.toBeNull()
    expect(b!.getAttribute('aria-expanded')).toBe('false')
    expect(b!.closest('.od-titlerow')).not.toBeNull()
    expect(chip('All')).toBeNull()
    expect(chip('Erase')).toBeNull()
    expect(container.querySelector('.od-controls')).toBeNull()
    // The search box and the status row stay where they were.
    expect(screen.getByRole('searchbox', { name: 'Search Band Activity' }).closest('.od-titlerow')).not.toBeNull()
    expect(container.querySelector('.od-status')?.textContent).toMatch(/1 heard/)
  })

  it('opening the button gives back every chip, and each still works by mouse', () => {
    paneHeight = 10 * EM
    mount()
    fireEvent.click(fold()!)
    expect(fold()!.getAttribute('aria-expanded')).toBe('true')
    const controls = document.getElementById(fold()!.getAttribute('aria-controls')!)
    expect(controls?.classList.contains('od-controls')).toBe(true)
    fireEvent.click(chip('CQ')!)
    expect(chip('CQ')!.getAttribute('aria-pressed')).toBe('true')
    expect(chip('All')!.getAttribute('aria-pressed')).toBe('false')
    // Closing again folds them away, and the filter stays lit.
    fireEvent.click(fold()!)
    expect(chip('CQ')).toBeNull()
    expect(localStorage.getItem(DECODE_FILTER_KEY)).toContain('cq')
  })

  it('the Filters button is a real button, so the keyboard reaches and works it', () => {
    paneHeight = 10 * EM
    mount()
    const b = fold()!
    expect(b.tagName).toBe('BUTTON')
    expect(b.getAttribute('type')).toBe('button')
    expect(b.hasAttribute('disabled')).toBe(false)
    expect(b.tabIndex).toBe(0)
  })

  it('a folded row says when a filter is lit, and how many', () => {
    localStorage.setItem(DECODE_FILTER_KEY, 'cq')
    paneHeight = 10 * EM
    mount()
    expect(fold()!.textContent).toMatch(/1/)
    expect(fold()!.classList.contains('active')).toBe(true)
  })

  it('a folded row with nothing lit is not drawn lit', () => {
    paneHeight = 10 * EM
    mount()
    expect(fold()!.classList.contains('active')).toBe(false)
    expect(fold()!.textContent).not.toMatch(/\d/)
  })

  it('follows the pane as it is resized, both ways', () => {
    paneHeight = 30 * EM
    mount()
    expect(fold()).toBeNull()
    resizeTo(10 * EM)
    expect(fold()).not.toBeNull()
    expect(chip('All')).toBeNull()
    resizeTo(30 * EM)
    expect(fold()).toBeNull()
    expect(chip('All')).not.toBeNull()
  })

  it('a hidden pane (0 px, a keep-alive host) keeps the state it had', () => {
    paneHeight = 30 * EM
    mount()
    resizeTo(0)
    expect(fold()).toBeNull()
    expect(chip('All')).not.toBeNull()
  })

  it('the compact Rx Frequency pane never draws the button', () => {
    paneHeight = 5 * EM
    mount({ compact: true, lockedFilter: 'rx' })
    expect(fold()).toBeNull()
  })
})

describe('a folded row never hides a call-hide that is hiding rows', () => {
  it('the Filters button lights while the wildcard call-hide is hiding rows', () => {
    localStorage.setItem('nexus.decodes.hideCalls', 'JA*')
    paneHeight = 10 * EM
    const { container } = mount({ decodes: [decode(), decode({ from: 'JA1ABC', message: 'CQ JA1ABC PM95' })] })
    expect(container.querySelector('.od-status')?.textContent).toMatch(/1 heard/)
    expect(fold()!.classList.contains('active')).toBe(true)
  })
})
