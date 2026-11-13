// The probes' one screen list (probe/sizes.mjs) and the rule that keeps it the only one.
//
// The 2026-11-11 ruling put the floor at 1366×768 and the top at 4K. Before it, every probe spelled its own window
// sizes, and the old 1024×768 floor sat in 27 of them; a ruling that has to be carried into 27 files by hand is a
// ruling some file misses. So: the matrix is pinned here, and a probe that sizes a window imports it — the only
// windows a probe may spell are off the matrix on purpose (`offMatrix`, or a bare [w, h] below any supported screen).
import { readdirSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import { CORE, FLOOR, MATRIX, SCREENS, UHD_200, metrics } from './sizes.mjs'

const DIR = dirname(fileURLToPath(import.meta.url))
/** [file, source] of every desktop probe, read once while the file is collected (not inside a test's timeout). */
const PROBES = readdirSync(DIR).filter((f) => f.endsWith('-probe.mjs')).map((f) => [f, readFileSync(join(DIR, f), 'utf8')])
/** Probes that size no screen: the Rotor box's window is searched for by height, the Spots pop-out by its own floor. */
const NO_SCREEN = new Set(['desktop-probe.mjs', 'spots-probe.mjs'])

describe('probe/sizes.mjs', () => {
  it('is the ruling: eight screens from 1366×768 to 4K, 100–175 % on each and 200 % on 4K', () => {
    expect(SCREENS).toEqual([[1366, 768], [1440, 900], [1536, 864], [1920, 1080], [1920, 1200], [2560, 1440], [2560, 1600], [3840, 2160]])
    expect(MATRIX).toHaveLength(8 * 4 + 1)
    expect(MATRIX.filter((s) => s.scale === 2).map((s) => s.name)).toEqual(['3840×2160 @200 %'])
  })

  it('emulates OS scaling as the webview sees it: the screen divided by the scale, at that pixel ratio', () => {
    expect(FLOOR).toMatchObject({ w: 780, h: 438, scale: 1.75 })
    expect(metrics(UHD_200)).toEqual({ width: 1920, height: 1080, deviceScaleFactor: 2, mobile: false })
    // The floor is the smallest effective window the matrix has.
    expect(Math.min(...MATRIX.map((s) => s.w))).toBe(FLOOR.w)
    expect(Math.min(...MATRIX.map((s) => s.h))).toBe(FLOOR.h)
  })

  it('keeps the representative set to the floor, a mid laptop, 1080p, 1440p and 4K at 200 %', () => {
    expect(CORE.map((s) => s.name)).toEqual(['1366×768 @175 %', '1440×900', '1920×1080', '2560×1440', '3840×2160 @200 %'])
  })

  it('is what every probe that sizes a screen imports', () => {
    const strays = PROBES.filter(([f, src]) => !NO_SCREEN.has(f) && !/from '\.\/sizes\.mjs'/.test(src)).map(([f]) => f)
    expect(strays, 'these probes size windows without probe/sizes.mjs').toEqual([])
  })

  it('leaves no probe on the old 1024×768 floor', () => {
    const old = /\b1024\s*[,x×]\s*768\b|\bw:\s*1024\b|\bwidth:\s*1024\b|\bw\s*=\s*1024\b/
    const left = PROBES.filter(([, src]) => old.test(src)).map(([f]) => f)
    expect(left, 'these probes still open a 1024×768 window').toEqual([])
  })
})
