// @vitest-environment jsdom
import { describe, it, expect, afterEach } from 'vitest'
import { renderHook } from '@testing-library/react'
import { useFt8StepDownNotice } from './useFt8StepDownNotice'
import { dismissToast, subscribeToasts, type Toast } from './toast'
import type { Ft8StepDown } from './types'

let toasts: Toast[] = []
subscribeToasts((ts) => (toasts = ts))
afterEach(() => {
  for (const x of toasts) dismissToast(x.id)
})

const step = (seq: number, from: Ft8StepDown['from'], to: Ft8StepDown['to']): Ft8StepDown => ({
  seq,
  chosen: 'ludicrous',
  from,
  to,
  decodeMs: 2900,
  budgetMs: 2100,
})

function mount(first?: Ft8StepDown) {
  const hook = renderHook((p: { sd?: Ft8StepDown }) => useFt8StepDownNotice({ ft8StepDown: p.sd }), {
    initialProps: { sd: first },
  })
  return (sd?: Ft8StepDown) => hook.rerender({ sd })
}

describe('the FT8 decoder step-down notice', () => {
  it('says each step once, and why', () => {
    const next = mount()
    next(step(1, 'ludicrous', 'keen'))
    expect(toasts.map((t) => t.message)).toEqual([
      'FT8 decoding took 2900 ms, more than the 2100 ms a reply can wait. The JTDX-style decoder now runs Keen instead of Ludicrous for this session.',
    ])
    next(step(1, 'ludicrous', 'keen'))
    expect(toasts).toHaveLength(1)
  })

  it('says the floor', () => {
    const next = mount()
    next(step(3, 'easy', null))
    expect(toasts[0].message).toContain('Easy is the lightest JTDX-style preset')
  })

  it('treats a step made before the window opened as history', () => {
    const next = mount(step(2, 'keen', 'easy'))
    next(step(2, 'keen', 'easy'))
    expect(toasts).toHaveLength(0)
  })
})
