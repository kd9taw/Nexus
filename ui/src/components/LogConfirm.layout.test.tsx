// @vitest-environment jsdom
//
// The prompt to log lays its fields in two columns that share the popup's width. The free-text
// fields that would be cut short in half of it take the whole row: the Comment, and the Prop mode
// picker, whose longest names ("Non-satellite repeater or transponder", "Field aligned
// irregularities") showed only their first ~20 characters in a half-width closed box.
//
// Every assertion resolves the cascade WINNER for the rendered element (cssCascade: importance →
// specificity → source order) — a regex over the sheet would pass on a dead selector.
// ⚠️ jsdom does not lay out: the pixels are the real-Chrome probe's (ui/probe/stopabove-probe.mjs).
import { afterEach, describe, expect, it } from 'vitest'
import { cleanup, render, screen } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { chainOf, parseRules, winnerAt } from '../cssCascade'
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
