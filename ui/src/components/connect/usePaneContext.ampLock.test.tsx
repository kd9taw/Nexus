// @vitest-environment jsdom
//
// THE AMPLIFIER BOX'S OPERATE/STANDBY LOCK FAILS CLOSED. Every host lends the box the radio's ON AIR answer
// (`radioOnAir`) beside the amplifier; one that lends the amplifier and forgets the answer must get a dead
// button, never one that is live while the rig is keyed. The backend's refusal stays the real guard; this
// is the courtesy lock's direction of failure.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { cleanup, render } from '@testing-library/react'

vi.mock('../../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(() => new Promise(() => {})) : actual[k]
  return auto
})

import { usePaneContext, type PaneContextInput } from './usePaneContext'
import type { PaneContext } from './paneContext'

afterEach(cleanup)

function read(extra: Partial<PaneContextInput>): PaneContext {
  let got: PaneContext | null = null
  function Probe() {
    got = usePaneContext({
      myGrid: 'EN52',
      theme: 'dark',
      intent: 'dx',
      prop: null,
      needByCall: new Map(),
      rigBand: null,
      selectedCall: null,
      selection: { selStation: null, selSpot: null, selDxped: null, selGrid: null },
      onSelectCall: () => {},
      focusBand: null,
      toggleFocusBand: () => {},
      ...extra,
    } as PaneContextInput).ctx
    return null
  }
  render(<Probe />)
  return got!
}

describe('the Amplifier box’s ON AIR lock, as the box context carries it', () => {
  it('absent reads as ON AIR: a host that forgot to say gets a locked button', () => {
    expect(read({}).radioOnAir).toBe(true)
  })
  it('…and the host’s own answer is carried as given, both ways', () => {
    expect(read({ radioOnAir: false }).radioOnAir).toBe(false)
    cleanup()
    expect(read({ radioOnAir: true }).radioOnAir).toBe(true)
  })
})
