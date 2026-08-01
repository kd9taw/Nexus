import { describe, expect, it } from 'vitest'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join, relative } from 'node:path'

// Rules-of-hooks guard: no React hook may be called after a component's EARLY RETURN.
// That is the defect that blanks the whole app — the hook count changes between the
// bail-out render and the render after data arrives, React throws "Rendered more hooks
// than during the previous render", and (before the error boundary) unmounted the ENTIRE
// root: nav rail, top bar, everything. tsc, vite build and ordinary render tests all pass
// while the app is fully broken, so this cheap source scan is the backstop.
//
// It has now missed the defect twice, so both holes are closed here:
//   1. WHOLE TREE, not an allowlist. It used to name three files by hand; the 0.24.6
//      field crash (OpeningsLogPane — a pane nobody had listed) was outside it. Every
//      non-test .ts/.tsx under ui/src is scanned now; a new pane is covered on arrival.
//   2. GENERIC hook calls count. The old pattern ended in `\(`, so `useState<{…}>(…)`
//      — literally the first offending line of that crash — read straight past it; the
//      bug was caught only by the `useMemo(` on the next line. `\s*[<(]` sees both.
// Also widened from React's built-ins to ANY `use[A-Z]…` call: a custom hook
// (usePinnedScroll, usePaneWidths) after an early return breaks the count identically.
//
// Heuristic scan (no AST): track the FIRST component-level early return
// (`  if (...) return` / `  if (...) {  return` / `  return (` at 2-space indent, i.e.
// inside the component, not a nested block) and flag any hook call at ≤2-space indent
// after it. Nested-block returns (4+ spaces) and lowercase module helpers are ignored;
// the tracking RESETS at each module-level declaration, so an early return in one
// component cannot false-positive the next component in the same file.

const here = dirname(fileURLToPath(import.meta.url))

/** Any hook call — built-in or custom. `\s*[<(]` (not `\(`) so a generic call
 *  `useState<T>(…)` is a hook call too; that omission is why this guard missed the
 *  0.24.6 black-screen crash's first offending line. */
const HOOK = /(?:^|[^A-Za-z])use[A-Z]\w*\s*[<(]/
/** A component-level early return: 2-space indent (top of the component function). */
const EARLY_RETURN = /^ {2}(?:if \(.*\)\s*(?:\{\s*)?)?return[\s(;]/
/** A module-level declaration — resets the per-component tracking. */
const DECL = /^(?:export\s+)?(?:default\s+)?(?:async\s+)?(?:function|const|let|var|class)\s+(\w+)/
/** Rules of hooks bind components (PascalCase) and custom hooks (`useX`) only. */
const HOOKY = /^(?:[A-Z]|use[A-Z])/

interface Offender {
  /** The enclosing component / custom hook. */
  name: string
  /** 1-indexed source line of the offending hook call. */
  line: number
}

/** Every hook call that follows its own component's first early return. */
function hooksAfterEarlyReturn(src: string): Offender[] {
  const lines = src.split('\n')
  const offenders: Offender[] = []
  let name: string | null = null
  let earlyReturn = -1
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]
    const decl = DECL.exec(line)
    if (decl) {
      name = decl[1]
      earlyReturn = -1
      continue
    }
    if (name === null || !HOOKY.test(name)) continue
    if (earlyReturn < 0) {
      if (EARLY_RETURN.test(line)) earlyReturn = i
      continue
    }
    if (/^ {0,2}\S/.test(line) && HOOK.test(line)) offenders.push({ name, line: i + 1 })
  }
  return offenders
}

/** Count components/custom hooks that DO have a component-level early return — the
 *  liveness census below. A scan that parses nothing reports zero offenders too. */
function earlyReturningDecls(src: string): number {
  const lines = src.split('\n')
  let name: string | null = null
  let armed = false
  let count = 0
  for (const line of lines) {
    const decl = DECL.exec(line)
    if (decl) {
      name = decl[1]
      armed = false
      continue
    }
    if (name === null || !HOOKY.test(name) || armed) continue
    if (EARLY_RETURN.test(line)) {
      armed = true
      count++
    }
  }
  return count
}

/** Every non-test source file under ui/src. */
function sourceFiles(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    if (entry === 'node_modules') continue
    const path = join(dir, entry)
    if (statSync(path).isDirectory()) sourceFiles(path, out)
    else if (/\.tsx?$/.test(entry) && !entry.includes('.test.')) out.push(path)
  }
  return out
}

describe('rules of hooks: no hook after a component early return', () => {
  const files = sourceFiles(here)

  it('holds across every component in ui/src', () => {
    const bad: string[] = []
    for (const path of files) {
      for (const o of hooksAfterEarlyReturn(readFileSync(path, 'utf8'))) {
        bad.push(`${relative(here, path)}:${o.line} (${o.name})`)
      }
    }
    expect(bad, `hook call(s) after an early return:\n  ${bad.join('\n  ')}`).toEqual([])
  })

  it('the sweep actually reads the tree (liveness — a dead scan reports zero too)', () => {
    // If a refactor moves sources, renames the extension, or breaks the declaration
    // walk, the offender list above goes empty and the guard silently dies. These two
    // census floors sit well under the real counts (116 .tsx + 96 .ts, 152 early
    // returns at the time of writing) and only trip on a structural break.
    expect(files.length).toBeGreaterThan(150)
    const withEarlyReturn = files.reduce((n, p) => n + earlyReturningDecls(readFileSync(p, 'utf8')), 0)
    expect(withEarlyReturn).toBeGreaterThan(50)
  })

  it('flags a hook placed after an early return', () => {
    const broken = [
      'export function X() {',
      '  const [a] = useState(0)',
      '  if (!a) return null',
      '  useEffect(() => {}, [])',
      '  return null',
      '}',
    ].join('\n')
    expect(hooksAfterEarlyReturn(broken)).toEqual([{ name: 'X', line: 4 }])
  })

  it('flags a GENERIC hook call after an early return (the 0.24.6 shape)', () => {
    // Verbatim shape of the field crash: the bail-out, then `useState<{…}>(…)`.
    // The pre-2026-08 pattern ended in `\(` and scored this line clean.
    const broken = [
      'export function OpeningsLogPane() {',
      '  const [episodes, setEpisodes] = useState<OpeningEpisode[]>([])',
      '  if (episodes.length === 0) return null',
      "  const [opSort, setOpSort] = useState<{ key: OpSortKey; asc: boolean }>({ key: 'when', asc: false })",
      '  return null',
      '}',
    ].join('\n')
    expect(hooksAfterEarlyReturn(broken)).toEqual([{ name: 'OpeningsLogPane', line: 4 }])
  })

  it('flags a CUSTOM hook after an early return', () => {
    const broken = [
      'export function usePanes(open: boolean) {',
      '  if (!open) return null',
      '  const w = usePaneWidths()',
      '  return w',
      '}',
    ].join('\n')
    expect(hooksAfterEarlyReturn(broken)).toEqual([{ name: 'usePanes', line: 3 }])
  })

  it('does not blame the next component for the previous one\'s early return', () => {
    const fine = [
      'export function A() {',
      '  if (!ok) return null',
      '  return null',
      '}',
      '',
      'export function B() {',
      '  const [n] = useState(0)',
      '  return n',
      '}',
    ].join('\n')
    expect(hooksAfterEarlyReturn(fine)).toEqual([])
  })
})
