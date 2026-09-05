// @vitest-environment node
//
// ⚠️ A MIME TYPE IN `accept` MAKES A FILE PICKER THAT CAN HIDE THE FILE, WITH NO WAY OUT.
//
// Reported as "the file chooser never shows the adif file" on Linux. The dialog opens — that
// part of the theory was wrong — but WebKitGTK builds it in a way that turns `accept` into a
// trap. Verified by reading the shipped library's imports:
//
//     gtk_file_filter_add_mime_type   imported
//     gtk_file_chooser_set_filter     imported
//     gtk_file_filter_add_pattern     NOT imported
//     gtk_file_chooser_add_filter     NOT imported
//
// So file EXTENSIONS in `accept` are discarded outright — WebKit cannot express them — while a
// MIME type becomes ONE filter installed with `set_filter`. Because `add_filter` is not
// imported there is no filter dropdown and no "All Files" entry, so the operator has no way to
// widen it. GTK then types each file by sniffing its CONTENT, so whether their log survives the
// filter depends on what the sniffer decides about it rather than on what they named it.
//
// With NO MIME type in `accept`, WebKit builds no filter at all and every file is listed. The
// POTA input already did that by accident and was the one input nobody reported a problem with.
// The extensions cost nothing to keep: browsers and macOS still honour them, and on Linux they
// were never reaching GTK either way.
import { describe, it, expect } from 'vitest'
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join } from 'node:path'

function walk(dir: string, out: string[] = []): string[] {
  for (const e of readdirSync(dir)) {
    const p = join(dir, e)
    if (statSync(p).isDirectory()) walk(p, out)
    else if (/\.tsx?$/.test(e) && !/\.test\.tsx?$/.test(e)) out.push(p)
  }
  return out
}

describe('file pickers never install a filter the operator cannot escape', () => {
  const files = walk(new URL('.', import.meta.url).pathname)

  it('no accept attribute carries a MIME type', () => {
    const offenders: string[] = []
    for (const f of files) {
      const src = readFileSync(f, 'utf8')
      for (const m of src.matchAll(/accept="([^"]*)"/g)) {
        // A MIME type is the only form with a slash; extensions are ".adi", ".json", ….
        if (m[1].includes('/')) offenders.push(`${f.split('/ui/src/')[1]}: accept="${m[1]}"`)
      }
    }
    expect(offenders, 'a MIME type here can hide the file with no way to widen the filter').toEqual([])
  })

  it('POSITIVE CONTROL: the check can see accept attributes at all', () => {
    // Otherwise the assertion above would pass against a walk that found nothing.
    const found = files.filter((f) => /accept="/.test(readFileSync(f, 'utf8')))
    expect(found.length, 'the app does have file inputs to check').toBeGreaterThan(2)
  })
})
