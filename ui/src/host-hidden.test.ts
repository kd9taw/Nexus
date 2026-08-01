import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

// CSS-text guard (same technique as aprsLayout.test.ts) for the keep-alive host
// hide/show contract.
//
// THE BUG THIS EXISTS FOR (2026-08): a CSS insertion between `.operate-host {
// display: contents }` and `.operate-host[hidden] { display: none }` consumed the
// `[hidden]` rule — the new block reused its closing brace and the collapse rule
// vanished. Every gate stayed green because nothing guarded it. The symptom class:
// `display: contents` is an AUTHOR declaration, so it beats the UA stylesheet's
// `[hidden] { display: none }` no matter the specificity — without the explicit
// author-side `[hidden]` rule, a hidden keep-alive host keeps rendering, and the
// Operate cockpit's <main class="layout"> sits in the `.shell` flex row NEXT TO the
// current view (two mains fighting for the shell — the 0.4–0.21 bug class).
//
// The host list is derived from App.tsx, not hardcoded: a NEW keep-alive host wired
// with `hidden={...}` but missing either CSS rule fails here too. Existence of both
// rules is sufficient — `.x-host[hidden]` (0,2,0) always beats `.x-host` (0,1,0),
// so no order/cascade computation is needed.
//
// Comments are stripped before matching so prose describing a rule cannot satisfy
// the guard.
const css = readFileSync(fileURLToPath(new URL('./styles.css', import.meta.url)), 'utf8').replace(
  /\/\*[\s\S]*?\*\//g,
  '',
)
const app = readFileSync(fileURLToPath(new URL('./App.tsx', import.meta.url)), 'utf8')

// Every top-level rule as [selectorList, body]. The flat regex cannot cross a brace,
// so rules inside @media blocks still match individually.
const rules: Array<[string, string]> = []
for (const m of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) rules.push([m[1], m[2]])
const bodiesFor = (selector: string) =>
  rules
    .filter(([sels]) => sels.split(',').some((s) => s.trim() === selector))
    .map(([, body]) => body)

describe('keep-alive host hide/show contract', () => {
  const hosts = [...app.matchAll(/className="([a-z-]+-host)"\s+hidden=/g)].map((m) => m[1])

  it('App.tsx has keep-alive hosts to guard', () => {
    // If the wiring pattern ever changes shape, fail loudly instead of silently
    // guarding an empty set.
    expect(hosts).toContain('operate-host')
    expect(hosts.length).toBeGreaterThanOrEqual(4)
  })

  it.each(hosts)('.%s: display:contents when shown, display:none when hidden', (host) => {
    const shown = bodiesFor(`.${host}`)
    expect(
      shown.some((b) => /display:\s*contents/.test(b)),
      `.${host} must be display:contents so its <main> fills the shell (see .aprs-host, 0.20.5)`,
    ).toBe(true)

    const hidden = bodiesFor(`.${host}[hidden]`)
    expect(
      hidden.some((b) => /display:\s*none/.test(b)),
      `.${host}[hidden] must be display:none — the author-side 'contents' beats the UA [hidden] rule, so without this the hidden host keeps rendering beside the current view`,
    ).toBe(true)
  })
})
