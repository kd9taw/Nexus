// CANVAS BACKING-STORE CHURN — the guard that keeps the sawtooth dead.
//
// Assigning canvas.width/height DISCARDS the bitmap and allocates a new one even when
// the value is unchanged. Three draw paths did that on a timer: the map's main canvas
// (deps carry a 1 s pulseTick), its flare overlay (every view change), and MiniSpectrum
// (every 120 ms spectrum poll). At 3440x1440 the map's backing store is ~20 MB, which
// an operator watched oscillate 145->168 MB in Task Manager at a 1 s heartbeat
// (2026-08-01). GC kept up — no leak — but it is ~20 MB/s of garbage for no redraw
// benefit, and it is a documented jank source on weaker hardware.
//
// MiniSpectrum and the Waterfall's picture draw through the spectrum renderer (ui/src/spectrum,
// 2026-10-04), so their backing stores are the renderer's now: its two backends are in the census
// in MiniSpectrum's place. MiniSpectrum still sizes its trace on every poll, and that is safe only
// because the renderer's resize is guarded here.
//
// This is a CSS-TEXT-STYLE census over the TSX (the codebase's guard-test idiom), and
// like its siblings it computes rather than greps for presence: for every
// `canvas.width = ...` assignment in the draw paths it walks BACKWARD for the guarding
// `if` and asserts the file's own comparison shape. A future draw path that assigns
// unguarded fails here, not in a memory profiler six months later.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'

const read = (rel: string) => readFileSync(new URL(rel, import.meta.url), 'utf8')

/** Every `.width = ...` backing-store assignment, with the 8 lines preceding it.
 *  Comments are dropped first so prose about the trap can never satisfy the guard —
 *  the lesson aprsLayout.test.ts learned the hard way. */
function widthAssignments(src: string): { line: string; ctx: string }[] {
  const lines = src
    .split('\n')
    .map((l) => l.replace(/\/\/.*$/, ''))
  const out: { line: string; ctx: string }[] = []
  lines.forEach((line, i) => {
    // Any `<ident>.width =` on the line, including the inline `if (x) y.width = z` form.
    // `.style.width` is CSS layout, not a backing store — it allocates nothing, and neither does
    // a width of 0, which RELEASES one (the renderer's destroy).
    if (/[A-Za-z_$][\w$]*\.width\s*=[^=]/.test(line) && !/\.style\.width/.test(line) && !/\.width\s*=\s*0\b/.test(line)) {
      // The same line counts as context: an inline if-guard lives there.
      out.push({ line: line.trim(), ctx: lines.slice(Math.max(0, i - 8), i + 1).join('\n') })
    }
  })
  return out
}

/** A real size-change guard: an inequality against the intended device size, or the
 *  equality-then-return form (the Waterfall's against `devW`, the canvas-2D renderer's against
 *  the size it holds). */
const GUARDED = /\.width\s*!==|\.height\s*!==|!==\s*(?:dev|hw|hh)|===\s*(?:dev[WH]|this\.w)\b[\s\S]*?return/

describe('canvas backing stores are only reallocated on a real size change', () => {
  it.each([
    'components/MapView.tsx',
    'spectrum/canvas2d.ts',
    'spectrum/webgl2.ts',
    'components/Waterfall.tsx',
    'components/PhoneScope.tsx',
  ])('%s guards every canvas width assignment', (rel) => {
    const hits = widthAssignments(read(`./${rel}`))
    expect(hits.length, `no backing-store assignment found in ${rel} — did the file move?`)
      .toBeGreaterThan(0)
    for (const { line, ctx } of hits) {
      expect(GUARDED.test(ctx), `unguarded backing-store assignment: ${line}\ncontext:\n${ctx}`)
        .toBe(true)
    }
  })

  it('no cumulative scale where a guarded resize keeps the matrix (MiniSpectrum, the renderer)', () => {
    // With the resize guarded, the matrix is no longer reset each draw, so a
    // cumulative ctx.scale(dpr, dpr) would compound every frame and march the trace
    // off-canvas. This is the trap the guard creates; pin the cure. MiniSpectrum's trace is the
    // renderer's now, drawn in device pixels with no transform at all.
    for (const rel of ['./components/MiniSpectrum.tsx', './spectrum/canvas2d.ts']) {
      const src = read(rel)
      expect(src, rel).not.toMatch(/\.scale\(/)
      expect(src, rel).not.toMatch(/\.setTransform\(/)
    }
  })

  it('the map draw still clears unconditionally (the resize used to do it)', () => {
    const src = read('./components/MapView.tsx')
    const draw = src.slice(src.indexOf('const ctx = canvas.getContext'))
    const head = draw.slice(0, 400)
    expect(head).toMatch(/setTransform\(dpr, 0, 0, dpr, 0, 0\)/)
    expect(head).toMatch(/clearRect\(0, 0, w, h\)/)
  })
})
