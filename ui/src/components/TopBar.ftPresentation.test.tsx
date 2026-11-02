// @vitest-environment jsdom
//
// FT PRESENTATION (the 1080p pass, 2026-11-01): the top bar's FT controls look more compact at md and
// lg, and NOTHING ELSE about them changes. This file is the proof for the FT/TX review:
//   · the Tx cycle (Auto / Tx 1st / Tx 2nd) is the same three buttons in the same order at every size,
//     each with the same accessible name (its small caption stays for the ear, `.sr-only`, and only
//     leaves the screen at md and lg), the same tooltip, the same pressed/derived state, and each click
//     calls the same handler with the same argument as before;
//   · the eleven tiers are every one reachable at every size — as pills at xl and below md, through the
//     folded menu at md and lg — each picking exactly the tier it names, in the same order;
//   · the bar's TX cluster (TX On/Off, Tune, Stop TX, Hold Tx) is unchanged at every size.
import { describe, it, expect, vi, afterEach, beforeAll } from 'vitest'
import { render, screen, cleanup, fireEvent, within, waitFor } from '@testing-library/react'
import { TopBar } from './TopBar'
import type { RadioStatus, Tier } from '../types'

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  appVersion: async () => '1.0.0',
}))

vi.setConfig({ testTimeout: 15_000 })

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(() => {
  cleanup()
  delete document.documentElement.dataset.viewport
})

const SIZES = ['sm', 'md', 'lg', 'xl'] as const
const FOLDS = new Set(['md', 'lg'])
const TIERS: Array<[Tier, string]> = [
  ['TempoFast', 'Fast'], ['TempoDeep', 'Deep'], ['FT4', 'FT4'], ['FT8', 'FT8'], ['FT2', 'FT2'], ['WSPR', 'WSPR'],
  ['Q65', 'Q65'], ['MSK144', 'MSK144'], ['JT65', 'JT65'], ['FST4', 'FST4'], ['FST4W', 'FST4W'],
]

function bar(radio: Partial<RadioStatus>, calls: string[], ft = false) {
  const r = {
    dialMhz: 14.074, band: '20m', sideband: 'USB', rigMode: 'USB', rigConfirmed: true, nextSlotMs: 0,
    txEven: true, txCycleAuto: false, txEnabled: false, txAllowed: true, transmitting: false, tuning: false,
    qsoRecording: false, catOk: true, dtSec: 0, operatingMode: 'digital', holdTxFreq: false, ...radio,
  } as unknown as RadioStatus
  const log = (name: string) => (v?: unknown) => calls.push(v === undefined ? name : `${name}(${String(v)})`)
  return render(
    <div className="app">
      <TopBar
        mycall="N0CALL" mygrid="EN52xa" radio={r} link={{ tier: 'FT8', dtSec: 0 } as never} bandPlan={[]}
        onSetFrequency={log('setFrequency')} onSetTxEnabled={log('setTxEnabled')} onSetTune={log('setTune')}
        onHaltTx={() => calls.push('haltTx')} onSetTxEven={log('setTxEven')} onSetTxCycleAuto={log('setTxCycleAuto')}
        onSetHoldTxFreq={log('setHoldTxFreq')} tier="FT8" onTierChange={log('tier')} onOpenGuide={() => {}}
        field={false} onFieldChange={() => {}} fdActive hideTxControls={ft} hideFrequencyControl={ft}
        hideDigitalChrome={false} ftScreen={ft}
      />
    </div>,
  )
}

const cycle = (c: HTMLElement) => [...c.querySelectorAll('.tx-period > .tier-btn')] as HTMLButtonElement[]
const names = (els: Element[]) => els.map((e) => (e.textContent ?? '').replace(/\s+/g, ' ').trim())

describe('FT presentation: the Tx cycle is the same three controls at every size', () => {
  it.each(SIZES)('at %s: names, tooltips, order and the handler each click calls are unchanged', (vp) => {
    document.documentElement.dataset.viewport = vp
    const calls: string[] = []
    const { container } = bar({ txCycleAuto: false, txEven: true }, calls)
    const btns = cycle(container)
    expect(names(btns)).toEqual(['Auto cycle', 'Tx 1st even', 'Tx 2nd odd'])
    // The accessible names are whole at every size (the caption leaves the screen, not the name).
    for (const [i, name] of ['Auto cycle', 'Tx 1st even', 'Tx 2nd odd'].entries())
      expect(screen.getByRole('button', { name }), `${vp}: ${name}`).toBe(btns[i])
    for (const b of btns) expect(b.title, `${vp}: ${b.textContent} keeps its tooltip`).toBeTruthy()
    // Only the presentation folds: the captions are screen-reader-only at md and lg, drawn elsewhere.
    for (const s of container.querySelectorAll('.tx-period small'))
      expect(s.classList.contains('sr-only'), `${vp}: caption`).toBe(FOLDS.has(vp))
    // State: Tx 1st is the operator's lock here.
    expect(btns.map((b) => b.getAttribute('aria-pressed'))).toEqual(['false', 'true', 'false'])
    btns.forEach((b) => fireEvent.click(b))
    expect(calls).toEqual(['setTxCycleAuto(true)', 'setTxEven(true)', 'setTxEven(false)'])
  })

  it.each(SIZES)('on the FT screen (the bar operators use with Operate) at %s: the same three, the same calls', (vp) => {
    document.documentElement.dataset.viewport = vp
    const calls: string[] = []
    const { container } = bar({ txCycleAuto: false, txEven: false }, calls, true)
    expect(container.querySelector('header.topbar')!.classList.contains('topbar--ft')).toBe(true)
    const btns = cycle(container)
    expect(names(btns)).toEqual(['Auto cycle', 'Tx 1st even', 'Tx 2nd odd'])
    for (const name of ['Auto cycle', 'Tx 1st even', 'Tx 2nd odd']) expect(screen.getByRole('button', { name }), `${vp}: ${name}`).toBeTruthy()
    expect(btns.map((b) => b.getAttribute('aria-pressed'))).toEqual(['false', 'false', 'true'])
    btns.forEach((b) => fireEvent.click(b))
    expect(calls).toEqual(['setTxCycleAuto(true)', 'setTxEven(true)', 'setTxEven(false)'])
  })

  it.each(SIZES)('at %s: under Auto the derived side is marked as before, and the lock is not', (vp) => {
    document.documentElement.dataset.viewport = vp
    const { container } = bar({ txCycleAuto: true, txEven: false }, [])
    const btns = cycle(container)
    expect(names(btns)).toEqual(['Auto 2nd', 'Tx 1st even', 'Tx 2nd odd'])
    expect(btns.map((b) => b.getAttribute('aria-pressed'))).toEqual(['true', 'false', 'false'])
    expect(btns.map((b) => b.classList.contains('derived'))).toEqual([false, false, true])
  })
})

describe('FT presentation: every tier is reachable at every size and picks exactly the tier it names', () => {
  it.each(SIZES)('at %s', async (vp) => {
    document.documentElement.dataset.viewport = vp
    const calls: string[] = []
    const { container } = bar({}, calls)
    if (vp === 'sm') {
      // Off the FT screens the small size drops the tiers from the bar (unchanged, TopBar.small.test.tsx).
      return
    }
    if (FOLDS.has(vp)) {
      const trigger = container.querySelector('.tier-menu-folded') as HTMLButtonElement
      for (const [tier, label] of TIERS) {
        fireEvent.keyDown(trigger, { key: 'Enter' })
        const menu = await screen.findByRole('menu')
        const item = within(menu).getAllByRole('menuitem').find((i) => (i.textContent ?? '').endsWith(label))!
        expect(item, `${vp}: ${label} in the menu`).toBeTruthy()
        fireEvent.click(item)
        await waitFor(() => expect(screen.queryByRole('menu')).toBeNull())
        expect(calls[calls.length - 1], `${vp}: ${label}`).toBe(`tier(${tier})`)
      }
    } else {
      const pills = [...container.querySelectorAll('.tier-toggle:not(.tx-period) > .tier-btn[aria-pressed]')] as HTMLButtonElement[]
      expect(pills).toHaveLength(11)
      pills.forEach((p) => fireEvent.click(p))
      expect(calls).toEqual(TIERS.map(([tier]) => `tier(${tier})`))
    }
  })
})

describe('FT presentation: the bar’s TX cluster is untouched at every size', () => {
  it.each(SIZES)('at %s: TX Off, Tune, Stop TX, Hold Tx, each calling its own handler', (vp) => {
    document.documentElement.dataset.viewport = vp
    const calls: string[] = []
    const { container } = bar({}, calls)
    const btns = [...container.querySelectorAll('.op-controls > button')] as HTMLButtonElement[]
    expect(names(btns)).toEqual(['TX Off', 'Tune', 'Stop TX', 'Hold Tx'])
    btns.forEach((b) => fireEvent.click(b))
    expect(calls).toEqual(['setTxEnabled(true)', 'setTune(true)', 'haltTx', 'setHoldTxFreq(true)'])
  })
})
