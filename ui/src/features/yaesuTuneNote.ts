// #441: THE TUNE NOTE IN PHONE ON A YAESU WITH BUILT-IN USB AUDIO. In Phone, Nexus keys the radio in plain
// USB or LSB. The FT-991/991A, FT-710, FTDX10 and FTDX101D/MP then take their transmit audio from their SSB
// MOD SOURCE menu, MIC from the factory: the front jack only. Tune's tone goes to the radio over its USB
// audio, so with that menu on MIC the radio keys and puts out no RF. Nexus never changes a radio's menus,
// so a note says where the menu is. It is text only: Tune keys exactly as it does on every other radio.
// (Settings' computer-microphone hint says the same for the PC microphone.)
//
// IT SHOWS WHEN IT IS NEEDED, NOT ALWAYS. A permanent line cost Phone's TX strip 17 px at 1080p for every
// operator whose menu is already right. So the note comes up the first time a Tune in Phone reads about
// no power (under 2 W for 2 s while keyed) and then stays for the session, beside Tune, until dismissed;
// dismissed, it is gone for this computer, as the Flex DAX note is. Where the radio reports no power at
// all during a Tune (no reading in 2 s), Nexus cannot tell, so an ⓘ beside Tune holds the same words.
import { useEffect, useRef, useSyncExternalStore } from 'react'
import type { RadioStatus, Settings } from '../types'

/** The Yaesus whose own USB audio carries Tune's tone (Hamlib models, as rigmodels.rs names them):
 *  FT-991/991A (1035), FTDX101D (1040), FTDX10 (1042), FTDX101MP (1044), FT-710 (1049). The other
 *  newcat Yaesus (FT-450, FT-891, FT-950, FT-2000, FTDX1200/3000/5000/9000) have USB for CAT only and
 *  take computer audio through an outboard interface, where this advice would be wrong. */
export const USB_AUDIO_YAESUS: readonly number[] = [1035, 1040, 1042, 1044, 1049]

/** Whether Phone's Tune note can apply to the active radio: one of those Yaesus on its own CAT link. A
 *  TCI program stores a model of its own (`TCI_RIG_MODEL`), so a model number there is not the radio's. */
export function phoneTuneNeedsRearSource(s: Pick<Settings, 'rigModel' | 'rigConn'>): boolean {
  return USB_AUDIO_YAESUS.includes(s.rigModel) && s.rigConn !== 'tci'
}

/** "About no power": the radio's measured output under this many watts. */
export const NO_POWER_W = 2
/** For this long while Tune is keyed, before the note comes up (a reading starts low as the carrier
 *  builds, and the poll reads one meter at a time). */
export const NO_POWER_MS = 2000

export const TUNE_NOTE_KEY = 'nexus.phone.yaesuTuneNote'

/** What Phone shows: the note's line, the ⓘ that holds it, or nothing. */
export type TuneNote = 'line' | 'icon' | null

// This window's verdict for the session: once a Tune read no power, the line stays until dismissed.
let verdict: TuneNote = null
let dismissed: boolean | null = null
const listeners = new Set<() => void>()
const emit = () => listeners.forEach((l) => l())
const subscribe = (l: () => void) => {
  listeners.add(l)
  return () => {
    listeners.delete(l)
  }
}

function isDismissed(): boolean {
  if (dismissed === null) {
    try {
      dismissed = window.localStorage.getItem(TUNE_NOTE_KEY) === '1'
    } catch {
      dismissed = false
    }
  }
  return dismissed
}

/** The operator's Dismiss: gone for this computer, whichever window showed it. */
export function dismissTuneNote(): void {
  dismissed = true
  try {
    window.localStorage.setItem(TUNE_NOTE_KEY, '1')
  } catch {
    // A blocked store: dismissed for this window until it closes.
  }
  emit()
}

/** Forget the session's verdict and the remembered dismissal (a test's fresh window). */
export function resetTuneNote(): void {
  verdict = null
  dismissed = null
  emit()
}

function settle(next: Exclude<TuneNote, null>) {
  if (verdict === 'line' || verdict === next) return
  verdict = next
  emit()
}

/**
 * Phone's Tune note for the active radio. `applies`: one of the USB-audio Yaesus
 * (`phoneTuneNeedsRearSource`). While Tune is keyed it watches the measured power: under `NO_POWER_W` for
 * `NO_POWER_MS` settles on the line; no reading at all in that time settles on the ⓘ (a later Tune that
 * does read no power still brings the line). Any reading at or over `NO_POWER_W` restarts the count.
 */
export function useTuneNote(applies: boolean, radio: Pick<RadioStatus, 'tuning' | 'txPoW'>): TuneNote {
  const shown = useSyncExternalStore(subscribe, () => (isDismissed() ? null : verdict), () => null)
  const lowSince = useRef<number | null>(null)
  const read = useRef(false)
  const start = useRef(0)
  const tuning = applies && radio.tuning
  const watts = radio.txPoW
  // A Tune starts: nothing read yet in it.
  useEffect(() => {
    if (!tuning) return
    start.current = Date.now()
    read.current = false
    lowSince.current = null
  }, [tuning])
  // Each reading while keyed: low starts (or keeps) the count, a real one ends it.
  useEffect(() => {
    if (!tuning || watts == null) return
    read.current = true
    if (watts < NO_POWER_W) lowSince.current ??= Date.now()
    else lowSince.current = null
  }, [tuning, watts])
  useEffect(() => {
    if (!tuning) return
    const timer = window.setInterval(() => {
      const now = Date.now()
      if (lowSince.current != null && now - lowSince.current >= NO_POWER_MS) settle('line')
      else if (!read.current && now - start.current >= NO_POWER_MS) settle('icon')
    }, 250)
    return () => window.clearInterval(timer)
  }, [tuning])
  return applies ? shown : null
}
