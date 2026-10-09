// A LOG READ REFUSED WHILE A CHANGE IS STILL BEING SAVED — and asking it again.
//
// The engine never answers a read of the logbook without every change made before it was asked:
// a list asked straight after a contact shows the contact. When the change has not reached the
// logbook database within the read's own wait (two seconds: a slow disk, an import ahead of it),
// the read is REFUSED, and the refusal begins with `NOT_ANSWERED`.
//
// Nothing else asks again for it. The window's change feed (`loggedTick`, `logTick`) moved when
// the change was made, before the refusal, so a view that waits for the next change would keep its
// old answer until the next contact, and a view that reads once would show its failed state. So a
// refused read is asked again a second later, three times at most. Any other failure, and the last
// refusal, are the caller's to handle exactly as before.

/** How the engine's refusal begins (`tempo_app::logstore::NOT_ANSWERED`; wire-consistency.test.ts
 *  holds the two together). */
export const NOT_ANSWERED = 'the logbook has not saved every change made before this was asked'

/** How long after a refusal the read is asked again. */
export const ASK_AGAIN_AFTER_MS = 1_000

/** How many times a refused read is asked again: four asks in all. Each refusal comes after the
 *  engine has already waited up to two seconds for the change, so the last ask goes out about
 *  nine seconds after the first, and is answered by about eleven. */
export const ASK_AGAIN_TIMES = 3

/** Whether `e` is that refusal: the engine's words, as the desktop's IPC rejects with them. */
export function notAnswered(e: unknown): boolean {
  const words = e instanceof Error ? e.message : e
  return typeof words === 'string' && words.startsWith(NOT_ANSWERED)
}

/**
 * `read()`, asked again `ASK_AGAIN_AFTER_MS` after each refusal, `ASK_AGAIN_TIMES` times at most.
 * Rejects at once with any other failure, and with the last refusal. `wanted` says whether the
 * asker still wants the answer (the view is still open): once it does not, nothing more is asked.
 */
export async function askAgainWhileSaving<T>(read: () => Promise<T>, wanted: () => boolean = () => true): Promise<T> {
  for (let again = 0; ; again++) {
    try {
      return await read()
    } catch (e) {
      if (!notAnswered(e) || again === ASK_AGAIN_TIMES) throw e
      await new Promise((resolve) => setTimeout(resolve, ASK_AGAIN_AFTER_MS))
      if (!wanted()) throw e
    }
  }
}
