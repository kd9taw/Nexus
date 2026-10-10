// THE CONTEST STRIP'S CONTACT IN PROGRESS, SHARED WITH THE CONTEST LOGGER WINDOW — the strip's
// half of `tempo_app::engine::contest_entry`.
//
// While the contest logger window is open the snapshot carries `contestEntry`, and each contest
// strip on screen (the main window's and the logger's) shows that one entry: a change typed in
// either is put to the engine and taken up by the other at its next snapshot, and Enter in either
// names the entry it saw, so the engine logs the contact once (`claim`). With the window closed
// the snapshot carries no entry and every strip is its own, exactly as before: nothing here runs.
//
// Shared: the Call box, the received boxes, and two marks: which boxes hold a call-history fill,
// and the take-back line (Ctrl+D twice). Not shared: Enter Sends Message's line (the main
// window's alone), the "I moved" editor, and a box's open type-ahead list.
//
// ⛔ NOTHING HERE TRANSMITS. It moves text between two windows through the engine.

import { useEffect, useRef } from 'react'
import { contestEntryPut, type EntryClaim } from '../api'
import type { ContestEntryShared } from '../types'
import { EMPTY_FILL, type FillState } from './callHistoryFill'

/** The engine's refusals of an Enter (`contest_entry.rs`): the contact was logged from the
 *  other window, or changed there since this window showed it. Either way nothing was logged. */
export const ENTRY_LOGGED = 'contestEntryLogged'
export const ENTRY_CHANGED = 'contestEntryChanged'

/** The take-back line (Ctrl+D twice), as both windows show it: a press waiting for its second,
 *  or the answer to the last one. */
export interface TakeLine {
  armed: { call: string; whenUnix: number; label: string; at: number } | null
  note: { text: string; alert: boolean } | null
}

export const NO_TAKE: TakeLine = { armed: null, note: null }

/** What a strip shares. */
export interface StripContent {
  call: string
  fields: Record<string, string>
  fill: FillState
  take: TakeLine
}

/** One string per content, for "is this what I last agreed with": keys sorted, so two windows
 *  that wrote one box in a different order still agree. */
export function contentKey(c: StripContent): string {
  const sorted = (o: Record<string, unknown>) =>
    Object.fromEntries(Object.keys(o).sort().map((k) => [k, o[k]]))
  return JSON.stringify([
    c.call,
    sorted(c.fields),
    sorted(c.fill.typed),
    sorted(c.fill.filled),
    c.take.armed,
    c.take.note,
  ])
}

const isRecord = (v: unknown): v is Record<string, unknown> =>
  typeof v === 'object' && v !== null && !Array.isArray(v)

/** The marks another window put, read defensively: a shape this build does not know reads as
 *  no marks, never as a crash. */
function readMarks(raw: unknown): { fill: FillState; take: TakeLine } {
  const m = isRecord(raw) ? raw : {}
  const f = isRecord(m.fill) ? m.fill : {}
  const fill: FillState =
    isRecord(f.typed) && isRecord(f.filled)
      ? (f as unknown as FillState)
      : EMPTY_FILL
  const t = isRecord(m.take) ? m.take : {}
  const take: TakeLine = {
    armed: isRecord(t.armed) ? (t.armed as unknown as TakeLine['armed']) : null,
    note: isRecord(t.note) ? (t.note as unknown as TakeLine['note']) : null,
  }
  return { fill, take }
}

/** What a strip shows once it takes up the shared entry: its call, its marks, and its boxes over
 *  the strip's own. A box the entry does not hold keeps what the strip shows (a class guess, the
 *  last contact's carried-over exchange), so taking up a blank entry changes no box. */
export function takeUp(local: StripContent, e: ContestEntryShared): StripContent {
  const { fill, take } = readMarks(e.marks)
  return { call: e.call, fields: { ...local.fields, ...e.fields }, fill, take }
}

/** Nothing typed: no call and no box. */
function blankEntry(e: ContestEntryShared): boolean {
  return e.call.trim() === '' && Object.keys(e.fields).length === 0
}

function blankStrip(c: StripContent): boolean {
  return c.call.trim() === '' && Object.values(c.fields).every((v) => v.trim() === '')
}

/**
 * Keep one strip and the shared entry the same, while `on` and the logger window is open
 * (`entry` present):
 *  - when this strip starts sharing, it takes the entry up — or, if the entry is still blank and
 *    `seeds` (the main window's strip), puts what it already holds, so a logger window opened in
 *    the middle of a contact shows that contact;
 *  - a change made here (any change that is not taking the entry up) is put;
 *  - an entry newer than the last one this strip agreed with is taken up (`adopt`), unless a put
 *    of this strip's own is still on its way, whose answer moves it on.
 *
 * `claim` is Enter's half: what is on screen goes first, then the answer is the entry Enter saw,
 * for the log command to name. `undefined` while not sharing: log as always. `sharing` says
 * whether this strip takes part now, so a strip that does not can skip the claim altogether.
 */
export function useSharedEntry(opts: {
  entry: ContestEntryShared | undefined
  on: boolean
  seeds: boolean
  content: StripContent
  adopt: (c: StripContent) => void
}): { sharing: boolean; claim: (c: StripContent) => Promise<EntryClaim | undefined> } {
  const { entry, content } = opts
  const live = opts.on && entry !== undefined
  const liveRef = useRef(live)
  liveRef.current = live
  const contentRef = useRef(content)
  contentRef.current = content
  const adoptRef = useRef(opts.adopt)
  adoptRef.current = opts.adopt
  /** The entry this strip last agreed with: its rev, and the content as this strip shows it. */
  const synced = useRef<{ rev: number; key: string } | null>(null)
  const chain = useRef<Promise<unknown>>(Promise.resolve())
  const pending = useRef(0)
  /** Which render this is, and the render whose effects took the entry up: in that same pass
   *  the strip still shows what it held before, which is not a change of its own and must not be
   *  put straight back over what it just took up. */
  const renders = useRef(0)
  renders.current += 1
  const thisRender = renders.current
  const tookUpAt = useRef(0)

  const put = (c: StripContent) => {
    synced.current = { rev: synced.current?.rev ?? 0, key: contentKey(c) }
    const { fill, take } = c
    pending.current += 1
    chain.current = chain.current
      .then(() => contestEntryPut(c.call, c.fields, { fill, take }))
      .then((rev) => {
        if (synced.current) synced.current = { ...synced.current, rev: Math.max(synced.current.rev, rev) }
      })
      .catch(() => {})
      .finally(() => {
        pending.current -= 1
      })
  }

  const rev = entry?.rev
  useEffect(() => {
    if (!live || entry === undefined) {
      synced.current = null
      return
    }
    if (synced.current === null && opts.seeds && blankEntry(entry) && !blankStrip(contentRef.current)) {
      put(contentRef.current)
      return
    }
    if (synced.current !== null && (pending.current > 0 || entry.rev <= synced.current.rev)) return
    const next = takeUp(contentRef.current, entry)
    synced.current = { rev: entry.rev, key: contentKey(next) }
    tookUpAt.current = thisRender
    adoptRef.current(next)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [live, rev])

  const key = contentKey(content)
  useEffect(() => {
    if (tookUpAt.current === thisRender) return
    if (!live || synced.current === null || key === synced.current.key) return
    put(contentRef.current)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [live, key])

  const claim = async (c: StripContent): Promise<EntryClaim | undefined> => {
    if (!liveRef.current || synced.current === null) return undefined
    if (contentKey(c) !== synced.current.key) put(c)
    await chain.current
    if (!liveRef.current || synced.current === null) return undefined
    return { rev: synced.current.rev, call: c.call, fields: c.fields }
  }
  return { sharing: live, claim }
}
