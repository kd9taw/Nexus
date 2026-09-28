// WHO MAY CALL `setSettings` DIRECTLY. `set_settings` takes a whole struct, so a module that saves a
// copy of the settings it has been holding writes every field changed elsewhere since back over the
// live value — the bug `patch.ts` exists for, which three surfaces had. Everything else writes through
// the patch seam. The modules below import `setSettings` themselves, and each reads the settings
// immediately before it writes. A module that starts importing it fails here until it is routed
// through the seam or added with the reason it is safe, and one that stops must come off the list.
import { describe, it, expect } from 'vitest'
import { readFileSync, readdirSync } from 'node:fs'
import { dirname, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

const src = resolve(dirname(fileURLToPath(import.meta.url)), '..')

/** The modules that may import `setSettings`, and why each is safe. */
const DIRECT: Record<string, string> = {
  'settings/patch.ts': 'the seam itself',
  'App.tsx': 'memory recall and the setup wizard read the settings immediately before writing',
  'DetachedPanel.tsx': 'memory recall in a pop-out reads the settings immediately before writing',
  'components/AprsCockpit.tsx': 'the board reads the settings immediately before writing',
  'components/OperateCockpit.tsx': 'the special-op chips read the settings immediately before writing',
  'components/SatellitesView.tsx': 'the Doppler switch reads the settings immediately before writing',
  'components/SettingsPanel.tsx': 'loading a named configuration merges it over a fresh read',
}

const API = String.raw`['"](?:\.{1,2}/)+api['"]`

/** The production modules that can reach the api module's `setSettings`, under any name. */
function importers(): string[] {
  const found: string[] = []
  for (const rel of readdirSync(src, { recursive: true }) as string[]) {
    if (!/\.tsx?$/.test(rel) || /\.test\.tsx?$/.test(rel) || rel.split(sep).includes('__fixtures__')) continue
    const text = readFileSync(resolve(src, rel), 'utf8')
    const named = [...text.matchAll(new RegExp(String.raw`import\s*\{([^}]*)\}\s*from\s*` + API, 'g'))]
      .some((m) => /\bsetSettings\b/.test(m[1]))
    // A namespace or dynamic import reaches it as `api.setSettings`.
    const whole = new RegExp(String.raw`import\s*\*\s*as\s+\w+\s+from\s*` + API + String.raw`|import\(\s*` + API + String.raw`\s*\)`).test(text)
    if (named || (whole && /\.setSettings\b/.test(text))) found.push(rel.split(sep).join('/'))
  }
  return found.sort()
}

describe('setSettings is imported only where a fresh read comes first', () => {
  it('by exactly the modules listed, each with its reason', () => {
    expect(importers()).toEqual(Object.keys(DIRECT).sort())
  })
})
