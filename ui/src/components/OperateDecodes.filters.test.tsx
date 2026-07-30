// @vitest-environment jsdom
//
// The Band Activity filter chip comes back from storage on mount — and the Rx Frequency
// panes, which are the SAME component locked to the 'rx' filter, must not write over it.
// That is the one way a single shared key could go wrong here, so it is pinned.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { OperateDecodes } from './OperateDecodes'
import { DECODE_FILTER_KEY, loadDecodeFilter } from '../operateFilters'
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
  rv: 0, // decoded from the initial transmission, no HARQ combining
  ...over,
})

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

const chip = (label: string) => screen.getByRole('button', { name: label })
const pressed = (label: string) => chip(label).getAttribute('aria-pressed')

beforeEach(() => localStorage.clear())
afterEach(cleanup)

describe('Band Activity chip initializes from storage', () => {
  it('defaults to All when nothing is stored', () => {
    mount()
    expect(pressed('All')).toBe('true')
    expect(pressed('CQ')).toBe('false')
  })

  it('comes up on the stored chip', () => {
    localStorage.setItem(DECODE_FILTER_KEY, 'b4')
    mount()
    expect(pressed('B4')).toBe('true')
    expect(pressed('All')).toBe('false')
  })

  it('falls back to All on an unknown stored chip', () => {
    localStorage.setItem(DECODE_FILTER_KEY, 'dxped')
    mount()
    expect(pressed('All')).toBe('true')
  })

  it('writes the chip the operator clicks, and survives a remount', () => {
    mount()
    fireEvent.click(chip('CQ'))
    expect(loadDecodeFilter()).toBe('cq')
    cleanup()
    mount() // a fresh component, as after a restart
    expect(pressed('CQ')).toBe('true')
  })
})

describe('the locked Rx Frequency pane cannot clobber the Band Activity chip', () => {
  it('renders no filter bar and leaves the stored chip untouched', () => {
    localStorage.setItem(DECODE_FILTER_KEY, 'cq')
    // Exactly how OperateCockpit mounts the Rx Frequency pane.
    mount({ lockedFilter: 'rx', compact: true, title: 'Rx Frequency' })
    expect(screen.queryByRole('button', { name: 'All' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'On RX' })).toBeNull()
    expect(loadDecodeFilter()).toBe('cq')
  })
})
