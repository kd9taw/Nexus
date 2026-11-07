// @vitest-environment jsdom
//
// The Band Outlook pane's engine badge: what it says for each engine, and that a P.533 run that
// failed is named on the pane rather than passing for a normal answer.
import { describe, it, expect } from 'vitest'
import { render } from '@testing-library/react'
import type { PathPrediction } from '../../types'
import { PANE_BY_ID } from './panes'
import type { PaneContext } from './paneContext'

function pred(over: Partial<PathPrediction>): PathPrediction {
  return { engine: 'p533', bands: [], mufNow: 0, mufHourly: [], ...over } as PathPrediction
}

function badge(ctx: Partial<PaneContext>): string | null | undefined {
  const pane = PANE_BY_ID.get('outlook')!
  const { container } = render(<>{pane.expert({ pathOpen: [], ...ctx } as PaneContext)}</>)
  return container.querySelector('.cp-engine')?.textContent
}

describe('the Band Outlook pane names its engine', () => {
  it('on the path to a selected call', () => {
    const path = (p: Partial<PathPrediction>) => badge({ selectedCall: 'G4XYZ', pathPred: pred(p) })
    expect(path({ engine: 'p533' })).toBe('P.533')
    expect(path({ engine: 'heuristic' })).toBe('modelled')
    expect(path({ engine: 'heuristic', engineFallback: true })).toBe('modelled: P.533 could not run')
  })

  it('on the outlook to DX', () => {
    const ring = (p: Partial<PathPrediction>) => badge({ selectedCall: null, bandOutlook: pred(p), outlookOpen: [] })
    expect(ring({ engine: 'p533' })).toBe('modelled · DX')
    expect(ring({ engine: 'heuristic', engineFallback: true })).toBe('modelled: P.533 could not run')
  })
})
