// JS8Call's typeahead in the JS8 compose box (mainwindow.cpp:3590-3628): while the operator's message
// goes out, the box keeps what is still to go and takes more typing, and what has gone moves out of it
// into the line beside it. The ENGINE frames the box's newest text just before each frame
// (`Station::type_ahead`); this keeps the box and the engine in step. A MSG or relay message is
// read-only while it goes, as in JS8Call. Native only: the Remote has no compose box.
import { useMemo, useRef, useState } from 'react'
import { js8Typeahead, js8TypeaheadState } from '../api'
import type { Js8Typeahead } from '../types'
import { showWhenDriving } from '../features/drivingRefusal'

export interface Js8TypeaheadBox {
  /** A message of the box's is going out. */
  active: boolean
  /** What has gone of it, as its frames rendered. */
  sent: string
  /** MSG or a relay: the box may not change it. */
  locked: boolean
  /** After a Send the engine took: the box keeps the message while it goes, if it does. */
  begin(): Promise<boolean>
  /** The operator changed the box. */
  type(text: string): void
  /** The cockpit's poll: what has gone since, and whether the message has gone. */
  poll(): Promise<void>
}

/** The engine's answer, or null for anything that is not one (nothing going out). */
function parse(v: unknown): Js8Typeahead | null {
  if (!v || typeof v !== 'object') return null
  const o = v as Record<string, unknown>
  return typeof o.sent === 'string' && typeof o.unsent === 'string' && typeof o.locked === 'boolean'
    ? { sent: o.sent, unsent: o.unsent, locked: o.locked }
    : null
}

/** The end of what has gone, for the line beside the box: its last `max` characters. */
export function sentTail(sent: string, max = 24): string {
  return sent.length > max ? `…${sent.slice(sent.length - max)}` : sent
}

/** `text` is the compose box as the cockpit holds it, `setText` its setter. `begin`, `type` and
 * `poll` are the same functions on every render, so an effect may keep them. */
export function useJs8Typeahead(text: string, setText: (text: string) => void): Js8TypeaheadBox {
  const [view, setView] = useState<Js8Typeahead | null>(null)
  const viewRef = useRef<Js8Typeahead | null>(null)
  const textRef = useRef(text)
  textRef.current = text
  const seenRef = useRef('')
  const inflightRef = useRef(false)
  const pendingRef = useRef<string | null>(null)
  const setTextRef = useRef(setText)
  setTextRef.current = setText

  const box = useMemo(() => {
    const show = (v: Js8Typeahead | null) => {
      viewRef.current = v
      setView(v)
    }
    const write = (text: string) => {
      textRef.current = text
      setTextRef.current(text)
    }
    // What the engine says now: the message gone (the box empties, as JS8Call's does), or more of it
    // sent (taken off the front of the box, which goes on holding whatever was typed after it).
    const reconcile = (v: Js8Typeahead | null) => {
      if (!viewRef.current) return
      if (!v) {
        seenRef.current = ''
        pendingRef.current = null
        show(null)
        write('')
        return
      }
      const seen = seenRef.current
      if (v.sent.length > seen.length) {
        const gone = v.sent.startsWith(seen) ? v.sent.slice(seen.length) : null
        const now = textRef.current
        write(gone !== null && now.toUpperCase().startsWith(gone) ? now.slice(gone.length) : v.unsent)
        seenRef.current = v.sent
      }
      show(v)
    }
    // One change on its way at a time, the newest kept: the engine takes the box as it last was.
    const drain = async () => {
      inflightRef.current = true
      try {
        while (pendingRef.current !== null && viewRef.current) {
          const text = pendingRef.current
          pendingRef.current = null
          // A refusal leaves the box as it is. On a computer driving a station it is shown: the
          // typed words did not go out, and the link or the station said why.
          const v = await js8Typeahead(seenRef.current.length, text).then(parse, (e) => {
            showWhenDriving(e)
            return undefined
          })
          if (v !== undefined) reconcile(v)
        }
      } finally {
        inflightRef.current = false
      }
    }
    return {
      begin: async () => {
        const v = await js8TypeaheadState().then(parse, () => null)
        if (!v) return false
        seenRef.current = v.sent
        show(v)
        write(v.unsent)
        return true
      },
      type: (text: string) => {
        write(text)
        if (!viewRef.current || viewRef.current.locked) return
        pendingRef.current = text
        if (!inflightRef.current) void drain()
      },
      poll: async () => {
        if (!viewRef.current) return
        const v = await js8TypeaheadState().then(parse, () => undefined)
        if (v !== undefined) reconcile(v)
      },
    }
  }, [])

  return {
    active: view !== null,
    sent: view?.sent ?? '',
    locked: view?.locked ?? false,
    ...box,
  }
}
