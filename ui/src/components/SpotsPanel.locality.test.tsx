// @vitest-environment jsdom
//
// THE PANEL HAD NO LOCALITY TEST AT ALL. The Needed board has asked "did anyone on my continent
// actually hear this?" since the RBN measurement work, but the Spots panel showed the raw
// worldwide firehose — so a US operator saw JA stations that only Europe and Asia had copied,
// which says nothing about a path from Illinois (operator, 2026-08-19).
//
// Three properties, and the third is the one that keeps the feature honest.
import { describe, it, expect, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { SpotsPanel } from './SpotsPanel'
import type { SpotRow } from '../types'

const spot = (call: string, local: boolean | undefined): SpotRow =>
  ({
    call,
    entity: 'Japan',
    zone: 25,
    state: null,
    band: '20m',
    freqMhz: 14.074,
    mode: 'Digital',
    submode: 'FT8',
    spotter: local ? 'W3LPL' : 'DL8LAS',
    corroborators: [],
    ageSecs: 60,
    comment: '',
    licensed: true,
    spotterLocal: local,
  }) as unknown as SpotRow

afterEach(() => {
  cleanup()
  sessionStorage.clear()
  localStorage.clear()
})

describe('spots locality filter', () => {
  it('hides a spot no one on my continent heard, and says how many', () => {
    render(<SpotsPanel spots={[spot('JA1ABC', false), spot('K2DEF', true)]} bandPlan={[]} selectedCall={null} onSelect={() => {}} onWork={() => {}} />)
    expect(screen.queryByText('K2DEF'), 'a locally-heard spot stays').toBeTruthy()
    expect(screen.queryByText('JA1ABC'), 'a far-only spot is filtered').toBeNull()
    // The count rides on the chip — a filter that removes rows silently is how "my spots
    // disappeared" becomes an unanswerable report.
    expect(screen.getByRole('button', { name: /heard on my continent · 1 hidden/i })).toBeTruthy()
  })

  it('turning it off restores the worldwide feed', () => {
    render(<SpotsPanel spots={[spot('JA1ABC', false), spot('K2DEF', true)]} bandPlan={[]} selectedCall={null} onSelect={() => {}} onWork={() => {}} />)
    fireEvent.click(screen.getByRole('button', { name: /heard on my continent/i }))
    expect(screen.queryByText('JA1ABC'), 'the operator can always have the firehose').toBeTruthy()
  })

  it('KEEPS a spot whose locality could not be judged', () => {
    // `undefined` = an older backend, or an operator callsign that does not resolve. Fail OPEN,
    // the same posture hf_admit_spotters takes: an empty panel is a worse answer than an
    // unfiltered one, and a row must never vanish because we could not classify it.
    render(<SpotsPanel spots={[spot('JA1ABC', undefined)]} bandPlan={[]} selectedCall={null} onSelect={() => {}} onWork={() => {}} />)
    expect(screen.queryByText('JA1ABC')).toBeTruthy()
    expect(screen.getByRole('button', { name: /^heard on my continent$/i }), 'nothing hidden ⇒ no count').toBeTruthy()
  })
})
