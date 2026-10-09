// @vitest-environment jsdom
//
// THE RAIL CALLS THE CONTEST SCREEN "CONTEST". It is the workspace for whichever contest is
// picked, so its label is the word and its tooltip names the contest ("Contest — Illinois QSO
// Party"). The item keeps its id, `fieldDay`, because the operator's rail order and every stored
// record name it by id: an operator who dragged it next to the Logbook finds it there after the
// upgrade, under its new name, and the record says exactly what it said before.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import { ModeNav } from './ModeNav'
import type { FeatureId } from '../features/registry'

const ALL_ON = new Proxy({}, { get: () => true }) as Record<FeatureId, boolean>

function renderNav(contest?: string) {
  render(
    <ModeNav
      view="operate"
      mode="chat"
      enabled={ALL_ON}
      onSelect={vi.fn()}
      tier="FT8"
      onDigitalMode={vi.fn()}
      onClubBoard={vi.fn()}
      contest={contest}
    />,
  )
}

/** The reorderable items, top to bottom, by the label on screen. */
const railLabels = () =>
  [...document.querySelectorAll('.mode-nav-drag .mode-label')].map((el) => el.textContent)

// ⭐ AN UPGRADE FIXTURE: the record an older build stored for an operator who had dragged the
// Field Day item up beside the Logbook, and the rail that build drew from it.
const STORED = JSON.stringify(['logbook', 'fieldDay', 'connect', 'needed', 'spots', 'awards', 'stats'])
const OLD_RAIL = [
  'Logbook',
  'Field Day',
  'Conditions',
  'Needed',
  'Spots',
  'Awards',
  'Stats',
  'DXped',
  'Satellites',
  'POTA/SOTA',
  'Memories',
  'Repeaters',
]

beforeEach(() => localStorage.clear())
afterEach(cleanup)

describe('the rail item for the contest screen', () => {
  it('reads "Contest", and its tooltip names the picked contest', () => {
    renderNav('Illinois QSO Party')
    const btn = screen.getByRole('button', { name: 'Contest — Illinois QSO Party' })
    expect(btn.querySelector('.mode-label')!.textContent).toBe('Contest')
    // The event keeps its own name everywhere it is the event; the rail item is not the event.
    expect(railLabels()).not.toContain('Field Day')
  })

  it('before the settings arrive, the label alone', () => {
    renderNav()
    expect(screen.getByRole('button', { name: 'Contest' }).querySelector('.mode-label')!.textContent).toBe(
      'Contest',
    )
  })

  it('keeps its place: an older build’s stored order puts it where the operator left it, record untouched', () => {
    localStorage.setItem('nexus.navOrder', STORED)
    renderNav('ARRL Field Day')
    // Same rail, same order, one new name.
    expect(railLabels()).toEqual(OLD_RAIL.map((l) => (l === 'Field Day' ? 'Contest' : l)))
    // The Club Board button still rides directly under it.
    const buttons = [...document.querySelectorAll('.mode-nav-top button')]
    const contest = buttons.findIndex((b) => b.getAttribute('aria-label') === 'Contest — ARRL Field Day')
    expect(buttons[contest + 1].getAttribute('aria-label')).toMatch(/club band board/i)
    // Nothing rewrote the record: it is byte for byte what the older build stored.
    expect(localStorage.getItem('nexus.navOrder')).toBe(STORED)
  })
})
