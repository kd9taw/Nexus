// A Connect box's ⋯ menu — the box's own options, behind ONE control in its head: its text size
// (A− / A+, 80–160 % of the app's Text size).
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

export function BoxMenu({
  title,
  textScale,
  onTextScale,
}: {
  /** The box's name, already translated — the trigger's accessible name says whose menu it is. */
  title: string
  /** The box's text size, a factor on the app's (1 = the app's size). */
  textScale: number
  /** Change it. Omitted ⇒ no text-size entries. */
  onTextScale?: (factor: number) => void
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
          </div>
        </RM.Content>
      </RM.Portal>
    </RM.Root>
  )
}
