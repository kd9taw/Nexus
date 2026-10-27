// THE JS8 SOUNDS, FROM WHICHEVER SCREEN IS SHOWING. JS8Call plays its notification sounds from its
// decode and command paths, never from a view (`tryNotify`: processDecodeEvent.cpp:407, :429;
// processCommandActivity.cpp:277, :587, :806, :823, :849; mainwindow.cpp:2313-2318), so they sound
// whatever its window shows. The JS8 cockpit reads the engine only while it is on screen, so the
// sounds are watched here instead, mounted by App beside the cockpit's host and not inside it: while
// JS8 is the mode and at least one sound is on, one read a second of the state the cockpit polls
// (`get_js8_state`), and nothing at all otherwise (every sound ships off, as in JS8Call).
//
// Each read is held against the one before (features/js8Sounds `Js8SoundWatch`), so a frame, a filed
// message or a newly heard call sounds once; when the watch starts (JS8 entered, a sound switched
// on) its first read only learns what is there. A new call waits for the log's word on whether it is
// worked, asked only while one of those two sounds is on.
//
// Renders nothing, and transmits nothing.
import { useEffect, useMemo, useRef, useState } from 'react'
import { getJs8State } from '../api'
import { pollSingleFlight } from '../singleFlight'
import { useLatestLogAnswer } from '../features/logSource'
import { Js8SoundWatch, js8SoundsToPlay, playJs8Sound, type Js8SoundEvent } from '../features/js8Sounds'
import type { Js8Heard, Tier } from '../types'

export function Js8SoundPlayer({
  tier,
  sounds,
  files,
  myCall,
  groups,
  logTick,
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
}): null {
  const on = tier === 'JS8' && sounds.length > 0
  // What a read is judged by, as it is when the read lands.
  const now = useRef({ sounds, files, myCall, groups })
  now.current = { sounds, files, myCall, groups }
  const watch = useRef(new Js8SoundWatch())
  const stations = useRef<readonly Js8Heard[]>([])
  /** The heard calls at the latest read, for the log's question; null until a read has landed. */
  const [calls, setCalls] = useState<string | null>(null)
  const play = (events: readonly Js8SoundEvent[]) => {
    for (const ev of js8SoundsToPlay(events, now.current.sounds)) void playJs8Sound(ev, now.current.files[ev])
  }

  useEffect(() => {
    if (!on) return
    watch.current = new Js8SoundWatch()
    stations.current = []
    setCalls(null)
    return pollSingleFlight('js8 sounds', 1000, (owns) =>
      getJs8State()
        .then((s) => {
          if (!owns()) return
          const { myCall, groups } = now.current
          play([...watch.current.frames(s.activity, myCall, groups), ...watch.current.inbox(s.inbox)])
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
