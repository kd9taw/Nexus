// THE BAND MENU — the band dropdown as a Nexus menu, every band carrying its opening.
//
// ⚠️ ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Band names, channel labels, dial
// frequencies and the HF/VHF/UHF group names are invariant tokens handed in by the caller; the
// condition word is the backend's `BandModeled` enum (see `dualStateLabel`) or a catalog string.
//
// WHY A MENU AND NOT THE SYSTEM <select>. A native popup is drawn by the OS — on Linux as a
// separate GTK toplevel that never sees the app's CSS (#169) — so it cannot carry a coloured dot
// and it looks different on each platform. Radix DropdownMenu is portaled, re-applies the
// operator's zoom (the `Menu.tsx` pattern), and gives roving keyboard focus, typeahead and
// `menuitemradio` + `aria-checked` semantics for free.
//
// WHAT EACH ROW SAYS. The band, then a dot AND a word for its condition from `bandConditions.ts`
// — the same cell the Band conditions strip draws. A dot alone would be a colour-only signal; the
// word is what makes it readable to a colour-blind operator and to a screen reader.
//
// Nothing on this surface keys, unkeys or stops a transmission: a pick is a band QSY exactly as
// the select it replaces sent (BandPicker → `pickBand`, FrequencyControl → `onSet`).
import * as RM from '@radix-ui/react-dropdown-menu'
import { Fragment, useState, type CSSProperties, type ReactNode } from 'react'
import { useBandConditions } from '../bandConditions'

export interface BandMenuItem {
  /** The value handed back on pick (a band id or a channel key). */
  value: string
  label: ReactNode
  /** The band whose condition this row shows ('20m', '6m'). */
  conditionBand: string
  /** Optional group heading (HF/VHF/UHF) — consecutive items with the same group share one. */
  group?: string
  title?: string
}

interface Props {
  items: BandMenuItem[]
  /** The checked item's value, or null when the dial matches none. */
  value: string | null
  onPick: (value: string) => void
  disabled?: boolean
  triggerLabel: ReactNode
  triggerClassName: string
  triggerStyle?: CSSProperties
  ariaLabel: string
  title?: string
}

export function BandMenu({ items, value, onPick, disabled = false, triggerLabel, triggerClassName, triggerStyle, ariaLabel, title }: Props) {
  const [open, setOpen] = useState(false)
  const condition = useBandConditions()

  const groups: { group?: string; items: BandMenuItem[] }[] = []
  for (const it of items) {
    const last = groups[groups.length - 1]
    if (last && last.group === it.group) last.items.push(it)
    else groups.push({ group: it.group, items: [it] })
  }

  return (
    <RM.Root open={open && !disabled} onOpenChange={setOpen}>
      <RM.Trigger asChild disabled={disabled}>
        <button type="button" className={`band-menu-trigger ${triggerClassName}`} style={triggerStyle} aria-label={ariaLabel} title={title} disabled={disabled}>
          <span className="band-menu-value">{triggerLabel}</span>
          <span className="band-menu-caret" aria-hidden="true" />
        </button>
      </RM.Trigger>
      <RM.Portal>
        <RM.Content className="ui-menu band-menu" align="start" sideOffset={4} collisionPadding={8}>
          {/* The portal escapes `.app`'s zoom; re-apply it (Menu.tsx / Tooltip.tsx). */}
          <div style={{ zoom: 'var(--ui-zoom, 1)' }}>
            <RM.RadioGroup value={value ?? ''}>
              {groups.map((g, gi) => (
                <Fragment key={`${g.group ?? ''}-${gi}`}>
                  {g.group && <RM.Label className="band-menu-group">{g.group}</RM.Label>}
                  {g.items.map((it) => {
                    const c = condition(it.conditionBand)
                    return (
                      <RM.RadioItem
                        key={it.value}
                        value={it.value}
                        className="ui-menu-item band-menu-item"
                        data-condition={c.state}
                        title={it.title ? `${it.title}\n${c.title}` : c.title}
                        onSelect={() => onPick(it.value)}
                      >
                        <span className="band-menu-label">{it.label}</span>
                        <span className="band-menu-cond">
                          <span className="band-menu-dot" style={c.color ? { background: c.color } : undefined} aria-hidden="true" />
                          {c.word}
                        </span>
                      </RM.RadioItem>
                    )
                  })}
                </Fragment>
              ))}
            </RM.RadioGroup>
          </div>
        </RM.Content>
      </RM.Portal>
    </RM.Root>
  )
}
