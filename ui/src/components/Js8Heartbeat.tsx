// THE HEARTBEAT CONTROLS, as JS8Call 3.0.3 has them, for the JS8 cockpit's TX dock:
//   · HB NET — JS8Call's Mode ▸ "Enable Heartbeat Networking" (`SubModeHB`, off by default),
//     persisted. Without it JS8Call shows no HB button and acknowledges no heartbeat; at a speed
//     with no heartbeats (Turbo) its menu item is disabled (`prepareHeartbeatMode`,
//     mainwindow.cpp:4124-4134). It keys nothing by itself.
//   · HB — the heartbeat repeat (session-only) with JS8Call's context menu (mainwindow.cpp:
//     4309-4325, opened on HB by a right-click, or by the ▾ beside it): "Start / Stop heartbeat
//     timer" when an interval is set, and "Send heartbeat now". Shown only where JS8Call shows
//     its button (`js8HbShown`).
//   · What holds an armed HB ACK back right now (the speed, a message still arriving, the QSO
//     pause), and the stations the 55-minute rule stopped answering: the HB ACK chip's words.
//
// Nothing here stops a transmission (THE STOP LINE, cockpit-panes.css): turning HB off cancels the
// SCHEDULE, and a heartbeat already keyed finishes; Stop TX is in the TX strip. "Send heartbeat
// now" is a SENDER. Every one of these keys nothing unless TX is on, which the engine re-reads at
// every period.
//
// Migrated from birth (i18n/hardcoded-strings.test.ts): HB and HB NET are the mode's tokens
// (js8Vocab), and the ▾ is a glyph.
import { useEffect, useId, useLayoutEffect, useRef, useState } from 'react'
import { t } from '../i18n'
import { HB_NET } from '../js8Vocab'
import type { Js8Presence } from '../types'

const MENU_GLYPH = '▾'

/** JS8Call shows its HB button only with heartbeat networking on, at a speed that heartbeats
 *  (`prepareHeartbeatMode(canCurrentModeSendHeartbeat() && actionModeJS8HB->isChecked())`). A
 *  sample with no presence (an older station's, on the Remote page) keeps the button as it
 *  always was. */
export function js8HbShown(presence: Js8Presence | undefined): boolean {
  return presence === undefined || (presence.hbNetworking && presence.hbSpeed)
}

/** What holds an armed HB ACK back right now, in the operator's words, or null when nothing does.
 *  `speed` is the TX speed's name. */
export function js8HbAckSuppressedTitle(presence: Js8Presence | undefined, speed: string): string | null {
  switch (presence?.hbAckSuppressed) {
    case 'speed':
      return t('js8.dock.hbAck.suppressed.speed', { speed })
    case 'arriving':
      return t('js8.dock.hbAck.suppressed.arriving')
    case 'qsoPause':
      return t('js8.dock.hbAck.suppressed.qsoPause')
    default:
      return null
  }
}

/** The stations the 55-minute rule stopped answering, named for the HB ACK chip, or null. */
export function js8HbAckRateBlockedNote(presence: Js8Presence | undefined): string | null {
  const calls = presence?.hbAckRateBlocked ?? []
  return calls.length === 0 ? null : t('js8.dock.hbAck.rateBlocked', { calls: calls.join(', ') })
}

/** HB NET: JS8Call's "Enable Heartbeat Networking", a persisted switch the dock turns on and off
 *  (`onToggle` gets the state it has now). Disabled at a speed with no heartbeats, as JS8Call
 *  disables its menu item there; it keeps its state for when the speed comes back. */
export function Js8HbNetChip({
  presence,
  speed,
  onToggle,
}: {
  presence: Js8Presence
  speed: string
  onToggle: (on: boolean) => void
}) {
  const on = presence.hbNetworking
  const title = !presence.hbSpeed
    ? t('js8.dock.hbNet.title.speed', { speed })
    : on
      ? t('js8.dock.hbNet.title.on')
      : t('js8.dock.hbNet.title.off')
  return (
    <button
      type="button"
      className={`cw-macro rtty-arm js8-arm js8-hbnet${on ? ' on' : ''}`}
      aria-pressed={on}
      disabled={!presence.hbSpeed}
      onClick={() => onToggle(on)}
      title={title}
    >
      <span className="cw-macro-label">{HB_NET}</span>
    </button>
  )
}

/** The HB button and its menu. `label` is what the button reads (`HB`, or `HB (42)` counting
 *  down), `title` its tooltip. The menu offers the timer only with an interval set, as JS8Call's
 *  does (`if (m_hbInterval > 0)`). It stands on the dock's top edge over HB, as the macro editor
 *  does, so it covers no control of the dock; `onOpenChange` lets the dock lift itself over the
 *  panes while it is open. */
export function Js8HbButton({
  label,
  title,
  on,
  armed,
  canControl,
  intervalMin,
  onToggle,
  onSendNow,
  onOpenChange,
}: {
  label: string
  title: string
  on: boolean
  armed: boolean
  canControl: boolean
  intervalMin: number
  onToggle: () => void
  onSendNow: () => void
  onOpenChange?: (open: boolean) => void
}) {
  const [open, setOpenState] = useState(false)
  const [left, setLeft] = useState(0)
  const rootRef = useRef<HTMLSpanElement>(null)
  const menuRef = useRef<HTMLSpanElement>(null)
  const menuId = useId()
  const openChange = useRef(onOpenChange)
  openChange.current = onOpenChange
  const setOpen = (next: boolean) => {
    setOpenState(next)
    openChange.current?.(next)
  }
  // A click anywhere else, or Escape, closes the menu (Escape also stops TX: that listener is the
  // cockpit's, and this one never swallows the key).
  useEffect(() => {
    if (!open) return
    const onDown = (e: PointerEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) {
        setOpenState(false)
        openChange.current?.(false)
      }
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setOpenState(false)
        openChange.current?.(false)
      }
    }
    window.addEventListener('pointerdown', onDown)
    window.addEventListener('keydown', onKey)
    return () => {
      window.removeEventListener('pointerdown', onDown)
      window.removeEventListener('keydown', onKey)
    }
  }, [open])
  // Over HB, in layout units against the dock (the control's nearest positioned ancestor), so the
  // app's zoom cancels out, and kept inside the dock's right edge.
  useLayoutEffect(() => {
    const root = rootRef.current
    const dock = root?.offsetParent as HTMLElement | null | undefined
    if (!open || !root || !dock || !menuRef.current) return
    setLeft(Math.max(0, Math.min(root.offsetLeft, dock.clientWidth - menuRef.current.offsetWidth)))
  }, [open])
  // Unmounted with the menu open (heartbeat networking off, Turbo): the dock goes back down.
  useEffect(() => () => openChange.current?.(false), [])
  const pick = (act: () => void) => {
    setOpen(false)
    act()
  }
  return (
    <span className="js8-hb-control" ref={rootRef}>
      <button
        type="button"
        className={`cw-macro rtty-arm js8-arm js8-hb${on ? ' on' : ''}${on && armed ? ' armed' : ''}`}
        aria-pressed={on}
        disabled={!canControl}
        onClick={onToggle}
        onContextMenu={(e) => {
          if (!canControl) return
          e.preventDefault()
          setOpen(true)
        }}
        title={title}
      >
        <span className="cw-macro-label">{label}</span>
      </button>
      <button
        type="button"
        className="cw-macro js8-hb-menu-btn"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        aria-label={t('js8.dock.hb.menu.aria')}
        title={t('js8.dock.hb.menu.title')}
        disabled={!canControl}
        onClick={() => setOpen(!open)}
      >
        {MENU_GLYPH}
      </button>
      {open && (
        <span className="js8-hb-menu" role="menu" id={menuId} aria-label={t('js8.dock.hb.menu.aria')} ref={menuRef} style={{ left }}>
          {intervalMin > 0 && (
            <button type="button" role="menuitem" className="cw-macro js8-hb-menu-item" onClick={() => pick(onToggle)}>
              {on ? t('js8.dock.hb.menu.stop') : t('js8.dock.hb.menu.start')}
            </button>
          )}
          <button type="button" role="menuitem" className="cw-macro js8-hb-menu-item" onClick={() => pick(onSendNow)}>
            {t('js8.dock.hb.menu.now')}
          </button>
        </span>
      )}
    </span>
  )
}
