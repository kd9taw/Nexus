// @vitest-environment jsdom
import { describe, it, expect } from 'vitest'
import { normaliseTyped, upperTyped } from './typedInput'

function box(value: string, start: number, end = start) {
  const el = document.createElement('input')
  el.value = value
  el.setSelectionRange(start, end)
  return el
}

describe('normaliseTyped', () => {
  it('upper-cases in place and leaves the caret after the typed character', () => {
    const el = box('dABC', 1)
    expect(upperTyped(el)).toBe('DABC')
    expect(el.value).toBe('DABC')
    expect([el.selectionStart, el.selectionEnd]).toEqual([1, 1])
  })

  it('keeps a selection', () => {
    const el = box('dl1abc', 1, 3)
    expect(upperTyped(el)).toBe('DL1ABC')
    expect(el.value).toBe('DL1ABC')
    expect([el.selectionStart, el.selectionEnd]).toEqual([1, 3])
  })

  it('places the caret by the normalised text before it when characters are dropped', () => {
    const el = box('DL 1ABC', 4)
    expect(normaliseTyped(el, (s) => s.replace(/\s+/g, ''))).toBe('DL1ABC')
    expect(el.selectionStart).toBe(3)
  })

  it('touches nothing when the value is already normal', () => {
    const el = box('DL1ABC', 2)
    expect(upperTyped(el)).toBe('DL1ABC')
    expect(el.selectionStart).toBe(2)
  })
})
