// NO VIEW READS THE WHOLE LOG (SPEC-2 v2 R6 §8; v3 C17b). A census the compiler takes, and a ratchet.
//
// At 150k contacts every window held the whole log as parsed rows (195 MiB) and re-pulled it after
// every upload stamp (141 MB of JSON). C17b moves each view onto `LogSource` (features/logSource.ts),
// which answers the questions a view asks instead of handing it the log. This guard is what keeps a
// moved view moved, and stops a new one reading the log:
//
//   - it PARSES every non-test module under `ui/src` with the TypeScript compiler (never a grep) and
//     finds each way to reach the whole log: an import of `features/logStore`; an import or a use of
//     `getLog` / `getLogDelta` / `LogDelta` from `api`; and `invoke('get_log' | 'get_log_delta')`;
//   - the modules that may do so are named below, and the census must EQUAL that list. A new
//     reader fails; so does a module on the list that no longer reads it — remove it, and the list
//     can only shrink. It is the progress meter: C17b empties STILL_READING, C17a deletes the store,
//     the adapter and the api calls, and WHOLE_LOG_ADAPTER goes with them.
//
// Its own positive controls run on every pass: a detector that has only ever been green is untested.

import { describe, expect, it } from 'vitest'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { dirname, join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import ts from 'typescript'

const SRC = resolve(dirname(fileURLToPath(import.meta.url)), '..')

/** Where the whole log is allowed to be read: the api calls, the window's copy of it, and the one
 *  `LogSource` adapter answering from that copy until the engine answers (C17a deletes all three). */
const WHOLE_LOG_ADAPTER = ['api.ts', 'features/logStore.ts', 'features/wholeLogSource.ts']

/** Views not yet moved onto `LogSource`. Each C17b step removes the views it moves. */
const STILL_READING = [
  'DetachedPanel.tsx',
  'components/AwardsView.tsx',
  'components/Globe3D.tsx',
  'components/Js8Cockpit.tsx',
  'components/LogEntry.tsx',
  'components/Logbook.tsx',
  'components/MapView.tsx',
  'components/OperateCockpit.tsx',
  'components/StatsView.tsx',
]

const WHOLE_LOG_API = new Set(['getLog', 'getLogDelta', 'LogDelta'])
const WHOLE_LOG_COMMANDS = new Set(['get_log', 'get_log_delta'])

/** How `file` (a path under src) reaches the whole log, one line per finding. */
export function wholeLogReads(file: string, source: string): string[] {
  const sf = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true, file.endsWith('x') ? ts.ScriptKind.TSX : ts.ScriptKind.TS)
  const found: string[] = []
  const at = (node: ts.Node) => sf.getLineAndCharacterOfPosition(node.getStart(sf)).line + 1
  /** The module a relative specifier names, as a path under src without its extension. */
  const target = (spec: string) =>
    spec.startsWith('.') ? relative(SRC, resolve(SRC, dirname(file), spec)).replace(/\.(tsx?|js)$/, '') : spec
  const apiNamespaces = new Set<string>()

  const visit = (node: ts.Node): void => {
    if ((ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) && node.moduleSpecifier && ts.isStringLiteral(node.moduleSpecifier)) {
      const mod = target(node.moduleSpecifier.text)
      if (mod === 'features/logStore') found.push(`${file}:${at(node)} imports features/logStore`)
      if (mod === 'api') {
        const clause = ts.isImportDeclaration(node) ? node.importClause?.namedBindings : node.exportClause
        if (clause && ts.isNamespaceImport(clause)) apiNamespaces.add(clause.name.text)
        if (clause && (ts.isNamedImports(clause) || ts.isNamedExports(clause)))
          for (const el of clause.elements) {
            const name = (el.propertyName ?? el.name).text
            if (WHOLE_LOG_API.has(name)) found.push(`${file}:${at(el)} takes ${name} from api`)
          }
      }
    }
    if (ts.isCallExpression(node)) {
      const callee = node.expression
      // A dynamic import of the store is still an import of it.
      if (callee.kind === ts.SyntaxKind.ImportKeyword) {
        const [arg] = node.arguments
        if (arg && ts.isStringLiteral(arg) && target(arg.text) === 'features/logStore')
          found.push(`${file}:${at(node)} imports features/logStore`)
      }
      const name = ts.isIdentifier(callee) ? callee.text : ts.isPropertyAccessExpression(callee) ? callee.name.text : ''
      const [first] = node.arguments
      if (name === 'invoke' && first && ts.isStringLiteralLike(first) && WHOLE_LOG_COMMANDS.has(first.text))
        found.push(`${file}:${at(node)} invokes ${first.text}`)
    }
    if (ts.isPropertyAccessExpression(node) && ts.isIdentifier(node.expression) && apiNamespaces.has(node.expression.text) && WHOLE_LOG_API.has(node.name.text))
      found.push(`${file}:${at(node)} reads api.${node.name.text}`)
    ts.forEachChild(node, visit)
  }
  visit(sf)
  // A namespace import is only known once its declaration is visited; uses above it are rare
  // enough (none exist) that one pass suffices, and the control below pins the ordinary order.
  return found
}

function modules(dir: string): string[] {
  const out: string[] = []
  for (const name of readdirSync(dir)) {
    const path = join(dir, name)
    if (statSync(path).isDirectory()) {
      if (name === 'node_modules' || name === '__fixtures__') continue
      out.push(...modules(path))
    } else if (/\.tsx?$/.test(name) && !/\.test\.tsx?$/.test(name) && !name.endsWith('.d.ts') && name !== 'test-setup.ts') {
      out.push(relative(SRC, path))
    }
  }
  return out
}

describe('no view reads the whole log (the census, as a ratchet)', () => {
  it('the detector fires on every way in, and not on a near miss (positive and negative controls)', () => {
    const hits = (file: string, src: string) => wholeLogReads(file, src).length
    expect(hits('components/Fake.tsx', `import { useSharedLog } from '../features/logStore'`)).toBe(1)
    expect(hits('Fake.tsx', `import { NO_LOG } from './features/logStore'`)).toBe(1)
    expect(hits('components/Fake.tsx', `const m = import('../features/logStore')`)).toBe(1)
    expect(hits('components/Fake.tsx', `export { loadSharedLog } from '../features/logStore'`)).toBe(1)
    expect(hits('components/Fake.tsx', `import { getLogDelta, qrzLookup } from '../api'`)).toBe(1)
    expect(hits('components/Fake.tsx', `import { getLog as all } from '../api'`)).toBe(1)
    expect(hits('components/Fake.tsx', `import type { LogDelta } from '../api'`)).toBe(1)
    expect(hits('components/Fake.tsx', `import * as api from '../api'\nvoid api.getLog()`)).toBe(1)
    expect(hits('api.ts', `export async function x() { return invoke('get_log_delta', {}) }`)).toBe(1)
    // Near misses: a name that merely STARTS with getLog, a type from elsewhere, another command.
    expect(hits('components/Fake.tsx', `import { getLogStats, getLogbookSaving } from '../api'`)).toBe(0)
    expect(hits('components/Fake.tsx', `import type { LoggedQso } from '../types'\nimport { logSource } from '../features/logSource'`)).toBe(0)
    expect(hits('api.ts', `invoke('get_log_stats')`)).toBe(0)
    expect(hits('components/Fake.tsx', `import * as api from '../api'\nvoid api.getLogStats()`)).toBe(0)
  })

  it('the modules reading the whole log are exactly the adapter and the views not yet moved', () => {
    const readers = new Map<string, string[]>()
    for (const file of modules(SRC)) {
      const found = wholeLogReads(file, readFileSync(join(SRC, file), 'utf8'))
      if (found.length) readers.set(file, found)
    }
    // Positive control on the REAL tree: the store itself must be found, or the walk is blind.
    expect(readers.has('features/logStore.ts'), 'the census did not see the store it exists to police').toBe(true)
    const allowed = [...WHOLE_LOG_ADAPTER, ...STILL_READING].sort()
    const actual = [...readers.keys()].sort()
    const unexpected = actual.filter((f) => !allowed.includes(f)).map((f) => readers.get(f)!.join('\n'))
    expect(unexpected, 'a NEW whole-log reader — ask LogSource (features/logSource.ts) instead').toEqual([])
    const moved = allowed.filter((f) => !actual.includes(f))
    expect(moved, 'no longer reads the whole log — remove it from STILL_READING (the list only shrinks)').toEqual([])
  })
})
