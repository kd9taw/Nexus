// @vitest-environment jsdom
//
// The prompt to log stands in the largest room of the window clear of every stop control (LogConfirm,
// useClearOfStops). It measured that room only as it opened and when the window changed size, so a stop that moved
// or appeared while it was open (a cockpit going keyed or alarmed, a strip growing) could end up under the card.
// It measures again when a stop control's box can have changed, once per frame, and never because of its own move.
// ⚠️ jsdom does not lay out: the boxes here are set by hand; the real-Chrome case is ui/probe/stopvisible-probe.mjs.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, render, screen } from '@testing-library/react'
import { LogConfirm } from './LogConfirm'
import { t } from '../i18n'
import type { LoggedQso } from '../types'

const QSO = { call: 'DL1ABC', band: '6m', mode: 'FT8' } as unknown as LoggedQso

/** The window, 1000×800, and the one stop control's box (left, top, width, height), set by each test. */
const WIN = { left: 0, top: 0, right: 1000, bottom: 800, width: 1000, height: 800, x: 0, y: 0 }
let stopBox: [number, number, number, number] = [0, 0, 0, 0]
let measures = 0

beforeEach(() => {
  measures = 0
  vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function (this: Element) {
    if (this.classList.contains('logconfirm-backdrop')) {
      measures++
      return { ...WIN, toJSON: () => WIN } as DOMRect
    }
    if (this.hasAttribute('data-stop-control') && !this.closest('.logconfirm')) {
      const [left, top, width, height] = stopBox
      const r = { left, top, width, height, right: left + width, bottom: top + height, x: left, y: top }
      return { ...r, toJSON: () => r } as DOMRect
    }
    const z = { left: 0, top: 0, right: 0, bottom: 0, width: 0, height: 0, x: 0, y: 0 }
    return { ...z, toJSON: () => z } as DOMRect
  })
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

/** Let every queued frame and observer run: four frames' worth. */
const frames = () => act(() => new Promise<void>((r) => setTimeout(r, 80)))

/** A cockpit's Stop TX beside the open prompt; `at` names the class its layout gives it, so a move is a re-render. */
function Page({ at, stop = true }: { at: string; stop?: boolean }) {
  return (
    <>
      <div className={`strip ${at}`}>
        {stop && <button type="button" data-stop-control="stop-tx">Stop TX</button>}
      </div>
      <LogConfirm record={QSO} onConfirm={() => {}} onDiscard={() => {}} />
    </>
  )
}

const pad = () => {
  const s = (document.querySelector('.logconfirm-backdrop') as HTMLElement).style
  return [s.paddingTop, s.paddingBottom]
}

describe('the prompt to log measures its room again when a stop moves', () => {
  it('a stop that moves into the room after it opened moves the card, with one measure', async () => {
    // Stop TX along the foot of the window: the room is above it.
    stopBox = [0, 760, 1000, 30]
    const { rerender } = render(<Page at="foot" />)
    await frames()
    expect(pad(), 'the room above a stop at the foot').toEqual(['8px', '48px'])
    const comment = screen.getByLabelText(t('logPrompt.comment.label'))
    comment.focus()
    const card = document.querySelector('.logconfirm') as HTMLElement
    card.scrollTop = 37

    // The cockpit goes keyed: its strip, Stop TX in it, now stands along the top of the window.
    stopBox = [0, 10, 1000, 30]
    const before = measures
    rerender(<Page at="top" />)
    await frames()
    expect(pad(), 'the card stayed where the stop now is').toEqual(['48px', '8px'])
    expect(measures - before, 'one stop move is one measure').toBe(1)

    // The card's own move (its backdrop's padding, the stop layer's mirrors) measures nothing more.
    await frames()
    expect(measures - before, 'the card measured again after its own move').toBe(1)
    expect(document.activeElement, 'focus left the box the operator was in').toBe(comment)
    expect(card.scrollTop, 'the card lost its scroll position').toBe(37)
  })

  it('a stop that appears after it opened is measured too', async () => {
    stopBox = [0, 0, 0, 0]
    const { rerender } = render(<Page at="foot" stop={false} />)
    await frames()
    expect(pad(), 'no stop on screen: no room set').toEqual(['', ''])
    stopBox = [0, 760, 1000, 30]
    rerender(<Page at="foot" />)
    await frames()
    expect(pad()).toEqual(['8px', '48px'])
  })

  it('watching the boxes does not measure every frame', async () => {
    // A browser reports a box to its ResizeObserver as soon as it is watched, as this one does.
    class Reporting {
      constructor(private cb: ResizeObserverCallback) {}
      observe(el: Element) {
        setTimeout(() => this.cb([{ target: el } as ResizeObserverEntry], this as unknown as ResizeObserver), 0)
      }
      unobserve() {}
      disconnect() {}
    }
    vi.stubGlobal('ResizeObserver', Reporting)
    try {
      stopBox = [0, 760, 1000, 30]
      render(<Page at="foot" />)
      await frames()
      const settled = measures
      await frames()
      await frames()
      expect(measures - settled, 'it measures again with nothing changed').toBe(0)
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('typing in the card measures nothing', async () => {
    stopBox = [0, 760, 1000, 30]
    render(<Page at="foot" />)
    await frames()
    const before = measures
    const comment = screen.getByLabelText(t('logPrompt.comment.label')) as HTMLInputElement
    await act(async () => {
      comment.setAttribute('data-typed', '1')
      comment.parentElement!.append(document.createElement('span'))
    })
    await frames()
    expect(measures - before).toBe(0)
  })
})
