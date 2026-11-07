// Carrying out a memory's recall plan: the one piece of it App and the Memories pop-out share.
//
// RECALL ADDS NO WAY OF TUNING. Each screen tunes with the verb its own surfaces already use: the
// APRS Tune button's `aprs_tune`, `work_spot` for most, and for SSTV `sstv_tune_recall`, the strip's
// tune without its arming: a recall to SSTV leaves Enable TX as it was. All of them land in the
// engine's `tune_dial`, so a recall during a transmission is treated as any other QSY is, and none
// of them keys anything.
//
// ⚠️ SSTV MUST NOT USE `work_spot`. Its nav hint names a SECTION, and every window follows it: on a
// Phone-section tune it would pull the operator straight off SSTV onto Phone. `sstv_tune_recall`
// claims Phone and ticks no hint. That leaves a pop-out with no way to bring the main window along for
// SSTV or APRS, so it asks, as a contest window asks (`features/contestWorkIn.ts`).
//
// The ask's key is SHARED (storage-scope.test.ts): it is the whole mechanism between two documents.
import { useEffect, useRef } from 'react'
import { aprsTune, sstvTuneRecall, workSpot } from '../api'
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
  return workSpot(WORK_SECTION[plan.view], plan.freqMhz, band, undefined, plan.view === 'js8' ? 'JS8' : undefined)
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
