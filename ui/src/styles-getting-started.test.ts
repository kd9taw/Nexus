import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

// Theme guard for the Getting started guide (`.gsg-*` in styles.css).
//
// The guide paints entirely from tokens, and a token declared in only ONE theme
// renders as `unset` in the other — text that vanishes, a border that does not
// draw. Nothing about the declaration looks wrong, and nobody opens a help panel
// in light mode on purpose, so the failure would ship. This resolves every var
// the .gsg rules reference against :root and both theme blocks.
//
// Not a presence test: it does not look for a token by name, it computes which
// names are REACHED by the guide's own rules and then asks whether each one has
// somewhere to resolve in either theme.
describe('every token the guide paints with exists in BOTH themes', () => {
  const CSS = readFileSync(fileURLToPath(new URL('./styles.css', import.meta.url)), 'utf8')
    // Prose must not read as CSS — a comment naming --some-var is not a use.
    .replace(/\/\*[\s\S]*?\*\//g, ' ')

  /** Custom-property names declared directly inside the first `<sel> { … }` block. */
  function declaredIn(sel: string): Set<string> {
    const out = new Set<string>()
    let from = 0
    for (;;) {
      const at = CSS.indexOf(sel, from)
      if (at === -1) return out
      const open = CSS.indexOf('{', at)
      const close = CSS.indexOf('}', open)
      if (open === -1 || close === -1) return out
      for (const m of CSS.slice(open, close).matchAll(/(--[\w-]+)\s*:/g)) out.add(m[1])
      from = close
    }
  }

  const root = declaredIn(':root')
  const dark = declaredIn("[data-theme='dark']")
  const light = declaredIn("[data-theme='light']")

  it('finds the palettes (the discovery cannot silently empty out)', () => {
    // A positive control: if these come back empty the sweep below proves nothing.
    expect(root.size).toBeGreaterThan(20)
    expect(dark.size).toBeGreaterThan(20)
    expect(light.size).toBeGreaterThan(20)
    expect(dark.has('--accent')).toBe(true)
    expect(light.has('--accent')).toBe(true)
  })

  it('resolves every var referenced by a .gsg rule', () => {
    // Every rule whose selector mentions the guide, plus the guide's dialog box.
    const rules = [...CSS.matchAll(/([^{}]+)\{([^{}]*)\}/g)].filter((m) =>
      /\.gsg[\w-]*/.test(m[1]),
    )
    expect(rules.length, 'the guide has styles to check').toBeGreaterThan(30)

    const missing: string[] = []
    for (const [, sel, body] of rules) {
      for (const m of body.matchAll(/var\((--[\w-]+)/g)) {
        const name = m[1]
        if (root.has(name)) continue
        if (dark.has(name) && light.has(name)) continue
        missing.push(
          `${name} in "${sel.trim()}" — ` +
            `${dark.has(name) ? '' : 'not in dark; '}${light.has(name) ? '' : 'not in light; '}` +
            'not in :root',
        )
      }
    }
    expect(
      missing,
      `a guide token resolves in only one theme — it renders as unset in the other:\n${missing.join('\n')}`,
    ).toHaveLength(0)
  })
})
