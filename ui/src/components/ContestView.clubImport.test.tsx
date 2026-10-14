// @vitest-environment jsdom
//
// The host's "Import a laptop's file…" on its club block: a laptop's contest journal, whose name
// carries its position, is imported as that position's contacts, and what the host did is said on
// the block. A file whose name carries none waits for the host to pick a position from its board.
// Only on the host's own screen: not on Remote (read-only), not in the pop-out, and a press that
// comes through a stream imports nothing.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, act, fireEvent } from '@testing-library/react'
import { FdClubSection } from './ContestView'
import { StreamInputDispatcher } from '../remote-native/stream-input'
import type { FdClubStatus } from '../types'
import type { FdClubImport } from '../api'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', () => ({
  fdClubLogNewest: vi.fn(async () => null),
  openPanelWindow: vi.fn(async () => {}),
}))

const HOST: FdClubStatus = {
  syncState: 'synced',
  queued: 0,
  offlineSinceUnix: 0,
  hosting: true,
  event: 'W9XYZ ILQP',
  hostCall: 'W9XYZ',
  score: 0,
  qsos: 0,
  sections: 0,
  skewSecs: 0,
  dupes: [],
  board: [],
}
const LABEL = "Import a laptop's file…"
const RESULT: FdClubImport = {
  added: 2,
  dupes: 1,
  alreadyIn: 0,
  otherContest: 0,
  otherRunning: 0,
  otherCall: 1,
  takenBack: 0,
  unreadable: 0,
  refused: [],
}

afterEach(cleanup)

async function choose(name: string, text = '<CALL:5>K9AAA <EOR>') {
  const input = document.querySelector('input[data-club-import]') as HTMLInputElement
  await act(async () => {
    fireEvent.change(input, { target: { files: [new File([text], name)] } })
  })
}

describe('Import a laptop’s file', () => {
  it('imports a journal as the position its name carries, and says what it did', async () => {
    const onImport = vi.fn(async () => RESULT)
    render(<FdClubSection club={HOST} onImport={onImport} />)
    fireEvent.click(screen.getByRole('button', { name: LABEL }))
    await choose('fieldday_backup_bbbb0002.adi')
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Import' }))
    })
    expect(onImport).toHaveBeenCalledWith('<CALL:5>K9AAA <EOR>', 'bbbb0002')
    expect(screen.getByRole('status').textContent).toBe(
      'Imported fieldday_backup_bbbb0002.adi: added 2, duplicates 1, already in the club log 0.' +
        "Kept out: logged on another call than the club's 1. Nothing was taken out of the club log.",
    )
  })

  it('waits for a position when the file name carries none', async () => {
    const onImport = vi.fn(async () => RESULT)
    const club = {
      ...HOST,
      board: [
        {
          posid: 'cccc0003',
          posName: 'SSB tent',
          band: '20m',
          mode: 'PH',
          operator: 'W9XYZ',
          qsos: 3,
          rate: 0,
          lastSeenSecs: 2,
          link: 'connected' as const,
          heardSecs: 2,
        },
      ],
    }
    render(<FdClubSection club={club} onImport={onImport} />)
    await choose('my-log.adi')
    const yes = screen.getByRole('button', { name: 'Import' }) as HTMLButtonElement
    expect(yes.disabled).toBe(true)
    fireEvent.change(screen.getByRole('combobox'), { target: { value: 'cccc0003' } })
    await act(async () => {
      fireEvent.click(yes)
    })
    expect(onImport).toHaveBeenCalledWith('<CALL:5>K9AAA <EOR>', 'cccc0003')
  })

  it('is on the host’s own screen alone, and a stream’s press imports nothing', async () => {
    const onImport = vi.fn(async () => RESULT)
    const { unmount } = render(<FdClubSection club={HOST} onImport={onImport} readOnly />)
    expect(screen.queryByRole('button', { name: LABEL })).toBeNull()
    unmount()
    render(<FdClubSection club={HOST} onImport={onImport} detached />)
    expect(screen.queryByRole('button', { name: LABEL })).toBeNull()
    cleanup()
    render(<FdClubSection club={{ ...HOST, hosting: false }} onImport={onImport} />)
    expect(screen.queryByRole('button', { name: LABEL })).toBeNull()
    cleanup()
    // jsdom never lays out: `elementFromPoint` does not exist. The press says what is under it.
    let under: Element | null = null
    Object.defineProperty(document, 'elementFromPoint', { configurable: true, value: () => under })
    const stream = new StreamInputDispatcher(window)
    try {
      render(<FdClubSection club={HOST} onImport={onImport} />)
      const input = document.querySelector('input[data-club-import]') as HTMLInputElement
      const picked = vi.spyOn(input, 'click')
      under = screen.getByRole('button', { name: LABEL })
      const pointer = (action: 'down' | 'up') =>
        ({ type: 'pointer', action, x: 0.5, y: 0.5, button: 0, buttons: action === 'down' ? 1 : 0,
          modifiers: 0, pointerType: 'mouse', clicks: 1 }) as const
      act(() => {
        stream.handle(pointer('down'))
        stream.handle(pointer('up'))
      })
      await act(async () => {
        await Promise.resolve()
      })
      expect(picked).not.toHaveBeenCalled()
      expect(screen.getByRole('alert').textContent).toBe(
        "A laptop's file is imported on the host's own screen, not through Remote.",
      )
      // CONTROL: the same button pressed at the host opens the file chooser.
      fireEvent.click(screen.getByRole('button', { name: LABEL }))
      expect(picked).toHaveBeenCalledTimes(1)
    } finally {
      stream.dispose()
      delete (document as { elementFromPoint?: unknown }).elementFromPoint
    }
    expect(onImport).not.toHaveBeenCalled()
  })
})
