// #468: THE LISTENING LEVEL, IN THE TOP BAR. The same setting as Settings ▸ Radio ▸ "Receive audio
// on this computer" ▸ Listening level (`monitorLevel`): how loud Nexus plays the receive audio on
// this computer. Receive only; it does not touch transmit audio or anything the radio is told.
//
// A drag is many changes and each save writes the settings file, so the slider shows the value
// under the pointer at once (`draft`) and hands the one it settles on to `onCommit`, once, 250 ms
// after the last change. The draft gives way when the saved level arrives back, or when the save
// fails, so the slider never sits on a value that was not kept.
import { useEffect, useRef, useState } from 'react'
import { Volume2 } from 'lucide-react'
import { t } from '../i18n'

const SETTLE_MS = 250

export function ListenLevel({
  level,
  onCommit,
}: {
  /** The saved listening level, 0..1. */
  level: number
  /** Save a level, 0..1. A rejection puts the slider back on the saved level. */
  onCommit: (level: number) => Promise<unknown> | void
}) {
  const [draft, setDraft] = useState<number | null>(null)
  if (draft != null && Math.abs(draft - level) < 1e-6) setDraft(null)
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null)
  useEffect(
    () => () => {
      if (timer.current != null) clearTimeout(timer.current)
    },
    [],
  )
  const shown = draft ?? level
  const change = (v: number) => {
    setDraft(v)
    if (timer.current != null) clearTimeout(timer.current)
    timer.current = setTimeout(() => {
      timer.current = null
      void Promise.resolve(onCommit(v)).catch(() => setDraft(null))
    }, SETTLE_MS)
  }
  return (
    <label className="listen-level" title={t('topbar.listen.title', { pct: Math.round(shown * 100) })}>
      <Volume2 size={14} aria-hidden="true" />
      <input
        type="range"
        min="0"
        max="1"
        step="0.01"
        value={String(shown)}
        aria-label={t('settings.headphoneMonitor.level.aria')}
        onChange={(e) => change(Number(e.target.value))}
      />
    </label>
  )
}
