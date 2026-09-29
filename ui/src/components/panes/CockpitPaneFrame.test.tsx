// @vitest-environment jsdom
//
// The shared frame four cockpits are about to depend on. What is pinned here is the
// CONTRACT, not the markup: the body is the scroller (so a pane's overflow has somewhere
// to go), the frame exposes no styling hook (so a pane cannot size itself back into the
// clipping bug), and an action the caller did not supply renders no button at all — a
// pane with no removal callback is one a bad stored layout cannot make disappear.
import { createRef } from 'react'
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { CockpitPaneFrame } from './CockpitPaneFrame'

afterEach(cleanup)

describe('CockpitPaneFrame', () => {
  it('renders the shipped .pane-frame > .pane-head + .pane-body shape', () => {
    render(
      <CockpitPaneFrame title="Band Activity" paneId="bandActivity">
        <p>rows</p>
      </CockpitPaneFrame>,
    )
    const frame = screen.getByLabelText('Band Activity')
    expect(frame.className).toBe('pane-frame')
    expect(frame.getAttribute('data-pane')).toBe('bandActivity')
    expect(frame.querySelector('.pane-head .pane-title')!.textContent).toBe('Band Activity')
    // The content lives in the BODY — the box styles.css gives `overflow:auto`.
    expect(frame.querySelector('.pane-body')!.textContent).toBe('rows')
  })

  it('offers no styling hook on the frame (a pane cannot size itself)', () => {
    render(
      <CockpitPaneFrame title="Log">
        <p>form</p>
      </CockpitPaneFrame>,
    )
    const frame = screen.getByLabelText('Log')
    expect(frame.className).toBe('pane-frame') // no caller class ever joins it
    // The ONLY inline styles are the role-derived flex (fill by default, honoring the
    // 1-col tier's --cockpit-pane-flex override) and the min-height consult of the
    // shell-owned --cockpit-fill-min knob (region-less cockpits; fenced in
    // cockpit-panes.test.ts). Both are placement, neither is caller-suppliable, so
    // "size yourself" is still not expressible.
    expect(frame.getAttribute('style')).toBe(
      'flex: var(--cockpit-pane-flex, 1 1 0); min-height: var(--cockpit-fill-min, 0);',
    )
    expect(frame.dataset.fit).toBe('fill')
  })

  it('an operator-dragged share rides as --pane-share ON the frame, so a seam can repaint it live', () => {
    // A PaneSeam paints a custom property on the two panes it splits, mid-drag, and the
    // record's value lands on the next render. A literal weight in the flex (the default
    // branch above) cannot be repainted that way, so a frame that is split by a seam carries
    // its share as a property of its own — still a PLACEMENT input, typed and numeric, never
    // a size the pane declares. The 1-col tier's --cockpit-pane-flex still wins first.
    const ref = createRef<HTMLElement>()
    render(
      <CockpitPaneFrame title="Spots" paneId="spots" share={1.3} paneRef={ref}>
        <p>rows</p>
      </CockpitPaneFrame>,
    )
    const frame = screen.getByLabelText('Spots')
    expect(ref.current, 'the seam has no handle on the frame').toBe(frame)
    expect(frame.style.getPropertyValue('--pane-share')).toBe('1.3')
    expect(frame.getAttribute('style')).toContain('flex: var(--cockpit-pane-flex, var(--pane-share, 1) 1 0)')
    expect(frame.className).toBe('pane-frame') // still no styling hook
  })

  it('a pane a divider splits floors IN PROPORTION TO ITS SHARE, so the divider moves it in a short column too', () => {
    // In a column too short for every fill pane's floor, each pane sits ON its floor and a share
    // moves nothing: Phone's Spots / Needed divider rewrote the record at 1024×768 and 1366×768
    // while neither pane moved (layout L2, measured in Chrome). So a split pane's floor is the
    // knob × its share — the pair's floors add up to the two stock floors, divided as the
    // operator divided the pair — and still yields to the column.
    render(
      <CockpitPaneFrame title="Spots" paneId="spots" share={1.3} split={1}>
        <p>rows</p>
      </CockpitPaneFrame>,
    )
    expect(screen.getByLabelText('Spots').style.minHeight).toBe(
      'min(calc(var(--cockpit-fill-min, 0px) * var(--pane-share, 1) / 1), 100%)',
    )
  })

  it('a split pane nobody has moved is the stock pane: its weight as the grow, the knob as the floor', () => {
    // A pair of unequal weights (JS8: Activity 2, Band activity 1) is split with `split` = the
    // pair's mean weight: no share of its own yet, so no --pane-share — the grow falls back to the
    // weight and the floor to knob × split / split, the stock floor.
    render(
      <CockpitPaneFrame title="Activity" paneId="activity" weight={2} split={1.5}>
        <p>rows</p>
      </CockpitPaneFrame>,
    )
    const frame = screen.getByLabelText('Activity')
    expect(frame.style.getPropertyValue('--pane-share')).toBe('')
    expect(frame.getAttribute('style')).toContain('flex: var(--cockpit-pane-flex, var(--pane-share, 2) 1 0)')
    expect(frame.style.minHeight).toBe('min(calc(var(--cockpit-fill-min, 0px) * var(--pane-share, 1.5) / 1.5), 100%)')
  })

  it('renders pop-out / remove only when the cockpit supplies them', () => {
    const onPopOut = vi.fn()
    const onRemove = vi.fn()
    const { rerender } = render(
      <CockpitPaneFrame title="DSP" onPopOut={onPopOut} onRemove={onRemove}>
        <p>x</p>
      </CockpitPaneFrame>,
    )
    fireEvent.click(screen.getByLabelText('Open DSP in its own window'))
    fireEvent.click(screen.getByLabelText('Hide DSP'))
    expect(onPopOut).toHaveBeenCalledTimes(1)
    expect(onRemove).toHaveBeenCalledTimes(1)

    rerender(
      <CockpitPaneFrame title="DSP">
        <p>x</p>
      </CockpitPaneFrame>,
    )
    expect(screen.queryByLabelText('Open DSP in its own window')).toBeNull()
    expect(screen.queryByLabelText('Hide DSP')).toBeNull()
  })

  it('puts pane-supplied actions in the head cluster, before the frame buttons', () => {
    render(
      <CockpitPaneFrame title="Decode" actions={<button type="button">Clear</button>} onRemove={() => {}}>
        <p>x</p>
      </CockpitPaneFrame>,
    )
    const acts = screen.getByLabelText('Decode').querySelector('.cockpit-pane-acts')!
    expect([...acts.children].map((c) => c.textContent)).toEqual(['Clear', '✕'])
  })
})
