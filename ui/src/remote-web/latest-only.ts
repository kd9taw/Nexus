/** Send a dragged value to the station without queueing every intermediate step.
 *
 * A browser holds one station command at a time, so sending each slider step would refuse all but
 * the first. This keeps one request in flight and, when it settles, sends only the newest value the
 * operator moved to while it was out (none if they did not move). Each failure is reported once. */
export function latestOnly<T>(send: (value: T) => Promise<unknown>, onError: (error: unknown) => void): (value: T) => void {
  let busy = false
  let next: { value: T } | null = null
  const run = (value: T): void => {
    busy = true
    void send(value).catch(onError).finally(() => {
      busy = false
      const queued = next
      next = null
      if (queued) run(queued.value)
    })
  }
  return (value: T) => {
    if (busy) next = { value }
    else run(value)
  }
}
