// @vitest-environment jsdom
//
// DOES GERMAN ACTUALLY REACH THE SCREEN? Everything else about this feature is tested in
// pieces — the catalog agrees with English, the switch persists and announces, the runtime
// falls back — and none of that proves a component renders a German word. A catalog wired to
// nothing passes every one of those checks.
//
// So: install the real German catalog, switch, render a REAL shipped component, and read the
// German off the DOM. Then switch back and read the English off the same component, which is
// what proves the switch is what did it rather than the test fixture.

import { describe, it, expect, afterEach } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import { LogConfirm } from '../components/LogConfirm'
import { DE } from './de'
import { EN, installCatalog, setLocale } from './index'
import type { LoggedQso } from '../types'

const QSO = {
  call: 'DL1ABC',
  grid: 'JO62',
  band: '20m',
  mode: 'FT8',
  rstSent: '-12',
  rstRcvd: '-09',
} as unknown as LoggedQso

afterEach(() => {
  setLocale('en')
  cleanup()
})

describe('German on a real screen', () => {
  it('renders the log prompt in German, and back in English when switched', () => {
    installCatalog('de', DE)

    setLocale('de')
    render(<LogConfirm record={QSO} onConfirm={() => {}} onDiscard={() => {}} />)
    expect(screen.getByRole('heading', { name: DE['logPrompt.title'] as string })).toBeTruthy()
    // The label beside it, so this is not one lucky string.
    expect(screen.getByText(DE['logPrompt.call.label'] as string)).toBeTruthy()
    cleanup()

    // THE CONTROL. Same component, same props, English catalog — if this also came out German
    // the test above proved nothing about the switch.
    setLocale('en')
    render(<LogConfirm record={QSO} onConfirm={() => {}} onDiscard={() => {}} />)
    expect(screen.getByRole('heading', { name: EN['logPrompt.title'] })).toBeTruthy()
    expect(DE['logPrompt.title']).not.toBe(EN['logPrompt.title'])
  })

  it('leaves the technical vocabulary alone on a German screen', () => {
    installCatalog('de', DE)
    setLocale('de')
    render(<LogConfirm record={QSO} onConfirm={() => {}} onDiscard={() => {}} />)
    // The band and mode are DATA and render from the record, not the catalog. A German
    // operator reads 20m and FT8 exactly as an English one does — this is the units rule
    // arriving on screen rather than in a comment.
    expect(screen.getByText(/20m/)).toBeTruthy()
    expect(screen.getByText(/FT8/)).toBeTruthy()
  })
})
