// @vitest-environment jsdom
import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { OperateRoster, freshness } from './OperateRoster'
import type { Station } from '../types'

// The declination fetch is the only mount-time engine call; stub it away.
vi.mock('../api', () => ({
  getDeclination: vi.fn(() => Promise.resolve(0)),
}))

function station(call: string, lastHeardSlot: number): Station {
  return {
    call,
    grid: 'EN52',
    snr: -10,
    lastHeardSlot,
    heardCount: 1,
    presence: 'heard' as Station['presence'],
    worked: false,
  }
}

describe('OperateRoster recency window', () => {
  it('shows only stations heard within the last 3 cycles', () => {
    const currentSlot = 100
    const stations = [
      station('FRESH0', 100), // age 0 — this cycle
      station('FRESH3', 97), // age 3 — the window edge, still shown
      station('STALE4', 96), // age 4 — dropped
      station('STALE99', 1), // long gone — dropped
    ]
    render(
      <OperateRoster
        stations={stations}
        myGrid="EN52"
        currentSlot={currentSlot}
        needByCall={new Map()}
        selectedCall={null}
        onSelect={() => {}}
        onCall={() => {}}
      />,
    )
    expect(screen.queryByText('FRESH0')).not.toBeNull()
    expect(screen.queryByText('FRESH3')).not.toBeNull()
    expect(screen.queryByText('STALE4')).toBeNull()
    expect(screen.queryByText('STALE99')).toBeNull()
  })
})

describe('OperateRoster freshness fade', () => {
  it('dims rows as they age toward the drop-off (full when just heard)', () => {
    expect(freshness(0)).toBe(1)
    expect(freshness(3)).toBeCloseTo(0.5) // window edge → floor
    expect(freshness(1)).toBeGreaterThan(freshness(2)) // monotonically dimmer with age
    expect(freshness(99)).toBe(0.5) // never below the readable floor
  })
})
