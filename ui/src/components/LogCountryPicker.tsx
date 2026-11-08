// The Logbook's Country cell on the station: the entity a contact is shown under, as a button that
// opens a searchable list of every DXCC entity, current and deleted, to set by hand the one the
// contact counts toward (ADIF `DXCC`). An old log's record that carries no number counts by its
// call, and cty.dat places a 1×1 call like K1N in the United States whatever year it was: this is
// how the operator says which entity it was. Clearing it lets the call count again.
//
// It PORTALS (Radix DropdownMenu, the CountryExclude precedent): the Logbook's cells clip what
// overflows them, so a list anchored in the row would be cut off. The portal leaves `.app`'s zoom
// behind, so the content re-applies it.
//
// ⚠️ A React portal still BUBBLES its events through the component tree: a double-click in the
// list or the search box would reach the row's own double-click and open the contact's detail view
// behind the menu. The content stops it there.
//
// NOT modal, for the row's own double-click (#313's way to view a contact). A modal menu takes the
// page's pointer events the moment it opens, so the second click of a double-click on the country
// landed on nothing: the picker stayed open with its search box blurred, and the detail view never
// came (measured in Chromium). Not modal, that second click closes the picker again and the row
// opens the contact, as a double-click on the plain cell always did.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The entity names and
// numbers it lists are data (cty.dat's and the ARRL's); every word around them is the catalog's.

import * as RM from '@radix-ui/react-dropdown-menu'
import { MenuContent, MenuRoot } from './ui/MenuParts'
import { useRef, useState } from 'react'
import { getDxccEntities } from '../api'
import { findEntities, lotwDxccOf } from '../features/dxccPick'
import { t } from '../i18n'
import type { DxccEntityOption, LoggedQso } from '../types'

/** An entity as the picker names it: its name and number, and "deleted" where it is one. */
function entityLabel(e: DxccEntityOption): string {
  const named = `${e.name} (${e.code})`
  return e.deleted ? t('logbook.country.deletedEntity', { entity: named }) : named
}

interface Props {
  /** The contact, as its row shows it. */
  q: LoggedQso
  /** Set the contact's DXCC number; `null` clears it. */
  onPick: (dxcc: number | null) => void
}

export function LogCountryPicker({ q, onPick }: Props) {
  const [open, setOpen] = useState(false)
  const [search, setSearch] = useState('')
  // The station's table, read the first time this list opens: `null` until it answers.
  const [entities, setEntities] = useState<DxccEntityOption[] | 'unavailable' | null>(null)
  const listRef = useRef<HTMLDivElement>(null)
  const onOpenChange = (next: boolean) => {
    setOpen(next)
    if (!next) {
      setSearch('')
      return
    }
    if (entities === null || entities === 'unavailable') {
      void getDxccEntities()
        .then(setEntities)
        .catch(() => setEntities('unavailable'))
    }
  }
  const all = Array.isArray(entities) ? entities : []
  const { found, total } = findEntities(all, search)
  // An entity by its number, as the list names it — a number the table does not hold as itself.
  const named = (code: number) => {
    const e = all.find((x) => x.code === code)
    return e ? entityLabel(e) : t('awards.dxccDiffer.unknown', { code })
  }
  const lotw = lotwDxccOf(q)
  const shown = q.entity ?? q.country ?? '—'
  return (
    <MenuRoot open={open} onOpenChange={onOpenChange}>
      <RM.Trigger asChild>
        <button
          type="button"
          className="log-country-pick"
          // The stored COUNTRY text stays on the tooltip, as it did on the plain cell.
          title={[q.country, t('logbook.country.pick.title')].filter(Boolean).join('\n')}
          aria-label={t('logbook.country.pick.aria', { call: q.call, entity: shown })}
        >
          {shown}
        </button>
      </RM.Trigger>
      <RM.Portal>
        <MenuContent
          className="ui-menu country-menu log-country-menu"
          sideOffset={4}
          align="start"
          collisionPadding={8}
          onDoubleClick={(e) => e.stopPropagation()}
        >
          <div style={{ zoom: 'var(--ui-zoom, 1)' }}>
            {Array.isArray(entities) && (
              <div className="country-menu-note">
                {q.dxcc != null ? t('logbook.country.current', { entity: named(q.dxcc) }) : t('logbook.country.byCall')}
              </div>
            )}
            {lotw !== null && Array.isArray(entities) && (
              <div className="country-menu-note">{t('logbook.country.lotw', { entity: named(lotw) })}</div>
            )}
            {/* The way back (the satellite tag's Clear): offered only while there is a number to
                clear, so it never sits in the menu as a choice that does nothing — and at the top,
                where it is in view without scrolling past 400 entities. */}
            {q.dxcc != null && (
              <RM.Item className="ui-menu-item" onSelect={() => onPick(null)}>
                {t('logbook.country.clear')}
              </RM.Item>
            )}
            <div className="log-country-search">
              <input
                className="settings-input"
                type="text"
                value={search}
                // Typing is the way in: the list is 402 entities long.
                autoFocus
                placeholder={t('logbook.country.search.placeholder')}
                aria-label={t('logbook.country.search.placeholder')}
                autoComplete="off"
                spellCheck={false}
                onChange={(e) => setSearch(e.target.value)}
                onKeyDown={(e) => {
                  // Keys typed here are the search's, never the menu's typeahead. ↓ goes to the list.
                  e.stopPropagation()
                  if (e.key === 'ArrowDown') {
                    e.preventDefault()
                    listRef.current?.querySelector<HTMLElement>('[role="menuitemradio"]')?.focus()
                  }
                }}
              />
            </div>
            <RM.RadioGroup
              ref={listRef}
              value={q.dxcc != null ? String(q.dxcc) : ''}
              onValueChange={(v) => onPick(Number(v))}
            >
              {found.map((e) => (
                <RM.RadioItem key={e.code} value={String(e.code)} className="ui-menu-item country-item">
                  {entityLabel(e)}
                </RM.RadioItem>
              ))}
            </RM.RadioGroup>
            {entities === null ? (
              <div className="country-menu-note">{t('logbook.country.loading')}</div>
            ) : entities === 'unavailable' ? (
              <div className="country-menu-note">{t('logbook.country.unavailable')}</div>
            ) : total === 0 ? (
              <div className="country-menu-note">{t('logbook.country.noMatch')}</div>
            ) : (
              total > found.length && (
                <div className="country-menu-note">{t('logbook.country.more', { count: total - found.length })}</div>
              )
            )}
          </div>
        </MenuContent>
      </RM.Portal>
    </MenuRoot>
  )
}
