// The JS8 lists' menus: each list's ⋯ in its pane head (JS8Call's View menu and its header menu:
// Sort By…, Show Columns…, Show Band Heartbeats and ACKs, Clear) and the menu of one row (JS8Call's
// right-click menu on a row: Jump to …, Remove, Block, …), opened by the row's ⋯ or a right-click
// anywhere on the row. One shape for both: a list of entries, drawn by Radix's menu (the app's Menu
// primitive, as connect/BoxMenu draws its own), portalled out of the pane and re-applying --ui-zoom.
//
// Nothing here transmits: every entry is a view choice, a list edit or a move of the RX/TX cursors.
//
// ⚠️ Migrated from birth (i18n/hardcoded-strings.test.ts): `⋯`, `▸` and `●`/`✓` are glyphs; every word
// arrives from the caller's catalog keys.
import * as RM from '@radix-ui/react-dropdown-menu'
import { MenuContent, MenuRoot } from './ui/MenuParts'
import type { ReactNode } from 'react'

export type Js8MenuEntry =
  | { kind: 'item'; label: string; onSelect: () => void; disabled?: boolean }
  /** A switch; the menu stays open, so several can be set at once (JS8Call's Show Columns…). */
  | { kind: 'check'; label: string; checked: boolean; onChange: (on: boolean) => void }
  /** One choice of several (JS8Call's Sort By…). */
  | { kind: 'radio'; value: string; options: readonly { value: string; label: string }[]; onChange: (value: string) => void }
  | { kind: 'sub'; label: string; entries: readonly Js8MenuEntry[] }
  | { kind: 'sep' }
  | { kind: 'heading'; label: string }

/** The entries of a menu, drawn. Shared with the Call Roster's and Band Activity's row menu
 *  (CallRowMenu), which opens on a row rather than on a ⋯. */
export function Entries({ entries }: { entries: readonly Js8MenuEntry[] }) {
  return (
    <>
      {entries.map((e, i) => {
        switch (e.kind) {
          case 'item':
            return (
              <RM.Item key={i} className="ui-menu-item" disabled={e.disabled} onSelect={() => e.onSelect()}>
                {e.label}
              </RM.Item>
            )
          case 'check':
            return (
              <RM.CheckboxItem
                key={i}
                className="ui-menu-item"
                checked={e.checked}
                onCheckedChange={(on) => e.onChange(on === true)}
                onSelect={(ev) => ev.preventDefault()}
              >
                <span className="box-menu-glyph" aria-hidden="true">
                  <RM.ItemIndicator>✓</RM.ItemIndicator>
                </span>
                {e.label}
              </RM.CheckboxItem>
            )
          case 'radio':
            return (
              <RM.RadioGroup key={i} value={e.value} onValueChange={e.onChange}>
                {e.options.map((opt) => (
                  <RM.RadioItem key={opt.value} value={opt.value} className="ui-menu-item">
                    <span className="box-menu-glyph" aria-hidden="true">
                      <RM.ItemIndicator>●</RM.ItemIndicator>
                    </span>
                    {opt.label}
                  </RM.RadioItem>
                ))}
              </RM.RadioGroup>
            )
          case 'sub':
            return (
              <RM.Sub key={i}>
                <RM.SubTrigger className="ui-menu-item">
                  {e.label}
                  <span className="box-menu-chev" aria-hidden="true">
                    ▸
                  </span>
                </RM.SubTrigger>
                <RM.Portal>
                  <RM.SubContent className="ui-menu box-menu" sideOffset={2} collisionPadding={8}>
                    <div style={{ zoom: 'var(--ui-zoom, 1)' }}>
                      <Entries entries={e.entries} />
                    </div>
                  </RM.SubContent>
                </RM.Portal>
              </RM.Sub>
            )
          case 'sep':
            return <RM.Separator key={i} className="box-menu-sep" />
          case 'heading':
            return (
              <RM.Label key={i} className="box-menu-label">
                {e.label}
              </RM.Label>
            )
        }
      })}
    </>
  )
}

/**
 * A ⋯ menu. `entries` is a function, called only while the menu is open (Radix mounts no content
 * when closed), so a row builds its menu only when it opens. `open` and `onOpenChange` let a
 * right-click on the row open it; omitted, the ⋯ alone does.
 */
export function Js8ListMenu({
  label,
  title,
  className = 'pane-menu',
  entries,
  open,
  onOpenChange,
  children = '⋯',
}: {
  /** The trigger's accessible name: whose menu this is. */
  label: string
  title?: string
  className?: string
  entries: () => readonly Js8MenuEntry[]
  open?: boolean
  onOpenChange?: (open: boolean) => void
  children?: ReactNode
}) {
  return (
    <MenuRoot open={open} onOpenChange={onOpenChange}>
      <RM.Trigger asChild>
        <button type="button" className={className} aria-label={label} title={title ?? label}>
          {children}
        </button>
      </RM.Trigger>
      <RM.Portal>
        <MenuContent className="ui-menu box-menu js8-list-menu" sideOffset={4} align="end" collisionPadding={8}>
          {/* The portal escapes `.app`'s zoom, so the content re-applies it (Menu.tsx's reason). */}
          <div style={{ zoom: 'var(--ui-zoom, 1)' }}>
            <Entries entries={entries()} />
          </div>
        </MenuContent>
      </RM.Portal>
    </MenuRoot>
  )
}
