// @vitest-environment jsdom
//
// #391, the pop-out's half. A torn-off window is its own document with its own toast host
// (`DetachedShell`) and its own toast bus, so the main window's pop-up switch does not reach it:
// it has to follow the settings it polls itself. With pop-ups off there, a confirmation stays out
// of its corner and an error still pops up, as in the main window (App.popups.test.tsx).
import { describe, it, expect, vi, beforeAll, afterAll, afterEach } from 'vitest'
import { render, cleanup, act, screen, waitFor } from '@testing-library/react'
import { DetachedPanel } from './DetachedPanel'
import * as api from './api'
import { dismissToast, pushToast, subscribeToasts, type Toast } from './toast'

const held = vi.hoisted(() => ({ settings: null as unknown }))

vi.mock('./api', async () => {
  const actual = await vi.importActual<Record<string, unknown>>('./api')
  const out: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    out[k] = typeof actual[k] === 'function' ? vi.fn().mockResolvedValue(null) : actual[k]
  }
  out.subscribeSnapshot = vi.fn(() => () => {})
  out.getBandPlan = vi.fn().mockResolvedValue([])
  out.getPropagation = vi.fn(() => new Promise(() => {}))
  out.getNeedAlerts = vi.fn(() => new Promise(() => {}))
  out.getSettings = vi.fn(async () => structuredClone(held.settings))
  return out
})

let toasts: Toast[] = []
let unsubscribe: () => void = () => {}
beforeAll(() => {
  unsubscribe = subscribeToasts((now) => {
    toasts = now
  })
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(() => {
  cleanup()
  for (const toast of toasts) dismissToast(toast.id)
  vi.clearAllMocks()
})
afterAll(() => unsubscribe())

const inCorner = (text: string) => screen.queryAllByText(text).length > 0

describe('a pop-out window follows the pop-up switch (#391)', () => {
  it('keeps a confirmation out of its corner and still pops up an error, with pop-ups off', async () => {
    held.settings = { popupNotifications: false }
    render(<DetachedPanel panel="needed" />)
    await waitFor(() => expect(api.getSettings).toHaveBeenCalled())
    await act(async () => {
      for (let i = 0; i < 8; i++) await Promise.resolve()
    })
    act(() => {
      pushToast('Exported 12 QSOs', 'success')
      pushToast('Could not reach the station', 'error')
    })
    expect(inCorner('Could not reach the station'), 'the error vanished from the pop-out').toBe(true)
    expect(inCorner('Exported 12 QSOs'), 'the pop-out ignored the switch').toBe(false)
  })

  // THE CONTROL: with the setting absent the pop-out's corner is as it always was.
  it('pops a confirmation up when the settings say nothing about it', async () => {
    held.settings = {}
    render(<DetachedPanel panel="needed" />)
    await waitFor(() => expect(api.getSettings).toHaveBeenCalled())
    await act(async () => {
      for (let i = 0; i < 8; i++) await Promise.resolve()
    })
    act(() => {
      pushToast('Exported 12 QSOs', 'success')
    })
    expect(inCorner('Exported 12 QSOs')).toBe(true)
  })
})
