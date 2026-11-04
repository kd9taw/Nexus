import { describe, expect, it } from 'vitest'
import { FRESH, observe, ROTOR_GRACE_MISSES, shown, type Liveness } from './rotorLiveness'
import type { RotatorState } from '../types'

const AT = (azDeg: number): RotatorState => ({ azDeg, reading: 'position', elDeg: 45 })
const SILENT: RotatorState = { azDeg: null, reading: 'notAnswering' }

const run = (reads: (RotatorState | null | 'failed')[]): Liveness => reads.reduce(observe, FRESH)

describe('the rotor pane grace', () => {
  it('a single missed poll keeps the last reading, shown as stale, not "not answering"', () => {
    const v = shown(run([AT(213), SILENT]))
    expect(v.stale).toBe(true)
    expect(v.state?.azDeg).toBe(213)
    expect(v.state?.reading).toBe('position')
  })

  it(`says "not answering" after ${ROTOR_GRACE_MISSES} misses in a row, and not before`, () => {
    const reads: RotatorState[] = [AT(213)]
    for (let i = 1; i < ROTOR_GRACE_MISSES; i++) {
      reads.push(SILENT)
      expect(shown(run(reads)).state?.reading).toBe('position')
    }
    reads.push(SILENT)
    expect(shown(run(reads))).toEqual({
      state: { azDeg: null, reading: 'notAnswering', elDeg: null },
      stale: false,
    })
  })

  it('one answer forgives the misses', () => {
    const l = run([AT(213), SILENT, SILENT, AT(214), SILENT, SILENT])
    expect(shown(l).state?.azDeg).toBe(214)
    expect(shown(run([AT(213), SILENT, AT(215)]))).toEqual({ state: AT(215), stale: false })
  })

  it('a rotator that never answered is called not answering at once: nothing to show stale', () => {
    expect(shown(run([SILENT])).state?.reading).toBe('notAnswering')
  })

  it('the DCU-1, which answers with no position, keeps its own words through a miss', () => {
    const none: RotatorState = { azDeg: null, reading: 'noPosition' }
    expect(shown(run([none, SILENT])).state?.reading).toBe('noPosition')
  })

  it('a failed read falls back the same way, and after the grace shows nothing, not "not answering"', () => {
    expect(shown(run([AT(10), 'failed'])).state?.azDeg).toBe(10)
    expect(shown(run([AT(10), 'failed', 'failed', 'failed']))).toEqual({ state: null, stale: false })
  })

  it('no rotator configured starts again', () => {
    expect(shown(run([AT(10), SILENT, null]))).toEqual({ state: null, stale: false })
  })
})
