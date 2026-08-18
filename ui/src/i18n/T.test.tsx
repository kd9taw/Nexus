// @vitest-environment jsdom
//
// <T> renders the rich-text path — and, more to the point, renders the PILOT the same as
// before the migration. "Zero visible change" is a claim about the DOM, so it is checked
// against the DOM rather than asserted in a commit message.

import { afterEach, describe, expect, it } from 'vitest'
import { cleanup, render, screen } from '@testing-library/react'
import { T } from './T'
import { installCatalog, setLocale } from './index'
import { RevealNudge } from '../components/RevealNudge'
import { OnboardingBanner } from '../components/OnboardingBanner'

afterEach(() => {
  cleanup()
  setLocale('en')
})

describe('<T>', () => {
  it('wraps marked spans in the element the CALL SITE supplied', () => {
    const { container } = render(
      <T k="reveal.prompt" tags={{ b: <strong /> }} vals={{ achievement: 'First DX', feature: 'Awards' }} />,
    )
    expect(container.textContent).toBe('First DX — turn on Awards?')
    const bolds = [...container.querySelectorAll('strong')].map((e) => e.textContent)
    expect(bolds).toEqual(['First DX', 'Awards'])
  })

  it('renders a value containing markup as TEXT, never as an element', () => {
    const { container } = render(
      <T
        k="reveal.prompt"
        tags={{ b: <strong /> }}
        vals={{ achievement: '<b>injected</b>', feature: 'Awards' }}
      />,
    )
    expect(container.querySelectorAll('strong')).toHaveLength(2) // the two from the catalog
    expect(container.textContent).toContain('<b>injected</b>')
  })

  it('renders a marker the call site did not declare as literal text', () => {
    installCatalog('zz', { 'reveal.enable': 'ein <i>Ding</i>' })
    setLocale('zz')
    const { container } = render(<T k="reveal.enable" tags={{ b: <strong /> }} />)
    expect(container.querySelector('i')).toBeNull()
    expect(container.textContent).toBe('ein <i>Ding</i>')
  })
})

describe('the migrated components render exactly what they rendered before', () => {
  it('RevealNudge', () => {
    const { container } = render(
      <RevealNudge
        feature={
          { id: 'awards', label: 'Awards', oneLine: 'Track DXCC, WAS and more.' } as never
        }
        achievement={{ title: 'First DX worked' } as never}
        onEnable={() => {}}
        onDismiss={() => {}}
      />,
    )
    expect(container.querySelector('.reveal-text')?.textContent).toBe(
      'First DX worked — turn on Awards? Track DXCC, WAS and more.',
    )
    expect(screen.getByRole('button', { name: 'Enable' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Not now' })).toBeTruthy()
  })

  it('OnboardingBanner — the catalog stores a real "&", not the JSX entity', () => {
    render(<OnboardingBanner onOpenSettings={() => {}} onDismiss={() => {}} />)
    expect(screen.getByRole('button', { name: 'Set your callsign & station in Settings →' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Dismiss' })).toBeTruthy()
  })
})
