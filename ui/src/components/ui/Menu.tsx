// Radix DropdownMenu styled with Nexus tokens — context/overflow menus (e.g.
// per-row actions on decodes/roster in P2). Accessible roving focus for free:
// role=menu/menuitem, arrow keys, Home/End, typeahead, Esc and a click outside close it,
// and the keyboard goes back to the trigger — unless the chosen item has already moved it on (a
// dialog it opened). A submenu opens with → or Enter and closes with ←.
//
// AN ITEM THAT OPENS A DIALOG WITHOUT A FOCUS TRAP OF ITS OWN (`opensDialog`) runs only once the
// menu has closed and given the keyboard back to its button. Run while the menu is still up, the
// menu's own trap pulls the keyboard back out of the dialog the moment the dialog takes it, and the
// closing menu then puts it on the button, behind the dialog. Run after, the dialog opens from the
// button exactly as if the button had opened it, and gives the keyboard back to it when it closes.
// (A ui/Dialog or the confirm question needs none of this: its own trap takes the keyboard over.)
import * as RM from '@radix-ui/react-dropdown-menu'
import { useRef, useState, type ReactNode } from 'react'

export interface MenuItem {
  label: string
  onSelect: () => void
  disabled?: boolean
  icon?: ReactNode
  /** A second, quieter line under the label: the item's state, or why it is disabled. It is part
   *  of the item's text, so a screen reader reads it with the label. */
  hint?: string
  /** The item's tooltip. */
  title?: string
  /** A destructive item, drawn in the warning colour. */
  danger?: boolean
  /** A rule above the item, starting a new group. */
  separatorBefore?: boolean
  /** The item opens a dialog that does not trap the keyboard itself: run it after the menu has
   *  closed (see the header). */
  opensDialog?: boolean
  /** A disabled item the keyboard still stops on, so its hint (why it cannot be chosen) is heard.
   *  Choosing it does nothing and leaves the menu open. */
  focusableWhenDisabled?: boolean
}

/** An item that opens a submenu instead of doing something. */
export interface SubMenu extends Omit<MenuItem, 'onSelect'> {
  items: MenuEntry[]
}

export type MenuEntry = MenuItem | SubMenu

interface MenuProps {
  trigger: ReactNode
  items: MenuEntry[]
  className?: string
}

const SUB_ARROW = '▸'

function itemBody(it: MenuEntry): ReactNode {
  return (
    <>
      {it.icon && <span className="ui-menu-icon">{it.icon}</span>}
      <span className="ui-menu-text">
        <span className="ui-menu-label">{it.label}</span>
        {it.hint && <span className="ui-menu-hint">{it.hint}</span>}
      </span>
    </>
  )
}

function Items({ items, className, later }: { items: MenuEntry[]; className?: string; later: (run: () => void) => void }) {
  return items.map((it, i) => {
    const cls = ['ui-menu-item', it.danger && 'danger'].filter(Boolean).join(' ')
    return [
      it.separatorBefore && i > 0 ? <RM.Separator key={`sep-${i}`} className="ui-menu-sep" /> : null,
      'items' in it ? (
        <RM.Sub key={i}>
          <RM.SubTrigger className={cls} disabled={it.disabled} title={it.title}>
            {itemBody(it)}
            <span className="ui-menu-subarrow" aria-hidden="true">
              {SUB_ARROW}
            </span>
          </RM.SubTrigger>
          <RM.Portal>
            <RM.SubContent className={['ui-menu', className].filter(Boolean).join(' ')} sideOffset={2} collisionPadding={8}>
              <div style={{ zoom: 'var(--ui-zoom, 1)' }}>
                <Items items={it.items} className={className} later={later} />
              </div>
            </RM.SubContent>
          </RM.Portal>
        </RM.Sub>
      ) : it.disabled && it.focusableWhenDisabled ? (
        // Radix skips a `disabled` item; this one is enabled for the keyboard and says it is not.
        <RM.Item key={i} className={cls} aria-disabled data-disabled="" onSelect={(e) => e.preventDefault()} title={it.title}>
          {itemBody(it)}
        </RM.Item>
      ) : (
        <RM.Item
          key={i}
          className={cls}
          disabled={it.disabled}
          onSelect={it.opensDialog ? () => later(it.onSelect) : it.onSelect}
          title={it.title}
        >
          {itemBody(it)}
        </RM.Item>
      ),
    ]
  })
}

export function Menu({ trigger, items, className }: MenuProps) {
  const [open, setOpen] = useState(false)
  const afterClose = useRef<(() => void) | null>(null)
  return (
    <RM.Root open={open} onOpenChange={setOpen}>
      <RM.Trigger asChild>{trigger}</RM.Trigger>
      <RM.Portal>
        <RM.Content
          className={['ui-menu', className].filter(Boolean).join(' ')}
          sideOffset={4}
          align="end"
          collisionPadding={8}
          onCloseAutoFocus={(e) => {
            const run = afterClose.current
            afterClose.current = null
            if (run) {
              // The keyboard back on the button first, so the dialog opens from it.
              const button = document.getElementById((e.currentTarget as Element).getAttribute('aria-labelledby') ?? '')
              button?.focus({ preventScroll: true })
              e.preventDefault()
              run()
              return
            }
            // An item that opened a dialog has put the keyboard in it by the time the menu finishes
            // closing; taking it back to the button would leave it behind the dialog. Anywhere else
            // (the item gone with the menu, Esc) it goes back to the button as usual.
            const at = document.activeElement
            if (at instanceof HTMLElement && at !== document.body && at.isConnected && !at.closest('[role="menu"]'))
              e.preventDefault()
          }}
        >
          {/* Same portal-zoom re-application as Dialog/Tooltip (see Tooltip.tsx for
              why an inner wrapper is positioning-safe): the portal escapes `.app`'s
              zoom:var(--ui-zoom), so content must re-apply it. Item roving focus is
              context-based, so the extra div does not break keyboard navigation. */}
          <div style={{ zoom: 'var(--ui-zoom, 1)' }}>
            <Items items={items} className={className} later={(run) => (afterClose.current = run)} />
          </div>
        </RM.Content>
      </RM.Portal>
    </RM.Root>
  )
}
