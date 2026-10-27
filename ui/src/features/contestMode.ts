// WHAT A CONTEST LOG LINE LOGS UNDER, AND THE MODE THE STATION IS ON — both read off the station's
// operating section (`snap.radio.operatingMode`, the engine's, which follows the operator from
// cockpit to cockpit). The large log line (`ContestLargeEntry`, in the contest logger window and at
// the top of the Contest screen) logs under the first; it and the Contest screen's banned-mode chip
// name the second, so the line and the chip cannot disagree about whether the contest permits it.
import { useEffect, useState } from 'react'
import { getPskState } from '../api'
import { PSK_MODE_BY_SLUG } from '../pskModes'

/** What a contest log line off the cockpits logs under: the station's operating section as the
 *  cockpit there logs it — Phone as PH, CW as CW, RTTY and the keyboard modes as DIG with the mode on
 *  the air (`keyboardMode`, from `useKeyboardMode`: PSK31, QPSK31 or JTTY), and the digital section as DIG with the tier it runs (`tier`, the snapshot's, which is
 *  the engine's own log-mode name, as the Satellites strip reads it). A station too old to say logs
 *  as Phone, the voice logging the large log line exists for.
 *
 *  ⚠️ THE DIGITAL SECTION NAMES ITS MODE. A DIG contact logged with none is stored with none and
 *  exports as FT8 (`LoggedQso::recorded_mode`): from the JS8 cockpit's section, a JS8 contact went
 *  out as FT8, a mode the Illinois and New York QSO parties and Winter Field Day give no credit.
 *
 *  The sections are the engine's five (`settings::OperatingMode`), and the main window's other
 *  sections are not among them: JS8 is the digital section on the JS8 tier, picking an SSTV channel
 *  claims Phone, and APRS and Satellites claim none, so the station stays on the one it was on. */
export function contestLogMode(
  section: string | undefined,
  keyboardMode: string | null,
  tier?: string,
): { fdMode: 'CW' | 'PH' | 'DIG'; fdSubmode?: string; mode: string; defaultRst: string } {
  switch (section) {
    case 'cw':
      return { fdMode: 'CW', mode: 'CW', defaultRst: '599' }
    case 'rtty':
      return { fdMode: 'DIG', fdSubmode: 'RTTY', mode: 'RTTY', defaultRst: '599' }
    case 'keyboard': {
      // ⚠️ PSK AND JTTY ARE ONE SECTION: the mode on the air is `keyboardMode`, never the section.
      // Read off the section alone, a JTTY contact was logged as PSK31. The PSK cockpit's own
      // fallback when nothing has arrived yet: PSK31.
      const name = keyboardMode || PSK_MODE_BY_SLUG.psk31.name
      return { fdMode: 'DIG', fdSubmode: name, mode: name, defaultRst: '599' }
    }
    case 'digital': {
      const name = tier?.trim()
      return name
        ? { fdMode: 'DIG', fdSubmode: name, mode: name, defaultRst: '599' }
        : { fdMode: 'DIG', mode: 'FT8', defaultRst: '599' }
    }
    default:
      return { fdMode: 'PH', mode: 'SSB', defaultRst: '59' }
  }
}

/** The mode the station is on now, named as a contest's `bannedModes` names modes: on the digital
 *  section the FT tier it runs, on every other the mode its contacts log under. */
export function stationMode(section: string | undefined, tier: string | undefined, keyboardMode: string | null): string {
  return section === 'digital' ? (tier ?? '') : contestLogMode(section, keyboardMode).mode
}

/** The Keyboard section's mode on the air: the snapshot's own (`radio.keyboardMode`: PSK31, QPSK31
 *  or JTTY), and from a station too old to report it, the PSK cockpit's state as before. */
export function useKeyboardMode(section: string | undefined, keyboardMode: string | undefined): string | null {
  const pskName = usePskName(keyboardMode ? undefined : section)
  return keyboardMode || pskName
}

/** The keyboard modes' sub-mode, which is the PSK cockpit's state rather than the snapshot's: read
 *  the way that cockpit reads it, while the station is on that section. */
export function usePskName(section: string | undefined): string | null {
  const [pskName, setPskName] = useState<string | null>(null)
  useEffect(() => {
    if (section !== 'keyboard') return
    let live = true
    getPskState()
      .then((s) => live && setPskName(PSK_MODE_BY_SLUG[s.mode ?? 'psk31']?.name ?? null))
      .catch(() => {})
    return () => {
      live = false
    }
  }, [section])
  return pskName
}
