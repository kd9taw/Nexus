// Driving a Logbook row's ⋯ menu from a test, the way the keyboard does: Enter on the row's ⋯ opens
// it, a submenu opens from its item, and the last item named is chosen. An item is found by the
// START of its accessible name, because the name also carries the item's state line ("Not
// uploaded", "Uploaded, 2026-10-28 …").
import { expect } from 'vitest'
import { fireEvent, screen, waitFor, within } from '@testing-library/react'
import { t } from '../i18n'

const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')

/** The row's ⋯ button for the contact with `call`. */
export const rowMore = (call: string) => screen.getByRole('button', { name: t('logbook.row.more', { call }) })

/** Open the ⋯ menu of `call`'s row and return the menu. */
export async function openRowMenu(call: string): Promise<HTMLElement> {
  fireEvent.keyDown(rowMore(call), { key: 'Enter' })
  return screen.findByRole('menu')
}

/** The item whose name starts with `label` in the open `menu`. */
export const menuItem = (menu: HTMLElement, label: string) =>
  within(menu).getByRole('menuitem', { name: new RegExp('^' + escape(label)) })

/** Open `call`'s ⋯ menu, walk the submenus named before the last label, and choose the last. */
export async function chooseRowAction(call: string, ...path: string[]): Promise<void> {
  let menu = await openRowMenu(call)
  for (const [i, label] of path.entries()) {
    const item = menuItem(menu, label)
    if (i === path.length - 1) {
      fireEvent.click(item)
      return
    }
    fireEvent.keyDown(item, { key: 'ArrowRight' })
    menu = await waitFor(() => {
      const menus = screen.getAllByRole('menu')
      expect(menus.length).toBeGreaterThan(1)
      return menus[menus.length - 1]
    })
  }
}

/** Open `call`'s ⋯ menu and its submenu `label` (QSL, Satellite); return the submenu. */
export async function openRowSubmenu(call: string, label: string): Promise<HTMLElement> {
  const menu = await openRowMenu(call)
  fireEvent.keyDown(menuItem(menu, label), { key: 'ArrowRight' })
  return waitFor(() => {
    const menus = screen.getAllByRole('menu')
    expect(menus).toHaveLength(2)
    return menus[1]
  })
}

/** The labels of a menu's items, without their state lines. */
export const itemLabels = (menu: HTMLElement) =>
  [...menu.querySelectorAll('[role="menuitem"]')].map((i) => i.querySelector('.ui-menu-label')?.textContent ?? '')
