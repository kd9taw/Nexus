// @vitest-environment jsdom
// THE CLUB BAND BOARD IS A NAV DESTINATION, not a panel buried at the bottom of a
// busy page. The operator's report: "On the networking window, it needs to be
// dedicated, popped out, not some burried feature in another busy window. This is
// what operators are using to see where people are so they can move to the right
// bands when multiple stations are operating."
//
// The findability defect this file pins is narrow and exact. The board used to
// exist ONLY as a block inside FieldDayView that renders when `fieldDay.club` is
// non-null — which requires club sync to already be ON. An operator who has not
// switched sync on saw nothing, was told nothing, and had no way to discover the
// board existed. So the rail button is gated on the FIELD DAY MASTER SWITCH
// (`enabled.fieldDay`, which App overrides with `settings.fdActive`) and on
// nothing else: ModeNav is handed no club state at all, which is the guarantee.
import { describe, it, expect, vi } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { ModeNav } from './ModeNav'
import type { FeatureId } from '../features/registry'

// (import.meta.url is an http: URL under the jsdom environment — resolve from the
// vitest cwd, which is the ui/ project root, the index-preseed.test.ts idiom.)
const app = readFileSync(resolve(process.cwd(), 'src/App.tsx'), 'utf8')

function renderNav(enabled: Partial<Record<FeatureId, boolean>>, onClubBoard = vi.fn()) {
  const onSelect = vi.fn()
  render(
    <ModeNav
      view="fieldDay"
      mode="fieldDay"
      enabled={enabled as Record<FeatureId, boolean>}
      onSelect={onSelect}
      tier="FT8"
      onDigitalMode={vi.fn()}
      onClubBoard={onClubBoard}
    />,
  )
  return { onSelect, onClubBoard }
}

describe('the club band board has its own way in', () => {
  it('the rail carries it, and pressing it opens the window rather than changing view', () => {
    const { onSelect, onClubBoard } = renderNav({ fieldDay: true })
    const btn = screen.getByRole('button', { name: /club band board/i })
    fireEvent.click(btn)
    expect(onClubBoard).toHaveBeenCalledTimes(1)
    // It is a WINDOW, not a section: nothing in the main window changes.
    expect(onSelect).not.toHaveBeenCalled()
    cleanup()
  })

  it('it is visible with club sync off — the whole point', () => {
    // ModeNav is handed no club/sync state, so there is nothing it COULD gate on.
    // Stated as a case because the bug was exactly this: a board only reachable
    // once you had already found the setting that fills it.
    renderNav({ fieldDay: true })
    expect(screen.queryByRole('button', { name: /club band board/i })).toBeTruthy()
    cleanup()
  })

  it('the Field Day master switch hides it, like every other Field Day surface', () => {
    renderNav({ fieldDay: false })
    expect(screen.queryByRole('button', { name: /club band board/i })).toBeNull()
    // POSITIVE CONTROL: the query above matches something when FD is on, so the
    // null is the gate rather than a stale accessible name.
    cleanup()
    renderNav({ fieldDay: true })
    expect(screen.queryByRole('button', { name: /club band board/i })).toBeTruthy()
    cleanup()
  })

  it('it sits with Field Day in the rail, not stranded at the end', () => {
    renderNav({ fieldDay: true })
    const buttons = [...document.querySelectorAll('.mode-nav-top button')]
    const fd = buttons.findIndex((b) => /Field Day/i.test(b.getAttribute('aria-label') ?? ''))
    const club = buttons.findIndex((b) => /club band board/i.test(b.getAttribute('aria-label') ?? ''))
    expect(fd).toBeGreaterThanOrEqual(0)
    expect(club).toBe(fd + 1)
    cleanup()
  })

  it('App wires the rail button to the club-board window', () => {
    // The rail is presentational (it takes callbacks, it never calls the API);
    // the seam that actually opens the OS window lives in App.
    expect(app).toMatch(/onClubBoard=\{\(\) => void openPanelWindow\('fdclub'\)\}/)
  })
})
