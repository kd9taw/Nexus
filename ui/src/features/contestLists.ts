// The contest strip's two aids as data: the Super Check Partial list and the operator's
// call-history file. Each arrives through ONE command when a strip mounts with that aid on — never
// per keystroke, so typing a call costs no IPC — and never on Remote: the hosted page gets
// neither, and the strip passes `on = false` there.
//
// "On" is the engine's answer, not the strip's. `FieldDayStatus.assistanceOn` carries the labels
// of the assistance sources EFFECTIVELY on (`Settings::assistance_sources`, which folds Unassisted
// mode in), so the strip reads two labels off it and never re-derives assistance from switches.

import { useEffect, useState } from 'react'
import { getCallHistory, getScpCalls, scpEnsure } from '../api'
import type { CallHistoryFile } from '../types'

/** ⚠️ MIRRORS of `tempo_app::settings::SCP_SOURCE` and `CALL_HISTORY_SOURCE`, read from Rust by
 *  `the_strip_reads_the_scp_and_call_history_labels_the_engine_sends`. Change neither alone. */
export const SCP_SOURCE = 'Super Check Partial'
export const CALL_HISTORY_SOURCE = 'Call history'

/** Settings fires this after an update, an import or a clear, so a mounted strip reads again. */
export const CONTEST_LISTS_CHANGED = 'nexus:contest-lists-changed'

/** How often a mounted strip gives the station the chance to run its daily check. The station
 *  decides whether a check is due; a contest can run for days without the strip remounting. */
const RECHECK_MS = 60 * 60 * 1000

const NO_CALLS: string[] = []

/** The Super Check Partial list, while `on`. The first mount with SCP on is what downloads the
 *  list the first time a contest starts; a failed download leaves the strip working without it. */
export function useScpList(on: boolean): string[] {
  const [calls, setCalls] = useState<string[]>(NO_CALLS)
  useEffect(() => {
    if (!on) return
    let live = true
    // The download time of the list in hand, so the hourly check re-reads only a newer one.
    let readAt = -1
    const check = () =>
      scpEnsure(false)
        .then((st) => {
          if (!live || st.count === 0 || st.fetchedAt === readAt) return
          return getScpCalls().then((list) => {
            if (!live) return
            readAt = st.fetchedAt
            setCalls(list)
          })
        })
        // The strip works without a list, and Settings says why there is none.
        .catch(() => {})
    void check()
    const timer = window.setInterval(check, RECHECK_MS)
    const changed = () => {
      readAt = -1
      void check()
    }
    window.addEventListener(CONTEST_LISTS_CHANGED, changed)
    return () => {
      live = false
      window.clearInterval(timer)
      window.removeEventListener(CONTEST_LISTS_CHANGED, changed)
    }
  }, [on])
  return on ? calls : NO_CALLS
}

/** The imported call-history file, while `on`; `null` when none is imported. */
export function useCallHistory(on: boolean): CallHistoryFile | null {
  const [file, setFile] = useState<CallHistoryFile | null>(null)
  useEffect(() => {
    if (!on) return
    let live = true
    const read = () =>
      getCallHistory()
        .then((f) => {
          if (live) setFile(f)
        })
        .catch(() => {})
    void read()
    window.addEventListener(CONTEST_LISTS_CHANGED, read)
    return () => {
      live = false
      window.removeEventListener(CONTEST_LISTS_CHANGED, read)
    }
  }, [on])
  return on ? file : null
}
