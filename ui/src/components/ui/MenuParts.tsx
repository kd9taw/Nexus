// THE ONE ROOT AND CONTENT EVERY RADIX MENU IN THE APP OPENS THROUGH, and why they are not modal.
//
//   STOP TX MUST WORK ON THE FIRST CLICK, WHATEVER IS OPEN.
//
// A Radix DropdownMenu is `modal` unless told otherwise, and a modal menu locks the page's pointer events
// (`pointer-events: none` on <body>) while it is open: a click anywhere outside it only closes it and reaches
// nothing. With the Help menu open, one click on Stop TX closed the menu and never stopped the transmission
// (measured in real Chrome: halts +0; the same click with the menu not modal: halts +1). Six of the app's
// twelve menus were modal, each by default, so the fix is the default, made here once: a press outside a
// menu closes it AND lands where it was aimed.
//
// `MenuRoot` takes no `modal` prop, so a modal menu cannot be written by passing one, and
// `menu-stop-click.test.tsx` holds every Radix menu in ui/src to these two parts.
//
// What not being modal changes, and what it keeps:
//   · The rest of the page stays live while a menu is open: a press elsewhere closes the menu and does what it
//     does with no menu open (a decode row's double-click calls that station, exactly as it would without one).
//   · The page is not marked aria-hidden behind the menu, nor its scroll locked.
//   · Keyboard: unchanged. Arrows, Home/End, typeahead and submenus are the menu's own and do not depend on
//     modality; Esc closes it.
//   · Focus on close (`MenuContent`): after Esc or a chosen item, the keyboard goes back to the menu's button
//     with `preventScroll` and `scrollIntoView({ block: 'nearest' })` (Radix's own return is a bare `focus()`,
//     which may scroll a shell to bring the button into view); after a press or focus outside, it stays where
//     the operator put it (on Stop TX, say), and so it does wherever anything else has taken it by the time the
//     return runs. A content's own `onCloseAutoFocus` runs first and, by preventing
//     the event, takes the return over, as the ATU and JS8 menus do.
//
// What this does NOT settle: a click reaches Stop TX only where no menu is DRAWN over it. That is stacking, not
// modality, and Stop TX standing above every overlay (menus and dialogs alike) is its own change; the menu-stop
// probe names the one case it found (the band pill's menu over the top bar's Stop TX at xs on the Logbook).
import * as RM from '@radix-ui/react-dropdown-menu'
import { forwardRef, useRef } from 'react'

/** A Radix DropdownMenu root that is never modal. */
export function MenuRoot(props: Omit<RM.DropdownMenuProps, 'modal'>) {
  return <RM.Root {...props} modal={false} />
}

/** A Radix DropdownMenu content whose close gives the keyboard back without scrolling anything. */
export const MenuContent = forwardRef<HTMLDivElement, RM.DropdownMenuContentProps>(function MenuContent(
  { onCloseAutoFocus, onInteractOutside, ...props },
  ref,
) {
  // Fresh per opening: the content unmounts when the menu closes.
  const outside = useRef(false)
  return (
    <RM.Content
      ref={ref}
      {...props}
      onInteractOutside={(e) => {
        onInteractOutside?.(e)
        if (!e.defaultPrevented) outside.current = true
      }}
      onCloseAutoFocus={(e) => {
        onCloseAutoFocus?.(e)
        if (e.defaultPrevented) return
        e.preventDefault()
        if (outside.current) return
        // The return runs a tick after the close: a control or another menu that has the keyboard by then keeps it.
        // (Taking it back closed a menu opened in that tick, which a menu that is not modal lets focus leave.)
        const at = document.activeElement
        const content = e.currentTarget instanceof Element ? e.currentTarget : null
        if (at && at !== document.body && at.isConnected && !content?.contains(at)) return
        // The content is labelled by its trigger (Radix sets both ids), and the event is dispatched on it.
        const id = content?.getAttribute('aria-labelledby')
        const trigger = id ? document.getElementById(id) : null
        if (!trigger?.isConnected) return
        trigger.focus({ preventScroll: true })
        trigger.scrollIntoView?.({ block: 'nearest' })
      }}
    />
  )
})
