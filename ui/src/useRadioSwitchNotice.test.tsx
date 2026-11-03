// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest'
import { renderHook } from '@testing-library/react'
import { useRadioSwitchNotice, type RadioSwitchNoticeDeps } from './useRadioSwitchNotice'
import { dismissToast, subscribeToasts, type Toast } from './toast'
import type { AutoRadioSwitch, RadioSwitchPolicy, RadioSummary } from './types'

const radios = [
  { id: 0, name: 'FTdx10' },
  { id: 1, name: 'IC-9700' },
] as RadioSummary[]

let toasts: Toast[] = []
const unsub = subscribeToasts((ts) => (toasts = ts))
afterEach(() => {
  for (const x of toasts) dismissToast(x.id)
})
void unsub

const sw = (seq: number, fromId = 0, toId = 1, band = '2m'): AutoRadioSwitch => ({ seq, fromId, toId, band, mode: 'digital' })

type Live = { activeRadioId?: number; onAir?: boolean }
const radio = (onAir = false) => ({ transmitting: onAir, txBusyReason: null, rigKeyed: false })

function mount(policy: RadioSwitchPolicy | undefined, first?: AutoRadioSwitch) {
  // set_active_radio answers with the snapshot after the pick: the radio asked for, as a working engine does.
  const selectRadio = vi.fn((id: number) => Promise.resolve({ activeRadioId: id } as { activeRadioId?: number }))
  const openSettingsAt = vi.fn()
  const snapOf = (s: AutoRadioSwitch | undefined, live: Live = {}) => ({
    radios,
    autoRadioSwitch: s,
    activeRadioId: 'activeRadioId' in live ? live.activeRadioId : (s?.toId ?? 0),
    radio: radio(live.onAir),
  })
  const hook = renderHook((p: RadioSwitchNoticeDeps) => useRadioSwitchNotice(p), {
    initialProps: { snap: snapOf(first), policy, selectRadio, openSettingsAt },
  })
  const next = (s: AutoRadioSwitch | undefined, pol = policy, live: Live = {}) =>
    hook.rerender({ snap: snapOf(s, live), policy: pol, selectRadio, openSettingsAt })
  return { next, selectRadio, openSettingsAt }
}

describe('the radio switch notice', () => {
  it('says a switch Nexus made by itself, with Undo back to the radio it left and Why? to the rules', async () => {
    const { next, selectRadio, openSettingsAt } = mount('tell')
    expect(toasts).toHaveLength(0)
    next(sw(1))
    expect(toasts).toHaveLength(1)
    const [n] = toasts
    expect(n.message).toBe('Switched to IC-9700 for 2m · Weak-signal digital')
    expect(n.kind).toBe('info')
    expect(n.actionLabel).toBe('Undo')
    await n.action?.()
    expect(selectRadio).toHaveBeenCalledWith(0)
    expect(n.secondaryLabel).toBe('Why?')
    n.secondaryAction?.()
    expect(openSettingsAt).toHaveBeenCalledWith('radios')
  })

  it('a switch refused because another Nexus window runs the radio is said, whatever the policy, with no Undo', () => {
    for (const policy of ['tell', 'act'] as const) {
      const { next } = mount(policy)
      // The station stayed on the radio it was on (fromId 0).
      next({ ...sw(1), refused: true }, policy, { activeRadioId: 0 })
      expect(toasts).toHaveLength(1)
      const [n] = toasts
      expect(n.message).toBe(
        'IC-9700 is open in another Nexus window, so the radio did not change — switch to that window.',
      )
      expect(n.kind).toBe('error')
      expect(n.action).toBeUndefined()
      dismissToast(n.id)
    }
  })

  it('a switch made before the window opened is not news', () => {
    const { next } = mount('tell', sw(4))
    expect(toasts).toHaveLength(0)
    next(sw(4))
    expect(toasts).toHaveLength(0)
    next(sw(5, 1, 0, '20m'))
    expect(toasts.map((x) => x.message)).toEqual(['Switched to FTdx10 for 20m · Weak-signal digital'])
  })

  it('a station that keeps the silent switch sees nothing, and nothing shows before the policy is read', () => {
    const silent = mount('act')
    silent.next(sw(1))
    expect(toasts).toHaveLength(0)
    const unread = mount(undefined)
    unread.next(sw(1))
    expect(toasts).toHaveLength(0)
  })

  it('a newer switch replaces the notice still up, one line at a time', () => {
    const { next } = mount('tell')
    next(sw(1))
    next(sw(2, 1, 0, '20m'))
    expect(toasts.map((x) => x.message)).toEqual(['Switched to FTdx10 for 20m · Weak-signal digital'])
  })

  it('closes as soon as the active radio is no longer the one it names, whoever changed it', () => {
    const { next } = mount('tell')
    next(sw(1))
    expect(toasts).toHaveLength(1)
    // Same switch record, but the operator (or Settings, or a Remote client) picked another radio.
    next(sw(1), 'tell', { activeRadioId: 2 })
    expect(toasts).toHaveLength(0)
  })

  it('is never shown for a switch the active radio has already moved on from', () => {
    const { next } = mount('tell')
    next(sw(1), 'tell', { activeRadioId: 0 })
    expect(toasts).toHaveLength(0)
  })

  it('Undo does nothing when the active radio is no longer the one it names', async () => {
    const { next, selectRadio } = mount('tell')
    next(sw(1))
    const [n] = toasts
    // A change the closing effect has not seen yet: Undo reads the station as it is at the click.
    next(sw(1), 'tell', { activeRadioId: 2 })
    await n.action?.()
    expect(selectRadio).not.toHaveBeenCalled()
  })

  it('Undo never switches radio mid-over: refused while transmitting, with the reason', async () => {
    const { next, selectRadio } = mount('tell')
    next(sw(1))
    const [n] = toasts
    next(sw(1), 'tell', { activeRadioId: 1, onAir: true })
    await expect(n.action?.()).rejects.toThrow('Not while transmitting')
    expect(selectRadio).not.toHaveBeenCalled()
  })

  it('Undo to a radio that is gone or disabled says so instead of closing as if it worked', async () => {
    const { next, selectRadio } = mount('tell')
    selectRadio.mockImplementation(() => Promise.resolve({ activeRadioId: 1 }))
    next(sw(1))
    const [n] = toasts
    await expect(n.action?.()).rejects.toThrow('FTdx10')
    expect(selectRadio).toHaveBeenCalledWith(0)
  })
})
