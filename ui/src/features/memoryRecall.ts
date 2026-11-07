// Carrying out a memory's recall plan: the one piece of it App and the Memories pop-out share.
//
// RECALL ADDS NO WAY OF TUNING. Each screen tunes with the verb its own surfaces already use: the
// APRS Tune button's `aprs_tune` and `work_spot` for most. All of them land in the engine's
// `tune_dial`, so a recall during a transmission is treated as any other QSY is, and none of them
// keys anything.
//
// ⛔ AND IT ARMS NOTHING IT DID NOT BEFORE. A memory on SSTV, RTTY, PSK or JTTY tunes with that verb's
// no-arming form (`sstv_tune_recall`, `work_spot_recall`), so Enable TX stays as it was. Phone and CW
// keep the arming entry they always had, and FT and JS8's section arms nothing. See
// [`recallLeavesTxAlone`] for the second way a recall could reach the latch.
//
// ⚠️ SSTV MUST NOT USE `work_spot`. Its nav hint names a SECTION, and every window follows it: on a
// Phone-section tune it would pull the operator straight off SSTV onto Phone. `sstv_tune_recall`
// claims Phone and ticks no hint. That leaves a pop-out with no way to bring the main window along for
// SSTV or APRS, so it asks, as a contest window asks (`features/contestWorkIn.ts`).
//
// The ask's key is SHARED (storage-scope.test.ts): it is the whole mechanism between two documents.
import { useEffect, useRef } from 'react'
import { aprsTune, sstvTuneRecall, workSpot, workSpotRecall } from '../api'
import type { AppSnapshot } from '../types'
import type { MemoryScreen, RecallPlan } from './memories'
import { WORK_SECTION } from './needs'

export function tuneForRecall(plan: RecallPlan, band: string): Promise<AppSnapshot> {
  if (plan.view === 'sstv') {
    // The engine arms its FM hold on the word FM, and an NFM memory is that same machine.
    const word = plan.mode.toUpperCase() === 'NFM' ? 'FM' : plan.mode
    return sstvTuneRecall(plan.freqMhz, band, word)
  }
  if (plan.view === 'aprs') return aprsTune(plan.freqMhz)
  if (recallLeavesTxAlone(plan.view)) return workSpotRecall(WORK_SECTION[plan.view], plan.freqMhz, band)
  return workSpot(WORK_SECTION[plan.view], plan.freqMhz, band, undefined, plan.view === 'js8' ? 'JS8' : undefined)
}

/** The manual sections a recall enters without arming. Their memories opened FT, which arms nothing,
 * until a memory could name its screen. The tune is one way to the latch; the other is the view
 * change, whose rig-mode effect re-asserts the section at the station's own authority, so a window
 * opening one of these for a recall must skip that re-assert. A tab opening it does not. */
export function recallLeavesTxAlone(view: MemoryScreen): view is 'rtty' | 'psk' | 'jtty' {
  return view === 'rtty' || view === 'psk' || view === 'jtty'
}

/** SSTV rides Phone, so a sideband override means the same on both. */
export function ridesPhone(view: MemoryScreen): boolean {
  return view === 'phone' || view === 'sstv'
}

/** The phone policy commands only SSB and FM, so for AM/WFM/DV/DN a recall tunes the dial and must
 * tell the operator to set the mode on the rig rather than claim a mode it did not command. (CW and
 * digital recalls set theirs through the section's policy, and APRS's tune commands FM.) */
export function modeNotCommanded(plan: RecallPlan): boolean {
  const mode = plan.mode.toUpperCase()
  if (!ridesPhone(plan.view) || mode === 'USB' || mode === 'LSB' || mode === 'FM') return false
  return !(plan.view === 'sstv' && mode === 'NFM')
}

export const MEMORY_OPEN_SCREEN_KEY = 'nexus.memory.openScreen'

/** An ask as stored, read back: SSTV or APRS, the screens no nav hint reaches, else null. */
export function openScreenRequest(stored: string | null): 'sstv' | 'aprs' | null {
  if (!stored) return null
  try {
    const screen = (JSON.parse(stored) as { screen?: unknown } | null)?.screen
    return screen === 'sstv' || screen === 'aprs' ? screen : null
  } catch {
    return null
  }
}

/** From the pop-out: ask the main window to open `screen`. The time makes a second recall of the
 * same screen a new value, which is what raises a second `storage` event. */
export function askMainWindowToOpen(screen: 'sstv' | 'aprs'): void {
  try {
    window.localStorage.setItem(MEMORY_OPEN_SCREEN_KEY, JSON.stringify({ screen, at: Date.now() }))
  } catch {
    /* unavailable: the tune stands, and the main window stays where it is */
  }
}

/** In the main window: hear those asks. A `storage` event is only raised by another document. */
export function useMemoryOpenScreenRequests(onOpen: (screen: 'sstv' | 'aprs') => void): void {
  const latest = useRef(onOpen)
  latest.current = onOpen
  useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (e.key !== MEMORY_OPEN_SCREEN_KEY) return
      const screen = openScreenRequest(e.newValue)
      if (screen) latest.current(screen)
    }
    window.addEventListener('storage', onStorage)
    return () => window.removeEventListener('storage', onStorage)
  }, [])
}
