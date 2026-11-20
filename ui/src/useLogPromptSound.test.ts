// @vitest-environment jsdom
//
// THE LOG PROMPT'S SOUND (#541): an operator not watching the screen hears that a contact wants
// logging. It plays once when the popup comes up (no popup → a contact), and once when Auto-log
// writes a contact with no popup to show it (`autoLoggedTick` moves). Never for a snapshot that
// only re-renders the popup, never for the next contact queued behind it taking its place (the
// popup never went away), never for what a fresh window opens on (its first snapshot included), and never while the
// setting is off, as it ships.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook } from '@testing-library/react'

const alerts = vi.hoisted(() => ({ logPromptEarcon: vi.fn() }))
vi.mock('./alerts', () => alerts)

import { useLogPromptSound } from './useLogPromptSound'

const A = { call: 'W9XYZ' }
const B = { call: 'K1ABC' }
type P = { prompt: object | null | undefined; tick: number | undefined; on: boolean }

function mount(first: P) {
  return renderHook((p: P) => useLogPromptSound(p.prompt, p.tick, p.on), { initialProps: first })
}

beforeEach(() => alerts.logPromptEarcon.mockClear())

describe('the log prompt sound', () => {
  it('plays once when the popup comes up, not on its re-renders or the next contact behind it', () => {
    const h = mount({ prompt: null, tick: 0, on: true })
    expect(alerts.logPromptEarcon).not.toHaveBeenCalled()
    h.rerender({ prompt: A, tick: 0, on: true })
    expect(alerts.logPromptEarcon).toHaveBeenCalledTimes(1)
    h.rerender({ prompt: { ...A }, tick: 0, on: true })
    h.rerender({ prompt: B, tick: 0, on: true })
    expect(alerts.logPromptEarcon).toHaveBeenCalledTimes(1)
    h.rerender({ prompt: null, tick: 0, on: true })
    h.rerender({ prompt: B, tick: 0, on: true })
    expect(alerts.logPromptEarcon).toHaveBeenCalledTimes(2)
  })

  it('plays once for each contact Auto-log writes, not for the value a window opens on', () => {
    const h = mount({ prompt: null, tick: 7, on: true })
    h.rerender({ prompt: null, tick: 7, on: true })
    expect(alerts.logPromptEarcon).not.toHaveBeenCalled()
    h.rerender({ prompt: null, tick: 8, on: true })
    h.rerender({ prompt: null, tick: 8, on: true })
    expect(alerts.logPromptEarcon).toHaveBeenCalledTimes(1)
  })

  it('a popup already up when the window opens (or its first snapshot lands) is not a new one', () => {
    mount({ prompt: A, tick: 0, on: true })
    const h = mount({ prompt: undefined, tick: undefined, on: true })
    h.rerender({ prompt: A, tick: 3, on: true })
    expect(alerts.logPromptEarcon).not.toHaveBeenCalled()
  })

  it('is silent while the setting is off', () => {
    const h = mount({ prompt: null, tick: 0, on: false })
    h.rerender({ prompt: A, tick: 1, on: false })
    expect(alerts.logPromptEarcon).not.toHaveBeenCalled()
  })
})
