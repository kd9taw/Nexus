// Copyright notices for the packages actually present in the hosted chunks.
// Reuse installed, lockfile-verified license texts; never invent a notice or
// publish an arbitrary node_modules file. A missing notice stops packaging.
import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { createHash } from 'node:crypto'

export function bundledLicenses(moduleIds: Iterable<string>): string {
  const packages = new Map<string, string>()
  const missing = new Set<string>()
  const radixReview = JSON.parse(readFileSync(new URL('../licenses/remote/RADIX-PACKAGES.json', import.meta.url), 'utf8')) as {
    sha256: string; packages: Record<string, string>
  }
  const radixLicense = readFileSync(new URL('../licenses/remote/RADIX-LICENSE.txt', import.meta.url), 'utf8')
  if (createHash('sha256').update(radixLicense).digest('hex') !== radixReview.sha256) throw new Error('The reviewed Radix license text changed')
  const scrollReview = JSON.parse(readFileSync(new URL('../licenses/remote/REACT-REMOVE-SCROLL-BAR-PACKAGE.json', import.meta.url), 'utf8')) as {
    sha256: string; name: string; version: string; license: string; repository: string
  }
  const scrollLicense = readFileSync(new URL('../licenses/remote/REACT-REMOVE-SCROLL-BAR-LICENSE.txt', import.meta.url), 'utf8')
  if (createHash('sha256').update(scrollLicense).digest('hex') !== scrollReview.sha256) throw new Error('The reviewed scroll-bar license text changed')
  for (const moduleId of moduleIds) {
    const path = moduleId.replace(/^\0/, '').replaceAll('\\', '/').split('?')[0]
    if (!path.includes('/node_modules/')) continue
    let directory = dirname(path)
    while (directory.includes('/node_modules/')) {
      const manifest = join(directory, 'package.json')
      if (existsSync(manifest)) {
        const pkg = JSON.parse(readFileSync(manifest, 'utf8')) as { name?: string; version?: string; license?: string; repository?: string | { url?: string } }
        if (pkg.name && pkg.version) {
          const key = `${pkg.name} ${pkg.version}`
          if (!packages.has(key)) {
            const names = readdirSync(directory, { withFileTypes: true })
              .filter(entry => entry.isFile() && /^(licen[cs]e|copying)([._-].*)?$/i.test(entry.name))
              .map(entry => entry.name).sort()
            // Radix's published leaf packages omit the monorepo license file.
            // Their package metadata declares MIT and this repository; the
            // retained text comes from its last LICENSE change (see NOTICE).
            const radix = !names.length && pkg.name.startsWith('@radix-ui/') && pkg.license === 'MIT'
              && typeof pkg.repository !== 'string' && pkg.repository?.url === 'git+https://github.com/radix-ui/primitives.git'
              && radixReview.packages[pkg.name] === pkg.version
            const scroll = !names.length && pkg.name === scrollReview.name && pkg.version === scrollReview.version
              && pkg.license === scrollReview.license && pkg.repository === scrollReview.repository
            if (!names.length && !radix && !scroll) { missing.add(key); break }
            packages.set(key, radix ? radixLicense : scroll ? scrollLicense
              : names.map(name => readFileSync(join(directory, name), 'utf8')).join('\n\n'))
          }
          break
        }
      }
      directory = dirname(directory)
    }
  }
  if (missing.size) throw new Error(`Bundled packages missing license texts: ${[...missing].sort().join(', ')}`)
  if (![...packages.keys()].some(name => name.startsWith('react '))) throw new Error('The hosted dependency license inventory is incomplete')
  return [...packages].sort(([a], [b]) => a.localeCompare(b, 'en')).map(([name, license]) => `${name}\n${license}`).join('\n\n')
}
