// #468: THE LISTENING LEVEL, FROM THE TOP BAR. The same setting as Settings ▸ Radio ▸ "Receive audio
// on this computer" ▸ Listening level (`monitorLevel`): how loud Nexus plays the receive audio on
// this computer. Receive only; it does not touch transmit audio or anything the radio is told.
//
// AN ICON CHIP THAT OPENS A SLIDER, never a slider in the bar. The bar has almost no slack at the
// common sizes (1366×768, 1024×768): a 5.5em inline slider wrapped it onto a second row there, which
// moved the whole cockpit and Stop TX down by a row whenever receive audio was switched on. The chip
// is narrower than Help, its neighbour, and its box is cockpit-panes.css's (`.listen-chip`); the
// slider lives in a portaled popover (Radix DropdownMenu, as ui/Menu, with the app zoom re-applied
// inside it) that takes no room in the bar.
//
// A drag is many changes and each save writes the settings file, so the slider shows the value
// under the pointer at once (`draft`) and hands the one it settles on to `onCommit`, once, 250 ms
// after the last change. The draft gives way when the saved level arrives back, or when the save
// fails, so the slider never sits on a value that was not kept.
import { useEffect, useRef, useState } from 'react'
import * as RM from '@radix-ui/react-dropdown-menu'
import { Volume2 } from 'lucide-react'
import { t } from '../i18n'

const SETTLE_MS = 250
/** The unit after the level's number: an invariant token. */
const PCT = '%'

export function ListenLevel({
  level,
  onCommit,
}: {
  /** The saved listening level, 0..1. */
  level: number
  /** Save a level, 0..1. A rejection puts the slider back on the saved level. */
  onCommit: (level: number) => Promise<unknown> | void
}) {
  const [open, setOpen] = useState(false)
  const [draft, setDraft] = useState<number | null>(null)
  if (draft != null && Math.abs(draft - level) < 1e-6) setDraft(null)
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const input = useRef<HTMLInputElement | null>(null)
  useEffect(
    () => () => {
      if (timer.current != null) clearTimeout(timer.current)
    },
    [],
  )
  // The keyboard goes to the slider when the popover opens, so the arrows change the level at once.
  useEffect(() => {
    if (!open) return
    const id = requestAnimationFrame(() => input.current?.focus({ preventScroll: true }))
    return () => cancelAnimationFrame(id)
  }, [open])
  const shown = draft ?? level
  const pct = Math.round(shown * 100)
  const change = (v: number) => {
    setDraft(v)
    if (timer.current != null) clearTimeout(timer.current)
    timer.current = setTimeout(() => {
      timer.current = null
      void Promise.resolve(onCommit(v)).catch(() => setDraft(null))
    }, SETTLE_MS)
  }
  const label = t('settings.headphoneMonitor.level.aria')
  return (
    <RM.Root open={open} onOpenChange={setOpen}>
      <RM.Trigger asChild>
        <button type="button" className="theme-chip listen-chip" title={t('topbar.listen.title', { pct })} aria-label={label}>
          <Volume2 size={14} aria-hidden="true" />
        </button>
      </RM.Trigger>
      <RM.Portal>
        <RM.Content className="ui-menu listen-pop" role="dialog" aria-label={label} sideOffset={4} align="end" collisionPadding={8}>
          {/* The portal escapes `.app`'s zoom, so the content puts it back (ui/Menu, portal-zoom). */}
          <div style={{ zoom: 'var(--ui-zoom, 1)' }}>
            <label className="listen-pop-row">
              <Volume2 size={14} aria-hidden="true" />
              <input
                ref={input}
                type="range"
                min="0"
                max="1"
                step="0.01"
                value={String(shown)}
                aria-label={label}
                onChange={(e) => change(Number(e.target.value))}
              />
              <span className="listen-pop-value">
                {pct}
                {PCT}
              </span>
            </label>
          </div>
        </RM.Content>
      </RM.Portal>
    </RM.Root>
  )
}
