// A Connect box's ⋯ menu — the box's own options, behind ONE control in its head: its text size
// (A− / A+, 80–160 % of the app's Text size) and "? … in the manual", a link to the manual section
// that describes it (connect/paneHelp; absent for a pane the manual does not describe yet).
//
// The manual link is a real `<a target="_blank">` inside the menu item (Radix `asChild`), so both a
// click and Enter reach the app's one external-link path (externalLinks.ts opens it in the browser
// through the `open_external_url` command); in a plain browser — the TV page, the Remote page — the
// anchor opens a tab by itself. Nothing is fetched until the operator asks.
//
// TABS: "Add a tab" opens a submenu of the panes the slot can take (connectConfig `addableTo`), in
// the picker's own groups; "Remove … from this slot" takes the shown pane out while the slot holds
// two or more. Both close the menu — the slot has changed under it.
//
// WHY ONE MENU AND NOT A ROW OF BUTTONS. A box head already carries the title, the content picker
// and ✕, and a Connect column can be 200 px wide (Map first, or a dragged rail). Three more buttons
// there (A−, A+, ?) would leave the picker a few pixels, or push ✕ past the frame's clip — the
// failure the head's shrink rules exist to prevent (styles.css `.pane-acts`). One ⋯ costs one button
// width, and it is the same control on every box. The menu is Radix's (the app's Menu primitive),
// portalled out of the frame's `overflow: hidden` and re-applying --ui-zoom, as Menu.tsx does.
//
// A− and A+ keep the menu OPEN (onSelect → preventDefault), so the size can be stepped several times
// and read back from the line above them without reopening it.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). `A−`, `A+` and `⋯` are the
// controls' glyphs — symbols of one letter or none, like ✕ — and every word is in the catalog.
import * as RM from '@radix-ui/react-dropdown-menu'
import { t } from '../../i18n'
import { BOX_SCALE_MAX, BOX_SCALE_MIN, BOX_SCALE_STEP } from '../../features/panelState'
import type { PaneId } from '../../features/connectConfig'
import { PANES } from './panes'

export function BoxMenu({
  title,
  textScale,
  onTextScale,
  helpUrl,
  addable,
  onAddTab,
  onRemoveTab,
}: {
  /** The box's name, already translated — the trigger's accessible name says whose menu it is. */
  title: string
  /** The box's text size, a factor on the app's (1 = the app's size). */
  textScale: number
  /** Change it. Omitted ⇒ no text-size entries. */
  onTextScale?: (factor: number) => void
  /** The manual section that describes this box. Null/omitted ⇒ no link. */
  helpUrl?: string | null
  /** The panes this slot can take as a tab. */
  addable?: readonly PaneId[]
  /** Add one. Omitted ⇒ no "Add a tab". */
  onAddTab?: (paneId: PaneId) => void
  /** Take the shown pane out of the slot. Omitted ⇒ no "Remove" (a slot's only pane). */
  onRemoveTab?: () => void
}) {
  const pct = Math.round(textScale * 100)
  const step = (dir: 1 | -1) => onTextScale?.(Math.round((textScale + dir * BOX_SCALE_STEP) * 100) / 100)
  return (
    <RM.Root>
      <RM.Trigger asChild>
        <button
          type="button"
          className="pane-menu"
          aria-label={t('connect.box.menu.aria', { title })}
          title={t('connect.box.menu.title')}
        >
          ⋯
        </button>
      </RM.Trigger>
      <RM.Portal>
        <RM.Content className="ui-menu box-menu" sideOffset={4} align="end" collisionPadding={8}>
          {/* The portal escapes `.app`'s zoom, so the content re-applies it (Menu.tsx's reason). */}
          <div style={{ zoom: 'var(--ui-zoom, 1)' }}>
            {onTextScale && (
              <RM.Group>
                <RM.Label className="box-menu-label">{t('connect.box.text.size', { pct })}</RM.Label>
                <RM.Item
                  className="ui-menu-item"
                  disabled={textScale <= BOX_SCALE_MIN}
                  onSelect={(e) => {
                    e.preventDefault()
                    step(-1)
                  }}
                >
                  <span className="box-menu-glyph" aria-hidden="true">
                    A−
                  </span>
                  {t('connect.box.text.smaller')}
                </RM.Item>
                <RM.Item
                  className="ui-menu-item"
                  disabled={textScale >= BOX_SCALE_MAX}
                  onSelect={(e) => {
                    e.preventDefault()
                    step(1)
                  }}
                >
                  <span className="box-menu-glyph" aria-hidden="true">
                    A+
                  </span>
                  {t('connect.box.text.larger')}
                </RM.Item>
              </RM.Group>
            )}
            {onTextScale && (onAddTab || onRemoveTab) && <RM.Separator className="box-menu-sep" />}
            {onAddTab && (
              <RM.Sub>
                <RM.SubTrigger className="ui-menu-item" disabled={!addable?.length}>
                  <span className="box-menu-glyph" aria-hidden="true">
                    +
                  </span>
                  {t('connect.box.tab.add')}
                  <span className="box-menu-chev" aria-hidden="true">
                    ▸
                  </span>
                </RM.SubTrigger>
                <RM.Portal>
                  <RM.SubContent className="ui-menu box-menu box-menu-panes" sideOffset={2} collisionPadding={8}>
                    <div style={{ zoom: 'var(--ui-zoom, 1)' }}>
                      {(['core', 'b2', 'b3'] as const).map((cat) => {
                        const items = PANES.filter((p) => p.category === cat && addable?.includes(p.id))
                        return items.length ? (
                          <RM.Group key={cat}>
                            <RM.Label className="box-menu-label">
                              {cat === 'core' ? t('connect.slot.group.core') : cat.toUpperCase()}
                            </RM.Label>
                            {items.map((p) => (
                              <RM.Item key={p.id} className="ui-menu-item" onSelect={() => onAddTab(p.id)}>
                                {p.title}
                              </RM.Item>
                            ))}
                          </RM.Group>
                        ) : null
                      })}
                    </div>
                  </RM.SubContent>
                </RM.Portal>
              </RM.Sub>
            )}
            {onRemoveTab && (
              <RM.Item className="ui-menu-item" onSelect={() => onRemoveTab()}>
                <span className="box-menu-glyph" aria-hidden="true">
                  −
                </span>
                {t('connect.box.tab.remove', { title })}
              </RM.Item>
            )}
            {(onTextScale || onAddTab || onRemoveTab) && helpUrl && <RM.Separator className="box-menu-sep" />}
            {helpUrl && (
              <RM.Item asChild className="ui-menu-item box-menu-link">
                <a href={helpUrl} target="_blank" rel="noreferrer" title={t('connect.box.help.title')}>
                  <span className="box-menu-glyph" aria-hidden="true">
                    ?
                  </span>
                  {t('connect.box.help', { title })}
                </a>
              </RM.Item>
            )}
          </div>
        </RM.Content>
      </RM.Portal>
    </RM.Root>
  )
}
