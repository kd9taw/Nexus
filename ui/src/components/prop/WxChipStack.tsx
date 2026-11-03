// THE SPACE-WEATHER CHIP STACK — one column at the map's top-left, beside the Layers panel, on the
// 2-D map and the 3-D globe alike: the replay bar or the flare chip, the proton chip, the aurora
// chip, then NOAA's storm-watch chip (`.wx-chip-stack` in styles.css places it).
//
// On a narrow map (narrower than the Layers panel and the Conditions rail side by side, the 1024
// floor) a big-event day can put four chips over most of the map cell. There the stack shows the
// first two (the replay bar, when there is one, is always first: a replay is never mistaken for now)
// and one "+N more" chip for the rest. That chip opens every chip it stands for, whole, in a list
// (a portaled ui/Dialog: focus trap, Escape, the keyboard back on the chip when it closes). When
// everything fits there is no "+N more" chip, and a wide map shows every chip as before.
//
// The storm-watch chip decides for itself whether it has something to say, so the stack reads the
// same text (`useArrivalChipText`) to count it, and draws it from that text.

import { Fragment, useState, type ReactNode } from 'react'
import { t } from '../../i18n'
import { Dialog } from '../ui/Dialog'
import { useFocusReturn } from '../../focusReturn'
import { ArrivalChipView, useArrivalChipText } from './ArrivalChip'

/** The chips a narrow map shows before the rest go behind "+N more" (the plan's "at most two"). */
export const WX_STACK_NARROW_SHOWN = 2

export interface WxChip {
  key: string
  node: ReactNode
}

export function WxChipStack({
  compact,
  chips,
}: {
  /** The map is narrower than the Layers panel and the Conditions rail side by side. */
  compact: boolean
  /** In stack order; `false`/`null` for a chip that is not showing. */
  chips: Array<WxChip | false | null | undefined>
}) {
  const arrival = useArrivalChipText()
  const [open, setOpen] = useState(false)

  type Item = { key: string; render: (inList: boolean) => ReactNode }
  const items: Item[] = chips.filter((c): c is WxChip => !!c).map((c) => ({ key: c.key, render: () => c.node }))
  if (arrival) items.push({ key: 'arrival', render: (inList) => <ArrivalChipView text={arrival} compact={compact && !inList} /> })

  const shown = compact ? items.slice(0, WX_STACK_NARROW_SHOWN) : items
  const hidden = compact ? items.slice(WX_STACK_NARROW_SHOWN) : []
  // The list closes for good when the last chip it held goes (it does not spring open again with
  // the next one).
  if (open && hidden.length === 0) setOpen(false)
  const listOpen = open && hidden.length > 0
  const returnFocus = useFocusReturn(listOpen)

  return (
    <div className="wx-chip-stack">
      {shown.map((it) => (
        <Fragment key={it.key}>{it.render(false)}</Fragment>
      ))}
      {hidden.length > 0 && (
        <button
          type="button"
          className="wx-chip-more"
          aria-haspopup="dialog"
          aria-expanded={listOpen}
          aria-label={t('map.wx.more.aria', { n: hidden.length })}
          onClick={() => setOpen(true)}
        >
          {t('map.wx.more', { n: hidden.length })}
        </button>
      )}
      <Dialog
        open={listOpen}
        onOpenChange={setOpen}
        title={t('map.wx.more.title')}
        className="wx-more-dialog"
        onCloseAutoFocus={returnFocus}
      >
        <div className="wx-more-list">
          {hidden.map((it) => (
            <Fragment key={it.key}>{it.render(true)}</Fragment>
          ))}
        </div>
      </Dialog>
    </div>
  )
}
