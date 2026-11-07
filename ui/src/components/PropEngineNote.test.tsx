// @vitest-environment jsdom
//
// The one-time note after the move to P.533: it says how to go back, opens Settings, and is
// remembered once dismissed even if the settings file could not record it.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { PropEngineNote, propEngineNoteDismissed, rememberPropEngineNoteDismissed } from './PropEngineNote'
import { EN as en } from '../i18n/en'

beforeEach(() => localStorage.clear())
afterEach(cleanup)

describe('the note after the move to P.533', () => {
  it('says predictions now use P.533 and where to switch back', () => {
    render(<PropEngineNote onOpenSettings={() => {}} onDismiss={() => {}} />)
    const text = screen.getByRole('note').textContent ?? ''
    expect(text).toContain('P.533')
    expect(text).toContain(en['settings.tabs.services'])
    expect(text).toContain(en['settings.integrations.propEngine.label'])
  })

  it('opens Settings from its words and dismisses from its own button, neither doing the other', () => {
    const onOpenSettings = vi.fn()
    const onDismiss = vi.fn()
    render(<PropEngineNote onOpenSettings={onOpenSettings} onDismiss={onDismiss} />)
    fireEvent.click(screen.getByRole('button', { name: /P\.533/ }))
    expect(onOpenSettings).toHaveBeenCalledTimes(1)
    expect(onDismiss).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: en['common.dismiss'] }))
    expect(onDismiss).toHaveBeenCalledTimes(1)
  })

  it('takes neither the focus nor a position over anything', () => {
    const { container } = render(
      <>
        <input aria-label="log" autoFocus />
        <PropEngineNote onOpenSettings={() => {}} onDismiss={() => {}} />
      </>,
    )
    expect((document.activeElement as HTMLElement).getAttribute('aria-label')).toBe('log')
    expect(container.querySelector('[role="dialog"]')).toBeNull()
  })

  it('remembers a dismissal the settings file could not record', () => {
    expect(propEngineNoteDismissed()).toBe(false)
    rememberPropEngineNoteDismissed()
    expect(propEngineNoteDismissed()).toBe(true)
  })
})
