// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, waitFor, cleanup, within } from '@testing-library/react'
import { Menu, type MenuEntry } from './Menu'

// The shared ⋯ / context menu, driven the way the keyboard and a pointer drive it.
afterEach(cleanup)

function host(items: MenuEntry[]) {
  const { container } = render(
    <div className="app">
      <Menu trigger={<button type="button">More</button>} items={items} className="probe-menu" />
    </div>,
  )
  return { container, trigger: screen.getByRole('button', { name: 'More' }) }
}

const press = (key: string, el: Element = document.activeElement!) => fireEvent.keyDown(el, { key })

describe('Menu', () => {
  it('opens from the keyboard as a portaled role=menu, named by its button, with the keyboard on its first item', async () => {
    const { container, trigger } = host([{ label: 'One', onSelect: vi.fn() }, { label: 'Two', onSelect: vi.fn() }])
    press('Enter', trigger)
    const menu = await screen.findByRole('menu')
    expect(container.contains(menu), 'portaled out of the host, like .ui-dialog').toBe(false)
    expect(menu.classList.contains('ui-menu') && menu.classList.contains('probe-menu')).toBe(true)
    expect(menu.getAttribute('aria-labelledby')).toBe(trigger.id)
    // The portal escapes `.app`'s zoom, so the content puts it back.
    expect((menu.firstElementChild as HTMLElement).style.zoom).toBe('var(--ui-zoom, 1)')
    expect(within(menu).getAllByRole('menuitem').map((i) => i.textContent)).toEqual(['One', 'Two'])
    await waitFor(() => expect(document.activeElement?.textContent).toBe('One'))
  })

  it('arrow keys move between items, skipping a disabled one; Enter chooses; the keyboard goes back to the button', async () => {
    const two = vi.fn()
    const off = vi.fn()
    const { trigger } = host([
      { label: 'One', onSelect: vi.fn() },
      { label: 'Off', onSelect: off, disabled: true, hint: 'Busy right now' },
      { label: 'Two', onSelect: two },
    ])
    press('ArrowDown', trigger)
    await screen.findByRole('menu')
    await waitFor(() => expect(document.activeElement?.textContent).toBe('One'))
    // Radix moves the keyboard a task after the key (a timeout), so each move is waited for.
    press('ArrowDown')
    await waitFor(() => expect(document.activeElement?.textContent, 'the disabled item is skipped').toBe('Two'))
    press('ArrowUp')
    await waitFor(() => expect(document.activeElement?.textContent).toBe('One'))
    press('End')
    await waitFor(() => expect(document.activeElement?.textContent).toBe('Two'))
    press('Enter')
    await waitFor(() => expect(two).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(screen.queryByRole('menu')).toBeNull())
    expect(document.activeElement).toBe(trigger)
    expect(off).not.toHaveBeenCalled()
  })

  it('a disabled item says why, and a click on it does nothing', async () => {
    const off = vi.fn()
    const { trigger } = host([{ label: 'Upload', onSelect: off, disabled: true, hint: 'Busy right now' }])
    press('Enter', trigger)
    const item = within(await screen.findByRole('menu')).getByRole('menuitem')
    expect(item.getAttribute('aria-disabled')).toBe('true')
    expect(item.textContent).toBe('UploadBusy right now')
    fireEvent.click(item)
    expect(off).not.toHaveBeenCalled()
  })

  it('Esc closes it without choosing, and the keyboard goes back to the button', async () => {
    const one = vi.fn()
    const { trigger } = host([{ label: 'One', onSelect: one }])
    press('Enter', trigger)
    await screen.findByRole('menu')
    press('Escape')
    await waitFor(() => expect(screen.queryByRole('menu')).toBeNull())
    expect(document.activeElement).toBe(trigger)
    expect(one).not.toHaveBeenCalled()
  })

  it('a press outside closes it without choosing', async () => {
    const one = vi.fn()
    const { trigger } = host([{ label: 'One', onSelect: one }])
    press('Enter', trigger)
    await screen.findByRole('menu')
    fireEvent.pointerDown(document.body)
    await waitFor(() => expect(screen.queryByRole('menu')).toBeNull())
    expect(one).not.toHaveBeenCalled()
  })

  it('a submenu opens with → and closes with ←, and its item is chosen like any other', async () => {
    const card = vi.fn()
    const { trigger } = host([
      { label: 'Spot', onSelect: vi.fn() },
      { label: 'QSL', items: [{ label: 'Bureau', onSelect: vi.fn() }, { label: 'Card received', onSelect: card }], separatorBefore: true },
    ])
    press('Enter', trigger)
    const menu = await screen.findByRole('menu')
    expect(within(menu).getAllByRole('separator')).toHaveLength(1)
    const sub = within(menu).getByRole('menuitem', { name: /QSL/ })
    expect(sub.getAttribute('aria-haspopup')).toBe('menu')
    sub.focus()
    press('ArrowRight', sub)
    await waitFor(() => expect(screen.getAllByRole('menu')).toHaveLength(2))
    await waitFor(() => expect(document.activeElement?.textContent).toBe('Bureau'))
    // The submenu is portaled too, so it puts the zoom back as the menu does.
    expect((screen.getAllByRole('menu')[1].firstElementChild as HTMLElement).style.zoom).toBe('var(--ui-zoom, 1)')
    press('ArrowLeft')
    await waitFor(() => expect(screen.getAllByRole('menu')).toHaveLength(1))
    expect(document.activeElement).toBe(sub)
    press('ArrowRight', sub)
    await waitFor(() => expect(document.activeElement?.textContent).toBe('Bureau'))
    press('ArrowDown')
    await waitFor(() => expect(document.activeElement?.textContent).toBe('Card received'))
    press('Enter')
    await waitFor(() => expect(card).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(screen.queryByRole('menu')).toBeNull())
  })

  it('an item that has moved the keyboard on by the time the menu has closed keeps it there', async () => {
    // A dialog that took the keyboard as the menu closed (the confirm question's way): the closing
    // menu does not pull it back to the button.
    const outside = document.createElement('input')
    document.body.append(outside)
    const { trigger } = host([{ label: 'Delete', onSelect: () => void setTimeout(() => outside.focus(), 0) }])
    press('Enter', trigger)
    const menu = await screen.findByRole('menu')
    await waitFor(() => expect(document.activeElement?.textContent).toBe('Delete'))
    press('Enter', within(menu).getByRole('menuitem'))
    await waitFor(() => expect(screen.queryByRole('menu')).toBeNull())
    await new Promise((r) => setTimeout(r, 50))
    expect(document.activeElement).toBe(outside)
    outside.remove()
  })
})
