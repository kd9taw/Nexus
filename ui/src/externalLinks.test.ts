// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { externalLinkHref, installExternalLinkInterceptor } from './externalLinks'

// The app-wide `_blank` anchor fix (mac QA audit): every external anchor must route through
// the injected `open` (the open_external_url command in the app), and internal navigation
// must be left completely alone — no preventDefault, no open.

function makeAnchor(href: string, target?: string): HTMLAnchorElement {
  const a = document.createElement('a')
  a.setAttribute('href', href)
  if (target) a.target = target
  document.body.appendChild(a)
  return a
}

function click(el: Element, init?: MouseEventInit): MouseEvent {
  const e = new MouseEvent('click', { bubbles: true, cancelable: true, button: 0, ...init })
  el.dispatchEvent(e)
  return e
}

describe('externalLinkHref', () => {
  afterEach(() => {
    document.body.innerHTML = ''
  })

  it('returns the href for an external _blank anchor', () => {
    const a = makeAnchor('https://www.repeaterbook.com', '_blank')
    expect(externalLinkHref(a)).toBe('https://www.repeaterbook.com')
  })

  it('resolves through a child of the anchor (icon/label spans)', () => {
    const a = makeAnchor('https://hearham.com', '_blank')
    const span = document.createElement('span')
    a.appendChild(span)
    expect(externalLinkHref(span)).toBe('https://hearham.com')
  })

  it('leaves internal hrefs alone — relative, hash, and non-_blank anchors', () => {
    expect(externalLinkHref(makeAnchor('/settings', '_blank'))).toBeNull()
    expect(externalLinkHref(makeAnchor('#section', '_blank'))).toBeNull()
    // External href but ordinary navigation: not the "open my browser" affordance.
    expect(externalLinkHref(makeAnchor('https://example.com'))).toBeNull()
  })

  it('ignores non-element and non-anchor targets', () => {
    expect(externalLinkHref(null)).toBeNull()
    expect(externalLinkHref(document.createElement('div'))).toBeNull()
  })
})

describe('installExternalLinkInterceptor', () => {
  afterEach(() => {
    document.body.innerHTML = ''
  })

  it('routes an external _blank click through open and consumes the click', () => {
    const open = vi.fn()
    const uninstall = installExternalLinkInterceptor(open)
    const e = click(makeAnchor('https://chirpmyradio.com', '_blank'))
    expect(open).toHaveBeenCalledExactlyOnceWith('https://chirpmyradio.com')
    expect(e.defaultPrevented).toBe(true)
    uninstall()
  })

  it('accepts modifier clicks — Cmd+click is the mac open-link reflex', () => {
    const open = vi.fn()
    const uninstall = installExternalLinkInterceptor(open)
    click(makeAnchor('https://hearham.com', '_blank'), { metaKey: true })
    expect(open).toHaveBeenCalledExactlyOnceWith('https://hearham.com')
    uninstall()
  })

  it('leaves internal anchors alone — no open, no preventDefault', () => {
    const open = vi.fn()
    const uninstall = installExternalLinkInterceptor(open)
    const e = click(makeAnchor('#section', '_blank'))
    expect(open).not.toHaveBeenCalled()
    expect(e.defaultPrevented).toBe(false)
    uninstall()
  })

  it('respects a click something else already handled', () => {
    const open = vi.fn()
    const a = makeAnchor('https://example.com', '_blank')
    a.addEventListener('click', (e) => e.preventDefault())
    const uninstall = installExternalLinkInterceptor(open)
    click(a)
    expect(open).not.toHaveBeenCalled()
    uninstall()
  })

  it('uninstall stops interception', () => {
    const open = vi.fn()
    installExternalLinkInterceptor(open)()
    // Swallow the un-intercepted click so jsdom doesn't attempt a real navigation.
    // Registered AFTER the (removed) interceptor, so a failed removal is still caught.
    document.addEventListener('click', (e) => e.preventDefault(), { once: true })
    click(makeAnchor('https://example.com', '_blank'))
    expect(open).not.toHaveBeenCalled()
  })
})
