// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Its one string comes from
// the catalog; the modes below are wire values (ADIF mode names), never translated.
//
// THE CONTEST LOGGER WINDOW (`?panel=contestlog`) — the whole contest screen with a log line
// under it, for a second person logging at a second monitor and keyboard while the operator works
// the radio in the main window (two keyboards and two mice on one computer, through MouseMux).
//
// ⭐ ONE CONTACT, TWO WINDOWS. The log line here and the operator's own contest strip share the
// contact in progress through the engine (`features/contestEntryShare`): what is typed in either
// shows in both as it is typed, every hint follows it in both, and Enter in either logs it once.
//
// ⛔ NOTHING IN THIS WINDOW TRANSMITS. No PTT, no F-key messages, no Tune, no Enter Sends Message:
// the log line here gets no `esm`; the screen's contest switch, Running / S&P and scoring are
// read-only (`ContestView`'s `logger`); the F-keys do nothing (the webview's own as well, so F5
// does not reload the logger mid-contact); and Esc clears the entry (`LogEntry`'s `sharedEntry`).
// Every TX and stop control stays on the operator's screen, whose hotkeys are bound in the main
// window's document only. This is a separate window with a document of its own, so a keystroke
// here never reaches them, and one there never reaches this.
//
// It logs as THIS computer's own position — the same engine, contest log and club sync, so the
// club wire carries nothing new — and under the station's operating mode, read live from the
// engine, so a contact logged here is credited as the operator's own would be. A Remote viewer
// never sees this window: the stream is the main window's picture.

import { useEffect, useState } from 'react'
import { getPskState, type FdRulesetDto } from '../api'
import { t } from '../i18n'
import { PSK_MODE_BY_SLUG } from '../pskModes'
import type { AppSnapshot } from '../types'
import { ContestView } from './ContestView'
import { LogEntry } from './LogEntry'

/** What this window logs under: the station's operating section (`snap.radio.operatingMode`, the
 *  engine's, which follows the operator from cockpit to cockpit) as the cockpit there logs it —
 *  Phone as PH, CW as CW, RTTY and the keyboard modes as DIG with the mode on the air, and the FT
 *  modes as DIG, whose mode the engine stamps from the tier it runs. A station too old to say
 *  logs as Phone, the voice logging this window exists for. */
export function loggerMode(
  section: string | undefined,
  pskName: string | null,
): { fdMode: 'CW' | 'PH' | 'DIG'; fdSubmode?: string; mode: string; defaultRst: string } {
  switch (section) {
    case 'cw':
      return { fdMode: 'CW', mode: 'CW', defaultRst: '599' }
    case 'rtty':
      return { fdMode: 'DIG', fdSubmode: 'RTTY', mode: 'RTTY', defaultRst: '599' }
    case 'keyboard': {
      // The PSK cockpit's own fallback when its state has not arrived: PSK31.
      const name = pskName ?? PSK_MODE_BY_SLUG.psk31.name
      return { fdMode: 'DIG', fdSubmode: name, mode: name, defaultRst: '599' }
    }
    case 'digital':
      return { fdMode: 'DIG', mode: 'FT8', defaultRst: '599' }
    default:
      return { fdMode: 'PH', mode: 'SSB', defaultRst: '59' }
  }
}

export function ContestLogger({
  snap,
  fdActive,
  fdRuleset,
}: {
  snap: AppSnapshot
  fdActive: boolean
  fdRuleset: FdRulesetDto | null
}) {
  const section = snap.radio.operatingMode
  // The keyboard modes' sub-mode is the PSK cockpit's state, not the snapshot's: read it the way
  // that cockpit does, when the station is on that section.
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
  // THE F-KEYS DO NOTHING HERE. In the main window they send messages; in this one they are not
  // bound to anything, and the webview's own F-key actions (F5 reloads the page) are held too.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (/^F\d{1,2}$/.test(e.key)) e.preventDefault()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])
  const fd = snap.fieldDay ?? null
  const m = loggerMode(section, pskName)
  return (
    <>
      <main className="layout single">
        <ContestView fieldDay={fd} fdActive={fdActive} fdRuleset={fdRuleset} tier={snap.link.tier} logger />
      </main>
      {fd && (
        <section className="contest-logger-entry" aria-label={t('contestLogger.entry.aria')}>
          <LogEntry
            snap={snap}
            mode={m.mode}
            defaultRst={m.defaultRst}
            exchange="terrestrial"
            titled={false}
            fieldDay={fd}
            fdMode={m.fdMode}
            fdSubmode={m.fdSubmode}
            sharedEntry="logger"
          />
        </section>
      )}
    </>
  )
}
