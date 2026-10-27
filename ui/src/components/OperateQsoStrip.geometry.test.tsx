// @vitest-environment jsdom
//
// TX-cluster position stability (the panic-control rule): TX On / Tune / Stop
// TX / Hold Tx / Call CQ / S&P must keep FIXED positions across every QSO/TX
// state — a Stop TX that drifts as the DX call appears or the rig keys is a
// mis-click on the one control that must never be missed. Measured before the
// fix (real Chrome, the tree's sheet): Stop TX x = 376 idle → 648 with a DX
// call → 688 while transmitting.
//
// jsdom computes no layout, so the pin is structural and split in two:
// The TX cluster OPENS the strip, as it opens every other screen's TX strip
// (CockpitTxStrip): the latch, then Tune, so Tune is in the same spot on the FT
// screen as on the other seven. It used to come after Call CQ / S&P, so on FT
// alone it sat one cluster further right.
//   1. HERE: nothing renders before the two button clusters, in any state —
//      every state-variable readout member (state cap, AUTO-CQ pill, sequencer
//      state, DX call, report) lives strictly AFTER the TX cluster;
//   2. operate-classic-grid.test.ts (computed cascade): everything sharing the
//      clusters' row edge resolves state-independent width — .cq-now has a
//      fixed non-growing basis, the TX On/Off toggle and the next-slot
//      countdown carry min-widths, the telemetry cell is a fixed em cell.
// Start-anchored clusters + state-independent widths ⇒ fixed positions.
import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, cleanup } from '@testing-library/react'

import { OperateQsoStrip } from './OperateQsoStrip'
import type { QsoStatus, RadioStatus } from '../types'

afterEach(cleanup)

type StateName = 'idle' | 'qso' | 'tx'

function radioFor(state: StateName): RadioStatus {
  return {
    txEnabled: state !== 'idle',
    transmitting: state === 'tx',
    tuning: false,
    holdTxFreq: false,
    txEven: true,
    txCycleAuto: true,
  } as unknown as RadioStatus
}

function qsoFor(state: StateName): QsoStatus | null {
  if (state === 'idle') return null
  return {
    state: 'AwaitRoger',
    dxcall: 'JA1XYZ',
    rxReport: -15,
    running: true,
    // The AUTO-CQ pill now keys off the real CQ-run flag, not `running` (which is true
    // through plain S&P QSOs too) — this fixture is a running CQ, so both are true.
    cqRunning: true,
    txNow: 'JA1XYZ KD9TAW R-15',
    txCount: 2,
  } as unknown as QsoStatus
}

function renderStrip(state: StateName) {
  return render(
    <OperateQsoStrip
      qso={qsoFor(state)}
      radio={radioFor(state)}
      onSetMode={vi.fn()}
      onCallCq={vi.fn()}
      onResend={vi.fn()}
      onFreetext={vi.fn()}
      onLog={vi.fn()}
      onSetTxEnabled={vi.fn()}
      onSetTune={vi.fn()}
      onHaltTx={vi.fn()}
      onSetHoldTxFreq={vi.fn()}
      onSetTxEven={vi.fn()}
      onSetTxCycleAuto={vi.fn()}
      skipTx1={false}
      onSkipTx1={vi.fn()}
      nextSlotSec={state === 'tx' ? 3 : 12}
      telemetry={<div data-testid="telemetry-node" />}
    />,
  )
}

const STATES: StateName[] = ['idle', 'qso', 'tx']

/** Class-list census of the strip's children up to and including the TX cluster. */
function leadingCensus(container: HTMLElement): string[] {
  const strip = container.querySelector('.cockpit-qso')
  expect(strip, 'no .cockpit-qso strip rendered').not.toBeNull()
  const out: string[] = []
  for (const child of Array.from(strip!.children)) {
    out.push(child.className)
    if (child.classList.contains('cq-roles')) return out
  }
  throw new Error('no .cq-roles cluster inside the strip')
}

describe('the TX cluster is start-anchored: state-variable content never precedes it', () => {
  for (const state of STATES) {
    it(`${state}: the strip opens with the TX cluster then Call CQ/S&P`, () => {
      const { container } = renderStrip(state)
      const census = leadingCensus(container)
      expect(
        census,
        `children before/at the TX cluster in state '${state}' — any state-variable member ` +
          'here moves Stop TX mid-QSO',
      ).toEqual(['op-controls cq-txctl', 'cq-roles'])
    })
  }

  it('Tune is the second control of the strip, right after the TX On/Off latch, as on every other screen', () => {
    const { container } = renderStrip('idle')
    const first = container.querySelector('.cockpit-qso')!.firstElementChild!
    expect(first.classList.contains('cq-txctl'), 'the TX cluster must open the strip').toBe(true)
    expect([...first.children].slice(0, 2).map((e) => e.textContent!.trim())).toEqual(['TX Off', 'Tune'])
  })

  it('every state-variable readout member renders AFTER both button clusters', () => {
    const { container } = renderStrip('tx') // the state where all five exist
    const lastCluster = container.querySelector('.cq-roles')!
    for (const cls of ['cq-statecap', 'cq-autocq', 'cq-state', 'cq-dx', 'cq-rpt']) {
      const m = container.querySelector(`.${cls}`)
      expect(m, `.${cls} missing in the TX state`).not.toBeNull()
      expect(
        lastCluster.compareDocumentPosition(m!) & Node.DOCUMENT_POSITION_FOLLOWING,
        `.${cls} renders before the button clusters — its width would shift the panic controls`,
      ).toBeTruthy()
    }
  })

  it('the leading census is IDENTICAL across all three states', () => {
    const seen = STATES.map((s) => {
      const view = renderStrip(s)
      const census = leadingCensus(view.container).join(' | ')
      cleanup()
      return census
    })
    expect(new Set(seen).size, `census diverges across states: ${JSON.stringify(seen)}`).toBe(1)
  })
})
