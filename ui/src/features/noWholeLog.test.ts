// NO MODULE READS THE WHOLE LOG (SPEC-2 v2 R6 §8; v3 C17). A census the compiler takes, at zero.
//
// At 150k contacts every window held the whole log as parsed rows (195 MiB) and re-pulled it after
// every upload stamp (141 MB of JSON). C17b moved each view onto `LogSource` (features/logSource.ts),
// which answers the questions a view asks instead of handing it the log, and C17 deleted the whole-log
// path: the store, its adapter, the api calls and the engine's commands. This guard keeps it deleted:
//
//   - it PARSES every non-test module under `ui/src` with the TypeScript compiler (never a grep) and
//     finds each way to reach the whole log: an import of `features/logStore`; an import or a use of
//     `getLog` / `getLogDelta` / `LogDelta` from `api`; and `invoke('get_log' | 'get_log_delta')`;
//   - NO module may: the census must be empty. It was the progress meter — C17b emptied the views'
//     list, C17 deleted the rest — and a module that reads the whole log again fails here, with where.
//
// Its own positive controls run on every pass: a detector that has only ever been green is untested.

import { describe, expect, it } from 'vitest'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { dirname, join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import ts from 'typescript'

const SRC = resolve(dirname(fileURLToPath(import.meta.url)), '..')

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

  it('no module reads the whole log', () => {
    const files = modules(SRC)
    // The walk is not blind: it reaches the api, and the source every view reads the log through.
    expect(files).toEqual(expect.arrayContaining(['api.ts', 'features/logSource.ts']))
    // Positive control on a REAL module: the api as it stands, with a whole-log read planted in it.
    const api = readFileSync(join(SRC, 'api.ts'), 'utf8')
    expect(wholeLogReads('api.ts', `${api}\nexport async function planted() { return invoke('get_log_delta', {}) }`)).toHaveLength(1)
    const readers = files.flatMap((file) => wholeLogReads(file, readFileSync(join(SRC, file), 'utf8')))
    expect(readers, 'a whole-log reader — ask LogSource (features/logSource.ts) instead').toEqual([])
  })
})
