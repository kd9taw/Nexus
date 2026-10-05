// Copyright notices for the packages actually present in the bundled chunks: the hosted
// browser's (vite.remote.config.ts) and the desktop app's (vite.config.ts).
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
  // The desktop bundle's reviewed exceptions (licenses/ui/REVIEWED-PACKAGES.json; NOTICE names every
  // package in it). `noLicenseFile`: packages that publish no license file, each with the upstream text
  // NOTICE reproduces for it, from the commit its npm package records as its source. `prebuilt`:
  // packages published pre-built, whose own files carry other packages' code where no module id can
  // show it (MapLibre); the packages inside are read as if their modules were bundled.
  const reviewed = JSON.parse(readFileSync(new URL('../licenses/ui/REVIEWED-PACKAGES.json', import.meta.url), 'utf8')) as {
    noLicenseFile: Record<string, { license: string; text: string; sha256: string }>; prebuilt: Record<string, string[]>
  }
  const reviewedTexts = new Map(Object.entries(reviewed.noLicenseFile).map(([key, entry]) => {
    const text = readFileSync(new URL(`../licenses/ui/${entry.text}`, import.meta.url), 'utf8')
    if (createHash('sha256').update(text).digest('hex') !== entry.sha256) throw new Error(`The reviewed license text of ${key} changed`)
    return [key, { license: entry.license, text }] as const
  }))
  const prebuiltNames = new Set(Object.keys(reviewed.prebuilt).map(key => key.slice(0, key.lastIndexOf(' '))))
  // A Set's iteration visits what is added during it, so a pre-built package's inner packages join the walk.
  const ids = new Set(moduleIds)
  for (const moduleId of ids) {
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
            const files = readdirSync(directory, { withFileTypes: true })
              .filter(entry => entry.isFile()).map(entry => entry.name).sort()
            const names = files.filter(name => /^(licen[cs]e|copying)([._-].*)?$/i.test(name))
            // Apache-2.0 §4(d): the attribution in a package's NOTICE file travels with its
            // license (h3-js has one). It never stands in for a missing license file.
            const notices = files.filter(name => /^notice([._-].*)?$/i.test(name))
            // Radix's published leaf packages omit the monorepo license file.
            // Their package metadata declares MIT and this repository; the
            // retained text comes from its last LICENSE change (see NOTICE).
            const radix = !names.length && pkg.name.startsWith('@radix-ui/') && pkg.license === 'MIT'
              && typeof pkg.repository !== 'string' && pkg.repository?.url === 'git+https://github.com/radix-ui/primitives.git'
              && radixReview.packages[pkg.name] === pkg.version
            const scroll = !names.length && pkg.name === scrollReview.name && pkg.version === scrollReview.version
              && pkg.license === scrollReview.license && pkg.repository === scrollReview.repository
            const fallback = names.length ? undefined : reviewedTexts.get(key)
            const reviewedText = fallback && fallback.license === pkg.license ? fallback.text : undefined
            if (!names.length && !radix && !scroll && !reviewedText) { missing.add(key); break }
            const read = (name: string) => readFileSync(join(directory, name), 'utf8')
            packages.set(key, [...(radix ? [radixLicense] : scroll ? [scrollLicense] : reviewedText ? [reviewedText] : names.map(read)), ...notices.map(read)].join('\n\n'))
            if (prebuiltNames.has(pkg.name) && !reviewed.prebuilt[key]) {
              throw new Error(`${key} is published pre-built with other packages inside it, and licenses/ui/REVIEWED-PACKAGES.json lists them for another version`)
            }
            for (const inner of reviewed.prebuilt[key] ?? []) {
              const manifest = installedManifest(directory, inner.slice(0, inner.lastIndexOf(' ')))
              if (manifest) ids.add(manifest)
              else missing.add(inner)
            }
          }
          break
        }
      }
      directory = dirname(directory)
    }
  }
  if (missing.size) throw new Error(`Bundled packages missing license texts: ${[...missing].sort().join(', ')}`)
  for (const [key, inside] of Object.entries(reviewed.prebuilt)) {
    const other = packages.has(key) ? inside.filter(inner => !packages.has(inner)) : []
    if (other.length) throw new Error(`${key} carries ${other.join(', ')} inside its files, and another version is installed: review licenses/ui/REVIEWED-PACKAGES.json`)
  }
  if (![...packages.keys()].some(name => name.startsWith('react '))) throw new Error('The hosted dependency license inventory is incomplete')
  return [...packages].sort(([a], [b]) => a.localeCompare(b, 'en')).map(([name, license]) => `${name}\n${license}`).join('\n\n')
}

/** The package.json of `name` as Node resolves it for the package in `directory`: the nearest
 *  node_modules folder up the tree that holds it. */
function installedManifest(directory: string, name: string): string | undefined {
  for (let dir = directory; ; dir = dirname(dir)) {
    const manifest = join(dir, 'node_modules', name, 'package.json')
    if (existsSync(manifest)) return manifest
    if (dirname(dir) === dir) return undefined
  }
}
