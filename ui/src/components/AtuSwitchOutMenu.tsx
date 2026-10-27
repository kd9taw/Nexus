// The ATU button's menu (#344, operator ruling): a CLICK on the ATU button always runs a tune-up,
// whatever the tuner state, exactly as before ATU ON existed; switching the tuner OUT of line is
// this separate action, "Switch tuner out", opened by a right-click on the button or a long press
// on a touch screen. Offered only where Nexus can switch the tuner out (`radio.atuSwitchOut`: never
// Nexus's own Flex client, never the native Icom path) and only while the tuner is in line.
//
// Nothing here transmits: the switch-out keys nothing, and the backend refuses it on the air.
//
// The menu is anchored on an empty span INSIDE the button, so the strip's row gains no item and
// no gap. That has one consequence the handlers below answer: React bubbles events from a portal
// through the component tree, so a click on the menu's item would also reach the button's own
// onClick and run a tune-up. Every handler the button takes from here ignores an event whose
// target is not inside the button's DOM.
//
// ⚠️ Born migrated (i18n/hardcoded-strings.test.ts): every word comes from the catalogs.
import * as RM from '@radix-ui/react-dropdown-menu'
import { useCallback, useEffect, useRef, useState, type MouseEvent, type PointerEvent, type RefObject } from 'react'
import { t } from '../i18n'

/** How long a touch must be held on the ATU button to open its menu. */
export const ATU_LONG_PRESS_MS = 500

const fromButton = (e: { currentTarget: EventTarget; target: EventTarget }) =>
  (e.currentTarget as Node).contains(e.target as Node)

/**
 * The button side of the menu: its open state, the right-click / long-press handlers to spread on
 * the button, and `click(fn)`, which wraps the button's own click so that the click a long press
 * ends with, or one bubbling out of the menu, does not run `fn` (the tune-up).
 */
export function useAtuSwitchOutMenu(offered: boolean) {
  const [open, setOpen] = useState(false)
  const buttonRef = useRef<HTMLButtonElement>(null)
  const timer = useRef<number | null>(null)
  const touching = useRef(false)
  const swallowClick = useRef(false)
  const clear = useCallback(() => {
    if (timer.current != null) {
      window.clearTimeout(timer.current)
      timer.current = null
    }
  }, [])
  useEffect(() => clear, [clear])
  // The tuner went out (or the path lost its switch-out): the menu has nothing left to offer.
  useEffect(() => {
    if (!offered) {
      clear()
      setOpen(false)
    }
  }, [offered, clear])

  const release = () => {
    touching.current = false
    clear()
  }
  const handlers = offered
    ? {
        onContextMenu: (e: MouseEvent<HTMLButtonElement>) => {
          if (!fromButton(e)) return
          e.preventDefault()
          // A touch screen's own long-press menu event: the click that may follow is the press's.
          if (touching.current) swallowClick.current = true
          setOpen(true)
        },
        onPointerDown: (e: PointerEvent<HTMLButtonElement>) => {
          if (!fromButton(e)) return
          swallowClick.current = false
          if (e.pointerType === 'mouse') return
          touching.current = true
          clear()
          timer.current = window.setTimeout(() => {
            timer.current = null
            swallowClick.current = true
            setOpen(true)
          }, ATU_LONG_PRESS_MS)
        },
        onPointerUp: release,
        onPointerCancel: release,
        onPointerLeave: release,
      }
    : {}
  const click = (fn: (() => void) | undefined) => (e: MouseEvent<HTMLButtonElement>) => {
    if (!fromButton(e)) return
    if (swallowClick.current) {
      swallowClick.current = false
      return
    }
    fn?.()
  }
  return { open, setOpen, buttonRef, handlers, click, offered }
}

/** The menu itself, rendered INSIDE the ATU button (see the header for why). */
export function AtuSwitchOutMenu({
  open,
  onOpenChange,
  onSwitchOut,
  buttonRef,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  onSwitchOut: () => void
  buttonRef: RefObject<HTMLButtonElement | null>
}) {
  return (
    <RM.Root open={open} onOpenChange={onOpenChange}>
      <RM.Trigger asChild>
        <span className="atu-menu-anchor" aria-hidden="true" tabIndex={-1} />
      </RM.Trigger>
      <RM.Portal>
        <RM.Content
          className="ui-menu box-menu"
          aria-label={t('operate.strip.atu.menu.aria')}
          // The anchor is an empty span; name the menu itself rather than point at it.
          aria-labelledby={undefined}
          sideOffset={4}
          align="start"
          collisionPadding={8}
          onCloseAutoFocus={(e) => {
            e.preventDefault()
            buttonRef.current?.focus({ preventScroll: true })
          }}
        >
          {/* The portal escapes `.app`'s zoom, so the content re-applies it (Menu.tsx's reason). */}
          <div style={{ zoom: 'var(--ui-zoom, 1)' }}>
            <RM.Item className="ui-menu-item" onSelect={() => onSwitchOut()}>
              {t('operate.strip.atu.switchOut')}
            </RM.Item>
          </div>
        </RM.Content>
      </RM.Portal>
    </RM.Root>
  )
}
