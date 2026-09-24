// @vitest-environment jsdom
//
// The confirm dialog replaces window.confirm, which is INERT in the Tauri webview: WKWebView only
// shows a JS confirm if the host implements runJavaScriptConfirmPanel, and wry does not. So
// `confirm()` returned false with no dialog, and every `if (!window.confirm(…)) return` guard
// silently cancelled — Remove radio, Restore, Reset and eleven others did nothing at all
// (operator, 2026-08-14).
//
// What is pinned here is the property that matters most: with no host mounted it must resolve
// FALSE. A missing dialog that answered "yes" would turn this bug into silent data loss, which is
// strictly worse than the silence it replaces.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { act, render, screen, cleanup, waitFor, fireEvent } from '@testing-library/react'
import { useState } from 'react'
import { confirmDialog, ConfirmHost } from './confirm'

afterEach(cleanup)

describe('confirmDialog', () => {
  it('resolves FALSE when no host is mounted — never proceeds unconfirmed', async () => {
    const err = vi.spyOn(console, 'error').mockImplementation(() => {})
    await expect(confirmDialog({ title: 'Delete everything?' })).resolves.toBe(false)
    expect(err).toHaveBeenCalled() // and says so, rather than failing mutely
    err.mockRestore()
  })

  it('shows the question and resolves true only on the affirmative button', async () => {
    render(<ConfirmHost />)
    const answer = confirmDialog({
      title: 'Remove FT-710?',
      body: 'This deletes its CAT config.',
      confirmLabel: 'Remove radio',
    })
    await waitFor(() => expect(screen.getByText('Remove FT-710?')).toBeTruthy())
    expect(screen.getByText('This deletes its CAT config.')).toBeTruthy()
    screen.getByRole('button', { name: 'Remove radio' }).click()
    await expect(answer).resolves.toBe(true)
  })

  it('answers a superseded question NO instead of leaving its await hanging forever', async () => {
    render(<ConfirmHost />)
    const first = confirmDialog({ title: 'Delete radio 1?' })
    await waitFor(() => expect(screen.getByText('Delete radio 1?')).toBeTruthy())

    // A second question arrives before the first is answered — the fire-and-forget call sites in
    // RadioProgView do not await one another. The first used to be dropped unresolved, so its
    // caller waited for the lifetime of the window.
    const second = confirmDialog({ title: 'Delete radio 2?' })
    await expect(first).resolves.toBe(false)

    // And the survivor is genuinely still live, not collateral damage from settling the first.
    await waitFor(() => expect(screen.getByText('Delete radio 2?')).toBeTruthy())
    screen.getByRole('button', { name: 'Confirm' }).click()
    await expect(second).resolves.toBe(true)
  })

  it('resolves false on Cancel', async () => {
    render(<ConfirmHost />)
    const answer = confirmDialog({ title: 'Reset everything?' })
    await waitFor(() => expect(screen.getByText('Reset everything?')).toBeTruthy())
    screen.getByRole('button', { name: 'Cancel' }).click()
    await expect(answer).resolves.toBe(false)
  })
})

// THE KEYBOARD GOES BACK TO THE CONTROL THAT ASKED (the standard pattern for a dialog). The dialog is
// opened by code, not by a Radix trigger, and Radix gives the focus back to a trigger only: so every
// question closed with the keyboard nowhere — on the page itself — and a keyboard or screen-reader
// operator had to find their place again from the top. Now it returns to the control the question
// was asked from; and if a YES then removes that control (a delete), to the same control in the item
// that takes its place — the next one, else the one before — or, when no item is left, to the list.
describe('the keyboard goes back to the control that asked', () => {
  function Asker({ onAnswer }: { onAnswer?: (ok: boolean) => void }) {
    return (
      <>
        <button type="button">Before</button>
        <button type="button" onClick={() => void confirmDialog({ title: 'Delete it?', confirmLabel: 'Delete' }).then((ok) => onAnswer?.(ok))}>
          Ask
        </button>
        <button type="button">After</button>
        <ConfirmHost />
      </>
    )
  }
  const ask = async (name = 'Ask') => {
    const opener = screen.getByRole('button', { name })
    act(() => opener.focus())
    fireEvent.click(opener)
    await screen.findByRole('dialog')
    return opener
  }

  it('on Cancel', async () => {
    render(<Asker />)
    const opener = await ask()
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    await waitFor(() => expect(document.activeElement).toBe(opener))
  })

  it('on Escape', async () => {
    render(<Asker />)
    const opener = await ask()
    fireEvent.keyDown(document.activeElement!, { key: 'Escape' })
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    await waitFor(() => expect(document.activeElement).toBe(opener))
  })

  it('on a yes that leaves it where it was', async () => {
    const answers: boolean[] = []
    render(<Asker onAnswer={(ok) => answers.push(ok)} />)
    const opener = await ask()
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }))
    await waitFor(() => expect(answers).toEqual([true]))
    await waitFor(() => expect(document.activeElement).toBe(opener))
  })

  /** A list whose ✕ deletes its item — after the yes, and after a round trip, as a real one does. */
  function List({ items }: { items: string[] }) {
    const [left, setLeft] = useState(items)
    return (
      <>
        <button type="button">Add</button>
        <ul aria-label="Pictures">
          {left.map((name) => (
            <li key={name}>
              <span>{name}</span>
              <button
                type="button"
                className="item-del"
                aria-label={`Delete ${name}`}
                onClick={async () => {
                  if (!(await confirmDialog({ title: `Delete ${name}?`, confirmLabel: 'Delete' }))) return
                  await new Promise((r) => setTimeout(r, 30))
                  setLeft((l) => l.filter((n) => n !== name))
                }}
              >
                ✕
              </button>
            </li>
          ))}
        </ul>
        <ConfirmHost />
      </>
    )
  }
  const deleteVia = async (name: string) => {
    await ask(`Delete ${name}`)
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }))
    await waitFor(() => expect(screen.queryByRole('button', { name: `Delete ${name}` })).toBeNull())
  }

  it('a yes that removes it: the same control in the item that takes its place', async () => {
    render(<List items={['one', 'two', 'three']} />)
    await deleteVia('two')
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Delete three' })))
  })

  it('…the one before, when it was the last', async () => {
    render(<List items={['one', 'two', 'three']} />)
    await deleteVia('three')
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Delete two' })))
  })

  it('…and the list itself when no item is left — never the page', async () => {
    render(<List items={['only']} />)
    await deleteVia('only')
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole('list', { name: 'Pictures' })))
  })

  it('a control disabled by the yes hands the keyboard to its place, not to another control', async () => {
    function Sender() {
      const [sending, setSending] = useState(false)
      return (
        <>
          <div className="tx-actions">
            <button type="button" className="tx" disabled={sending} onClick={async () => {
              if (await confirmDialog({ title: 'Send?', confirmLabel: 'Send' })) setSending(true)
            }}>Send now</button>
            <button type="button" className="tx">Send later</button>
          </div>
          <ConfirmHost />
        </>
      )
    }
    render(<Sender />)
    await ask('Send now')
    fireEvent.click(screen.getByRole('button', { name: 'Send' }))
    await waitFor(() => expect((screen.getByRole('button', { name: 'Send now' }) as HTMLButtonElement).disabled).toBe(true))
    await waitFor(() => expect(document.activeElement?.className).toBe('tx-actions'))
  })

  it('a question that replaces an unanswered one goes back where the first was asked', async () => {
    render(<Asker />)
    const opener = await ask()
    act(() => void confirmDialog({ title: 'Second?' }))
    await screen.findByText('Second?')
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    await waitFor(() => expect(document.activeElement).toBe(opener))
  })
})
