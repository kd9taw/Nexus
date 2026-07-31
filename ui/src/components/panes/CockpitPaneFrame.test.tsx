// @vitest-environment jsdom
//
// The shared frame four cockpits are about to depend on. What is pinned here is the
// CONTRACT, not the markup: the body is the scroller (so a pane's overflow has somewhere
// to go), the frame exposes no styling hook (so a pane cannot size itself back into the
// clipping bug), and an action the caller did not supply renders no button at all — a
// pane with no removal callback is one a bad stored layout cannot make disappear.
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
    expect(frame.getAttribute('style')).toBeNull()
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
