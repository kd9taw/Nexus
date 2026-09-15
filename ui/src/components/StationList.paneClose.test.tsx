// @vitest-environment jsdom
//
// THE STATIONS PANE'S OWN ✕ (2026-09-15).
//
// Operate's Classic rail hosts this list as its ⊞ `stations` entry, and it is the one pane
// in that cockpit whose NODE is built by App (passed down as `roster`), not by the cockpit.
// So OperateCockpit.paneClose.test.tsx cannot sweep it — its fixture passes a stub node —
// and it is computed here instead, against the real component.
//
// NOT SWEPT ANYWHERE, and said out loud rather than implied: App's one line of wiring
// (`onRemove={() => operatePanels.setPanelState('stations', 'removed')}`). That the prop
// below is actually passed by App is a human step.
import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { StationList } from './StationList'
import type { NeedTag, Station } from '../types'

const STATIONS = [
  { call: 'PA0XYZ', grid: 'JO22', snr: -5, lastHeardSlot: 10, heardCount: 1, presence: 'active', worked: false },
] as unknown as Station[]

// This project runs vitest WITHOUT auto-cleanup, so renders otherwise pile up in one
// document and every query finds two of everything.
afterEach(cleanup)

function mount(props: Partial<React.ComponentProps<typeof StationList>> = {}) {
  return render(
    <StationList
      stations={STATIONS}
      myGrid="EN52"
      currentSlot={10}
      activePeer={null}
      unreadByPeer={{}}
      needByCall={new Map<string, NeedTag>()}
      onSelect={() => {}}
      onCall={() => {}}
      conversations={[]}
      onArchive={() => {}}
      bandActive={false}
      bandUnread={0}
      onSelectBand={() => {}}
      {...props}
    />,
  )
}

describe('the Stations pane carries its own ✕', () => {
  it('names the panel the ⊞ menu calls it, and presses through to the hide', () => {
    const onRemove = vi.fn()
    mount({ onRemove, paneTitle: 'Stations' })
    fireEvent.click(screen.getByRole('button', { name: 'Hide Stations' }))
    expect(onRemove).toHaveBeenCalledTimes(1)
  })

  it('no hide handed down ⇒ no button — this list has hosts with no ⊞ entry for it', () => {
    mount()
    // POSITIVE CONTROL: the header rendered (the count badge is there), so the absence
    // below is the missing prop rather than a component that did not mount.
    expect(screen.getByText('Stations')).toBeTruthy()
    expect(screen.queryByRole('button', { name: /^Hide / })).toBeNull()
  })

  it('carries no consequence warning — hiding a roster ends nothing', () => {
    mount({ onRemove: vi.fn(), paneTitle: 'Stations' })
    const close = screen.getByRole('button', { name: 'Hide Stations' })
    expect(close.getAttribute('aria-describedby')).toBeNull()
    expect(close.getAttribute('title')).toMatch(/panels menu/i)
  })
})
