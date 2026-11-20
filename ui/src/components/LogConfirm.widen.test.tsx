// @vitest-environment jsdom
//
// THE LOG PROMPT CAN BE WIDENED (#551). It was drawn at a fixed 420 px, with no resize corner, and
// long fields were cramped. A handle on its right edge widens it; the width is remembered, and fitted
// into the window when the prompt opens (a width saved on a bigger screen never runs off this one),
// never narrower than the stock 420 px. Dragging the handle leaves the keyboard focus where it was:
// the box the operator was typing in keeps it.
//
// jsdom does not lay out, so the window the prompt opens in is given here: the backdrop (the window,
// in the app's zoom) measures `WIN` CSS px. The pixels in real Chrome are the stopabove probe's.
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { CLEAR, LogConfirm } from './LogConfirm'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { chainOf, parseRules, winnerAt } from '../cssCascade'
import { t } from '../i18n'
import type { LoggedQso } from '../types'

const QSO = { call: 'DL1ABC', band: '6m', mode: 'FT8' } as unknown as LoggedQso
const KEY = 'nexus.logPrompt.width'
let WIN = 1200

const desc = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'offsetWidth')!
beforeEach(() => {
  localStorage.clear()
  WIN = 1200
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', {
    configurable: true,
    get(this: HTMLElement) {
      return this.classList.contains('logconfirm-backdrop') ? WIN : 0
    },
  })
})
afterEach(() => {
  cleanup()
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', desc)
})

const card = () => document.querySelector<HTMLElement>('.logconfirm')!
const width = () => card().style.getPropertyValue('--log-prompt-w')
const open = () => render(<LogConfirm record={QSO} onConfirm={() => {}} onDiscard={() => {}} />)

describe('the log prompt can be widened', () => {
  it('opens at the stock 420 px with nothing saved', () => {
    open()
    expect(width()).toBe('420px')
  })

  it('opens at the width saved', () => {
    localStorage.setItem(KEY, '640')
    open()
    expect(width()).toBe('640px')
  })

  it('a width saved on a bigger window is fitted into this one when it opens', () => {
    localStorage.setItem(KEY, '1500')
    WIN = 800
    open()
    expect(width()).toBe(`${800 - 16}px`)
  })

  it('never opens narrower than the stock width, and a broken value is the stock width', () => {
    localStorage.setItem(KEY, '200')
    open()
    expect(width()).toBe('420px')
    cleanup()
    localStorage.setItem(KEY, 'wide')
    open()
    expect(width()).toBe('420px')
  })

  it('the edge handle widens it both ways from the centre, is remembered, and keeps the focus where it was', () => {
    open()
    const callBox = screen.getByLabelText(t('logPrompt.call.label'))
    callBox.focus()
    const handle = document.querySelector<HTMLElement>('.logconfirm-widen')!
    expect(handle, 'a handle on the card').not.toBeNull()
    expect(handle.tabIndex, 'not a Tab stop between the boxes').toBe(-1)
    // The card is centred, so its edge moves half the width change: 50 px of drag is 100 px of card.
    fireEvent.pointerDown(handle, { clientX: 500, pointerId: 1, button: 0 })
    fireEvent.pointerMove(handle, { clientX: 550, pointerId: 1 })
    expect(width()).toBe('520px')
    fireEvent.pointerUp(handle, { clientX: 550, pointerId: 1 })
    expect(localStorage.getItem(KEY)).toBe('520')
    expect(document.activeElement, 'the Call box keeps the focus').toBe(callBox)
  })

  it('a drag stops at the window and at the stock width', () => {
    WIN = 700
    open()
    const handle = document.querySelector<HTMLElement>('.logconfirm-widen')!
    fireEvent.pointerDown(handle, { clientX: 500, pointerId: 1, button: 0 })
    fireEvent.pointerMove(handle, { clientX: 2000, pointerId: 1 })
    expect(width()).toBe(`${700 - 16}px`)
    fireEvent.pointerMove(handle, { clientX: 0, pointerId: 1 })
    expect(width()).toBe('420px')
    fireEvent.pointerUp(handle, { clientX: 0, pointerId: 1 })
  })
})

// The card keeps CLEAR px from every stop control (useClearOfStops), and a card widened to fill its
// room stands exactly CLEAR px from one. The handle sits on the card's right edge, partly outside it:
// the part outside must fit in that clearance, or it stands over the stop's edge. Read off the
// cascade winners for the rendered handle, not the sheet's text.
describe('the edge handle stays inside the room the card keeps from a stop', () => {
  it('reaches no further past the card than the clearance, and still overlaps the card', () => {
    const sheet = readFileSync(resolve(process.cwd(), 'src', 'styles.css'), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
    const rules = parseRules(sheet)
    open()
    const handle = document.querySelector('.logconfirm-widen')!
    const px = (prop: string) => {
      const v = winnerAt(rules, 'dark', chainOf(handle), prop)?.value.trim() ?? ''
      expect(v, `${prop} is a px length`).toMatch(/^-?\d+(\.\d+)?px$/)
      return parseFloat(v)
    }
    const right = px('right')
    const width = px('width')
    // The handle's box against the card's right edge at 0: from (-right - width) to -right.
    const outside = -right
    const inside = width - outside
    expect(outside, 'px past the card\'s edge').toBeLessThanOrEqual(CLEAR)
    expect(inside, 'px over the card (its border), so the handle meets the edge').toBeGreaterThan(0)
  })
})
