// @vitest-environment jsdom
//
// The prompt to log lays its fields in two columns that share the popup's width. The free-text
// fields that would be cut short in half of it take the whole row: the Comment, and the Prop mode
// picker, whose longest names ("Non-satellite repeater or transponder", "Field aligned
// irregularities") showed only their first ~20 characters in a half-width closed box.
//
// Every assertion resolves the cascade WINNER for the rendered element (cssCascade: importance →
// specificity → source order) — a regex over the sheet would pass on a dead selector.
//
// Log, Discard and the prompt's own Stop TX hold to the bottom of the card, which is capped to the
// window and scrolls: at 1366x768 with 175 % scaling the full-width Prop mode row put them below
// the card's fold when it opened, so an operator had to scroll to log.
// ⚠️ jsdom does not lay out: the pixels are the real-Chrome probe's (ui/probe/stopabove-probe.mjs).
import { afterEach, describe, expect, it } from 'vitest'
import { cleanup, render, screen } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { chainOf, expandWith, parseRules, tokensAt, toRgb, winnerAt, type Mode } from '../cssCascade'
import { LogConfirm } from './LogConfirm'
import { t } from '../i18n'
import type { LoggedQso } from '../types'

afterEach(cleanup)

const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES = parseRules(sheet('styles.css'))
const win = (el: Element, prop: string) => winnerAt(RULES, 'dark', chainOf(el), prop)?.value.trim() ?? null

const QSO = { call: 'DL1ABC', band: '6m', mode: 'FT8' } as unknown as LoggedQso

/** The `<label>` that holds the field `name` labels — the grid item. */
function item(name: string): Element {
  const field = screen.getByLabelText(name)
  return field.closest('label')!
}

describe('the log prompt grid', () => {
  it('two columns that may shrink to the popup', () => {
    const { container } = render(<LogConfirm record={QSO} onConfirm={() => {}} onDiscard={() => {}} />)
    expect(win(container.querySelector('.logconfirm-grid')!, 'grid-template-columns')).toBe('repeat(2, minmax(0, 1fr))')
  })

  it('Prop mode and Comment each take the whole row; the short boxes keep one column', () => {
    render(<LogConfirm record={QSO} onConfirm={() => {}} onDiscard={() => {}} />)
    expect(win(item(t('logPrompt.propMode.label')), 'grid-column'), 'Prop mode is cramped into one column').toBe('1 / -1')
    expect(win(item(t('logPrompt.comment.label')), 'grid-column')).toBe('1 / -1')
    for (const k of ['call', 'grid', 'rstSent', 'rstRcvd', 'power', 'operator', 'myGrid'] as const) {
      expect(win(item(t(`logPrompt.${k}.label`)), 'grid-column'), k).toBeNull()
    }
  })
})

describe('the log prompt keeps Log on screen', () => {
  const mounted = () => {
    const { container } = render(<LogConfirm record={QSO} onConfirm={() => {}} onDiscard={() => {}} />)
    const box = container.querySelector('.logconfirm')
    const actions = box?.querySelector(':scope > .logconfirm-actions')
    expect(actions, 'the actions must be a child of the card that scrolls').not.toBeNull()
    return { box: chainOf(box!), actions: chainOf(actions!) }
  }

  it.each(['dark', 'light'] as Mode[])('the actions stick to the bottom of the scrolling card (%s)', (mode) => {
    const { box, actions } = mounted()
    expect(winnerAt(RULES, mode, box, 'overflow-y', 'overflow')?.value, 'the card is the scroller').toBe('auto')
    expect(winnerAt(RULES, mode, actions, 'position')?.value, 'Log can scroll out of view').toBe('sticky')
    const pad = winnerAt(RULES, mode, box, 'padding', 'padding-bottom')?.value
    expect(pad, 'the card must declare its padding').toBeTruthy()
    expect(winnerAt(RULES, mode, actions, 'bottom')?.value).toBe(`calc(-1 * ${pad})`)
    expect(winnerAt(RULES, mode, actions, 'padding', 'padding-bottom')?.value.split(/\s+/).pop()).toBe(pad)
  })

  it.each(['dark', 'light'] as Mode[])('the actions paint the card background (%s)', (mode) => {
    const { box, actions } = mounted()
    const paint = (chain: typeof box) => {
      const w = winnerAt(RULES, mode, chain, 'background', 'background-color')
      expect(w, 'no background wins').not.toBeNull()
      return toRgb(expandWith(tokensAt(RULES, mode, chain), w!.value), [255, 0, 255])
    }
    const under = paint(box)
    expect(under, 'the card must paint an opaque colour').not.toBeNull()
    expect(paint(actions)).toEqual(under)
  })

  it.each(['dark', 'light'] as Mode[])('what scrolls to the end stops above the held row (%s)', (mode) => {
    const { box } = mounted()
    expect(winnerAt(RULES, mode, box, 'scroll-padding-bottom', 'scroll-padding')?.value).toBe('var(--log-prompt-actions-h, 0px)')
    const card = document.querySelector('.logconfirm') as HTMLElement
    expect(card.style.getPropertyValue('--log-prompt-actions-h'), 'the card is not told the row height').toMatch(/^\d+px$/)
  })
})
