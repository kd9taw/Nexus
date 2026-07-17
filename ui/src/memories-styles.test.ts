import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

// The invisible-Tune-button lesson (0.9.5): a component can emit a className that
// styles.css never defines and everything still compiles — the control just renders
// unstyled/invisible. Guard: every class MemoryStrip/MemoriesView emit has a rule.
const read = (rel: string): string =>
  readFileSync(fileURLToPath(new URL(rel, import.meta.url)), 'utf8')

describe('Memories styles exist for every emitted class', () => {
  it('styles.css defines each memories/strip class the components use', () => {
    const css = read('./styles.css')
    const sources = read('./components/MemoryStrip.tsx') + read('./components/MemoriesView.tsx')
    // Static class tokens from className attributes only (datalist ids etc. are not classes).
    const attrs = sources.match(/className=(?:"[^"]*"|\{`[^`]*`\})/g) ?? []
    const used = new Set(attrs.join(' ').match(/\b(?:mem|mv|memories)-[a-z0-9-]+/g) ?? [])
    expect(used.size).toBeGreaterThan(10) // the scan itself must be alive
    const missing = [...used].filter((cls) => !css.includes(`.${cls}`))
    expect(missing, `classes with no styles.css rule:\n${missing.join('\n')}`).toEqual([])
  })
})
