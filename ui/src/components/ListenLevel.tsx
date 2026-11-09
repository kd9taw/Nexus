// #468: THE LISTENING LEVEL, FROM THE TOP BAR. The same setting as Settings ▸ Radio ▸ "Receive audio
// on this computer" ▸ Listening level (`monitorLevel`): how loud Nexus plays the receive audio on
// this computer. Receive only; it does not touch transmit audio or anything the radio is told.
//
// IT ADDS NO WIDTH TO THE BAR. The bar has no slack at 1366×768 on FT (its now-bar fills the line):
// an inline slider wrapped it onto a second row, and so did even a 25 px icon chip, each moving the
// whole cockpit and Stop TX down by a row whenever receive audio was switched on. So the trigger is
// the bar's own RX level meter (`children`, TopBar's `.rx-level`), made a button whose box is the
// meter's exactly (`.listen-trigger`, cockpit-panes.css, resets every box property a button brings).
// The slider lives in a portaled popover (Radix DropdownMenu through ui/MenuParts, as ui/Menu, with
// the app zoom re-applied inside it), which takes no room in the bar.
//
// A drag is many changes and each save writes the settings file, so the slider shows the value
// under the pointer at once (`draft`) and hands the one it settles on to `onCommit`, once, 250 ms
// after the last change. The draft gives way when the saved level arrives back, or when the save
// fails, so the slider never sits on a value that was not kept.
import { useEffect, useRef, useState, type ReactNode } from 'react'
import * as RM from '@radix-ui/react-dropdown-menu'
import { Volume2 } from 'lucide-react'
import { t } from '../i18n'
import { MenuContent, MenuRoot } from './ui/MenuParts'

const SETTLE_MS = 250
/** The unit after the level's number: an invariant token. */
const PCT = '%'

export function ListenLevel({
  level,
  onCommit,
  className,
  children,
}: {
  /** The trigger's own look (TopBar passes the RX meter's class). */
  className: string
  /** What the trigger shows: the RX level meter it stands in for. */
  children: ReactNode
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
    // NOT MODAL (MenuRoot never is): a modal menu turns off pointer events everywhere else while it
    // is open, so the first click on Stop TX would only close this popover. A volume control is open
    // while the operator listens, which is exactly when Stop TX may be needed: a click outside closes
    // the popover AND reaches what it lands on. MenuContent gives the keyboard back to the trigger
    // after Esc without scrolling, and leaves it where an outside press put it.
    <MenuRoot open={open} onOpenChange={setOpen}>
      <RM.Trigger asChild>
        <button type="button" className={`${className} listen-trigger`} title={t('topbar.listen.title', { pct })} aria-label={label}>
          {children}
        </button>
      </RM.Trigger>
      <RM.Portal>
        <MenuContent className="ui-menu listen-pop" role="dialog" aria-label={label} sideOffset={4} align="end" collisionPadding={8}>
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
        </MenuContent>
      </RM.Portal>
    </MenuRoot>
  )
}
