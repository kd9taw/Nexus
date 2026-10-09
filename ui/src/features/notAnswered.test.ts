// A read the engine refused because a change made before it is still being saved is asked again a
// second later, three times at most; any other failure is not asked again at all.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ASK_AGAIN_AFTER_MS, ASK_AGAIN_TIMES, NOT_ANSWERED, askAgainWhileSaving, notAnswered } from './notAnswered'

/** The refusal as the desktop's IPC rejects with it: the engine's words, a bare string. */
const refusal = `${NOT_ANSWERED}: a logbook change is still on its way (0 of 1 saved)`

/** How `p` settled, without an unhandled rejection while the test drives the clock. */
function settled<T>(p: Promise<T>): Promise<{ ok: T } | { failed: unknown }> {
  return p.then(
    (ok) => ({ ok }),
    (failed) => ({ failed }),
  )
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
})
afterEach(() => {
  vi.useRealTimers()
})

describe('a refused log read', () => {
  it('is asked again a second later, and the answer is the one that ask got', async () => {
    const read = vi.fn<() => Promise<string>>().mockRejectedValueOnce(refusal).mockResolvedValue('with the contact')
    const outcome = settled(askAgainWhileSaving(read))
    await vi.advanceTimersByTimeAsync(ASK_AGAIN_AFTER_MS - 1)
    expect(read, 'not asked again before the second is up').toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(1)
    expect(read).toHaveBeenCalledTimes(2)
    expect(await outcome).toEqual({ ok: 'with the contact' })
  })

  it(`is asked again ${ASK_AGAIN_TIMES} times at most, then rejects with the refusal`, async () => {
    const read = vi.fn<() => Promise<string>>().mockRejectedValue(refusal)
    const outcome = settled(askAgainWhileSaving(read))
    await vi.advanceTimersByTimeAsync(ASK_AGAIN_TIMES * ASK_AGAIN_AFTER_MS)
    expect(read).toHaveBeenCalledTimes(1 + ASK_AGAIN_TIMES)
    expect(await outcome).toEqual({ failed: refusal })
    await vi.advanceTimersByTimeAsync(60_000)
    expect(read, 'and never again').toHaveBeenCalledTimes(1 + ASK_AGAIN_TIMES)
  })

  it('stops asking once the view no longer wants the answer', async () => {
    let open = true
    const read = vi.fn<() => Promise<string>>().mockRejectedValue(refusal)
    const outcome = settled(askAgainWhileSaving(read, () => open))
    open = false
    await vi.advanceTimersByTimeAsync(10 * ASK_AGAIN_AFTER_MS)
    expect(read).toHaveBeenCalledTimes(1)
    expect(await outcome).toEqual({ failed: refusal })
  })
})

describe('any other failure', () => {
  it.each([
    ['an error', new Error('the logbook could not be read: disk I/O error')],
    ['a string', 'the logbook could not be read: disk I/O error'],
    ['the refusal, not at the start', `the logbook could not be read: ${NOT_ANSWERED}`],
  ])('is not asked again: %s', async (_, failure) => {
    const read = vi.fn<() => Promise<string>>().mockRejectedValue(failure)
    const outcome = settled(askAgainWhileSaving(read))
    await vi.advanceTimersByTimeAsync(10 * ASK_AGAIN_AFTER_MS)
    expect(read).toHaveBeenCalledTimes(1)
    expect(await outcome).toEqual({ failed: failure })
  })

  it('an Error carrying the refusal is the refusal', () => {
    expect(notAnswered(new Error(refusal))).toBe(true)
    expect(notAnswered(refusal)).toBe(true)
    expect(notAnswered(null)).toBe(false)
    expect(notAnswered({ message: refusal })).toBe(false)
  })
})
