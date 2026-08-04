import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

// A RETIRED RULE IS SAFE ONLY IF NO LIVE JSX STILL EMITS ITS CLASS.
//
// 8b38ff77 deleted the `<span class="bandstrip-title">` that duplicated the frame head in
// BandStrip's two framed hosts, and retired the rule with it — a correct per-COMPONENT check
// (BandStrip does have exactly two hosts, both framed). But `.bandstrip-title` is a CLASS, and
// BandMap — a different component, rendered in the torn-off ⧉ band-map window — still emits it
// at two render sites. The heading lost its entire style there at every size and zoom, with no
// test failing: nothing in the suite renders the detached window, and a rule's disappearance
// cannot fail a test that never asserted it.
//
// This guard is the class-level half of that check, so the next retirement of a shared class
// cannot pass on component-level reasoning alone. It reads the SOURCE for emitters rather than
// rendering, because the point is precisely to catch a consumer nothing renders in test.
const here = (p: string) => fileURLToPath(new URL(p, import.meta.url))
const css = readFileSync(here('../styles.css'), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '')

/** Every component source that emits `className="<cls>"` (exact, single-class form). */
function emitters(cls: string): string[] {
  const out: string[] = []
  for (const f of ['BandMap.tsx', 'BandStrip.tsx', 'PhoneCockpit.tsx', 'CwCockpit.tsx']) {
    const src = readFileSync(here(f), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '')
    if (src.includes(`className="${cls}"`)) out.push(f)
  }
  return out
}

/** True when some rule in styles.css has `.<cls>` as its subject. */
function styled(cls: string): boolean {
  return new RegExp(String.raw`(^|[,}])\s*[^{}]*\.${cls}(?![\w-])[^{},]*\{`, 'm').test(css)
}

describe('a shared class keeps its rule while any component still emits it', () => {
  it('.bandstrip-title is styled, because BandMap still renders it in the torn-off window', () => {
    const who = emitters('bandstrip-title')
    // Guard the guard: if nobody emits it any more, the rule may legitimately go — but then
    // this test must be deleted deliberately, not pass vacuously.
    expect(
      who,
      'nothing emits .bandstrip-title any more — retire this guard WITH the rule, in one commit',
    ).not.toHaveLength(0)
    expect(
      styled('bandstrip-title'),
      `${who.join(', ')} render class="bandstrip-title" and no styles.css rule targets it. ` +
        'The detached band-map heading is unstyled at every size and zoom.',
    ).toBe(true)
  })
})
