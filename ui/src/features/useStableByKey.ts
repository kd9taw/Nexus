import { useRef } from 'react'

/**
 * Hand back the SAME reference for as long as `key` — a serialisation of what the value
 * draws — is unchanged.
 *
 * Why it exists: App publishes a new `stations` array (and new station objects) on every
 * 300 ms snapshot whether or not anything was decoded. Every memo and effect keyed on that
 * identity re-runs three times a second for identical content: the 3-D globe re-digested its
 * arc layer (three-globe disposes each arc's material, so a shader-program compile per arc)
 * and the 2-D map redrew and re-projected its whole canvas. Keying on content turns "a new
 * array arrived" back into "something on the map changed".
 *
 * The key is the caller's to choose, and it must cover every field the consumer reads — a
 * field left out is a change that will not repaint.
 */
export function useStableByKey<T>(value: T, key: string): T {
  const ref = useRef<{ key: string; value: T } | null>(null)
  if (ref.current === null || ref.current.key !== key) ref.current = { key, value }
  return ref.current.value
}
