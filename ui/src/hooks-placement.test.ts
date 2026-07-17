import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

// Rules-of-hooks guard for the files whose components have an EARLY RETURN before
// the main render (the pattern that once blanked the whole app: a hook added after
// `if (!snap) return` changes the hook count between renders → React unmounts).
// tsc, vite build, and vitest render tests all pass while the app is fully broken,
// so this cheap source scan is the backstop. It asserts: once a component-body
// early return appears, no React hook call follows it in that component.
//
// Heuristic scan (no AST): we track the FIRST component-level early return
// (`  if (...) return` / `  if (...) {  return` / `  return (` at 2-space indent,
// i.e. inside the component, not a nested block) and flag any `useX(` at ≤2-space
// indent after it. Nested-block returns (4+ spaces) and module helpers are ignored.

const here = dirname(fileURLToPath(import.meta.url))

const HOOK = /(?:^|[^A-Za-z])use(?:Effect|State|Ref|Memo|Callback|LayoutEffect|Reducer|Context)\(/
// A component-level early return: 2-space indent (top of the component function).
const EARLY_RETURN = /^ {2}(?:if \(.*\)\s*(?:\{\s*)?)?return[\s(;]/

/** Hook calls after the first component-level early return. `startAt` scopes the
 * scan to the component body so module helpers with their own returns are ignored. */
function hooksAfterEarlyReturn(src: string, componentName?: string): number[] {
  const lines = src.split('\n')
  let scanning = componentName == null
  const compRe = componentName
    ? new RegExp(`function ${componentName}\\b`)
    : null
  let earlyReturnLine = -1
  const offenders: number[] = []
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]
    if (!scanning) {
      if (compRe!.test(line)) scanning = true
      continue
    }
    if (earlyReturnLine < 0 && EARLY_RETURN.test(line)) {
      earlyReturnLine = i
      continue
    }
    if (earlyReturnLine >= 0 && /^ {0,2}\S/.test(line) && HOOK.test(line)) {
      offenders.push(i + 1)
    }
  }
  return offenders
}

// Component files whose top-level component legitimately early-returns before its
// render — [file, componentName]. Scan starts at the component definition so
// module helpers before it (with their own returns) don't false-positive.
const GUARDED: [string, string][] = [
  ['App.tsx', 'App'],
  ['components/Conversation.tsx', 'Conversation'],
]

describe('rules of hooks: no hook after a component early return', () => {
  for (const [rel, name] of GUARDED) {
    it(`${rel} calls no hook after its early return`, () => {
      const src = readFileSync(join(here, rel), 'utf8')
      const bad = hooksAfterEarlyReturn(src, name)
      expect(bad, `hook(s) after early return at line(s) ${bad.join(', ')}`).toEqual([])
    })
  }

  it('the scanner itself flags a hook placed after an early return', () => {
    const broken = [
      'export function X() {',
      '  const [a] = useState(0)',
      '  if (!a) return null',
      '  useEffect(() => {}, [])',
      '  return null',
      '}',
    ].join('\n')
    expect(hooksAfterEarlyReturn(broken, 'X')).toEqual([4])
  })
})
