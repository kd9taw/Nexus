// @vitest-environment jsdom
//
// A TIER PILL SAYS RECEIVE-ONLY ONLY FOR A TIER THAT CANNOT TRANSMIT.
//
// The JT65 pill carried its own receive-only flag: an RX tag, the dashed receive-only style and
// a tooltip saying "Receive only in this build", long after JT65 gained its transmitter and TX On
// worked on it. An operator who believed the pill could arm, or leave TX armed, and be surprised
// when it keyed. The pill now reads the one receive-only list (`isRxOnly`, which mirrors the
// modes' declared capability) instead of a flag of its own.
// CONTROL: a tier on that list still gets the RX tag, the style and a tooltip that says so.
import { describe, it, expect, beforeAll, afterEach, vi } from 'vitest'
import { render, cleanup, screen } from '@testing-library/react'
import type { RadioStatus, Tier } from '../types'

/** Q65 stands in for the next receive-only mode: the list is empty while every mode transmits. */
const rxOnly = vi.hoisted(() => ({ tiers: [] as string[] }))
vi.mock('../types', async (orig) => {
  const real = await orig<typeof import('../types')>()
  return { ...real, isRxOnly: (t: Tier | null | undefined) => !!t && rxOnly.tiers.includes(t) }
})

import { TopBar } from './TopBar'

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(() => {
  cleanup()
  rxOnly.tiers = []
})

const radio = {
  dialMhz: 14.076, band: '20m', sideband: 'USB', rigMode: 'USB', rigConfirmed: true,
  nextSlotMs: 0, txCycleAuto: true, txEven: true, txEnabled: false, txAllowed: true,
  transmitting: false, tuning: false, txBusyReason: null, rigKeyed: false,
  qsoRecording: false, catOk: true, dtSec: 0, clockOffsetMs: 0,
} as unknown as RadioStatus

/** The bar on FT8 with its mode pills drawn; the pill named `name`. */
function pill(name: string) {
  const noop = () => {}
  render(
    <TopBar
      mycall="W9ABC" mygrid="EN52xa" radio={radio} link={{ tier: 'FT8' } as never} bandPlan={[]}
      onSetFrequency={noop} onSetTxEnabled={noop} onSetTune={noop} onHaltTx={noop}
      onSetTxEven={noop} onSetTxCycleAuto={noop} onSetHoldTxFreq={noop}
      tier="FT8" onTierChange={noop} onOpenGuide={noop}
    />,
  )
  const group = screen.getByRole('group', { name: 'Link tier' })
  const found = [...group.querySelectorAll('button')].find(
    (b) => b.lastChild?.textContent === name,
  ) as HTMLButtonElement
  expect(found, `the ${name} pill rendered`).toBeTruthy()
  return found
}

const RECEIVE_ONLY = /receive.only/i

describe('the receive-only mark on the tier pills', () => {
  it('JT65 transmits: its pill has no RX tag, no receive-only style, no receive-only tooltip', () => {
    const p = pill('JT65')
    expect(p.querySelector('small')?.textContent ?? '').not.toBe('RX')
    expect(p.classList.contains('rx-only')).toBe(false)
    expect(p.title).not.toMatch(RECEIVE_ONLY)
    expect(p.title).toMatch(/Transmit and receive/)
  })

  it('CONTROL: a receive-only tier still says so: RX tag, style and tooltip', () => {
    rxOnly.tiers = ['Q65']
    const p = pill('Q65')
    expect(p.querySelector('small')?.textContent).toBe('RX')
    expect(p.classList.contains('rx-only')).toBe(true)
    expect(p.title).toMatch(RECEIVE_ONLY)
  })
})
