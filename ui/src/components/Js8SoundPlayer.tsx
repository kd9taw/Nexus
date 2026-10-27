// THE JS8 SOUNDS AND THE NEW-MESSAGE NOTICE, FROM WHICHEVER SCREEN IS SHOWING. JS8Call plays its notification sounds from its
// decode and command paths, never from a view (`tryNotify`: processDecodeEvent.cpp:407, :429;
// processCommandActivity.cpp:277, :587, :806, :823, :849; mainwindow.cpp:2313-2318), so they sound
// whatever its window shows. The JS8 cockpit reads the engine only while it is on screen, so the
// sounds are watched here instead, mounted by App beside the cockpit's host and not inside it: while
// JS8 is the mode, one read a second of the state the cockpit polls (`get_js8_state`), and nothing at
// all otherwise. Every sound still ships off, as in JS8Call.
//
// JS8Call's "New Message Received" box (processCommandActivity.cpp:806-820) is raised here too, for
// the same reason: ONCE per message filed unread for me, staying up as long as that box (300 s) unless
// dismissed, with a way to the message (`onShowMessage`, which opens the JS8 screen on that station's
// Inbox). What the first read finds is taken as seen, so mail filed before JS8 was entered (or
// restored from the journal) does not announce itself.
//
// Each read is held against the one before (features/js8Sounds `Js8SoundWatch`), so a frame, a filed
// message or a newly heard call sounds once; when the watch starts (JS8 entered, a sound switched
// on) its first read only learns what is there. A new call waits for the log's word on whether it is
// worked, asked only while one of those two sounds is on.
//
// Renders nothing, and transmits nothing.
import { useEffect, useMemo, useRef, useState } from 'react'
import { getJs8State } from '../api'
import { pushToast } from '../toast'
import { t } from '../i18n'
import { utcClock } from '../js8Vocab'
import { pollSingleFlight } from '../singleFlight'
import { useLatestLogAnswer } from '../features/logSource'
import { Js8SoundWatch, js8SoundsToPlay, playJs8Sound, type Js8SoundEvent } from '../features/js8Sounds'
import type { Js8Heard, Js8State, Tier } from '../types'

/** How long the new-message notice stays up: JS8Call's "New Message Received" box closes itself
 *  after 300 seconds (`SelfDestructMessageBox(300, …)`, processCommandActivity.cpp:809-818). */
const JS8_NEW_MESSAGE_MS = 300_000

export function Js8SoundPlayer({
  tier,
  sounds,
  files,
  myCall,
  groups,
  logTick,
  onShowMessage,
}: {
  /** The engine's mode (the snapshot's link tier): JS8 is heard only in JS8. */
  tier: Tier | undefined
  /** Settings `js8Sounds`: the events that play a sound. */
  sounds: readonly string[]
  /** Settings `js8SoundFiles`: the WAV file an event plays, by event. */
  files: Readonly<Record<string, string>>
  myCall: string
  /** The joined @GROUPs: a message to one is directed to me. */
  groups: readonly string[]
  logTick: number | undefined
  /** The notice's "Show": the JS8 screen, on that station's Inbox. */
  onShowMessage?: (from: string) => void
}): null {
  const on = tier === 'JS8'
  // What a read is judged by, as it is when the read lands.
  const now = useRef({ sounds, files, myCall, groups })
  now.current = { sounds, files, myCall, groups }
  const watch = useRef(new Js8SoundWatch())
  const stations = useRef<readonly Js8Heard[]>([])
  /** The heard calls at the latest read, for the log's question; null until a read has landed. */
  const [calls, setCalls] = useState<string | null>(null)
  const showRef = useRef(onShowMessage)
  showRef.current = onShowMessage
  /** The Inbox ids already seen this watch; null until its first read. */
  const seenMail = useRef<Set<number> | null>(null)
  const play = (events: readonly Js8SoundEvent[]) => {
    for (const ev of js8SoundsToPlay(events, now.current.sounds)) void playJs8Sound(ev, now.current.files[ev])
  }

  const announce = (inbox: Js8State['inbox']) => {
    const seen = seenMail.current
    if (seen === null) {
      seenMail.current = new Set(inbox.map((e) => e.id))
      return
    }
    for (const e of inbox) {
      if (seen.has(e.id)) continue
      seen.add(e.id)
      if (e.state !== 'unread') continue
      pushToast(t('js8.inbox.new', { from: e.from, time: utcClock(e.atMs) }), 'info', JS8_NEW_MESSAGE_MS, {
        action: () => showRef.current?.(e.from),
        actionLabel: t('js8.inbox.new.open'),
      })
    }
  }

  useEffect(() => {
    if (!on) return
    watch.current = new Js8SoundWatch()
    seenMail.current = null
    stations.current = []
    setCalls(null)
    return pollSingleFlight('js8 sounds', 1000, (owns) =>
      getJs8State()
        .then((s) => {
          if (!owns()) return
          const { myCall, groups } = now.current
          play([...watch.current.frames(s.activity, myCall, groups), ...watch.current.inbox(s.inbox)])
          announce(s.inbox)
          stations.current = s.stations
          setCalls(s.stations.map((h) => h.call).sort().join(' '))
        })
        .catch(() => {}),
    )
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `play` reads the refs
  }, [on])

  const askLog = on && (sounds.includes('callNew') || sounds.includes('callOld'))
  const question = useMemo(() => (askLog && calls ? ({ kind: 'callsSummary', calls: calls.split(' ') } as const) : null), [askLog, calls])
  const summary = useLatestLogAnswer(question, logTick)
  useEffect(() => {
    if (!askLog || calls === null) return
    const answered = new Set(summary?.question.calls)
    play(watch.current.newCalls(stations.current, (call) => (answered.has(call) ? summary?.answer[call] !== undefined : null)))
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `play` reads the refs
  }, [askLog, calls, summary])
  return null
}
