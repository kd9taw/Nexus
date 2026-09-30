// @vitest-environment jsdom
// A saved-projects file Program could not read is kept aside (never saved over), and Program must
// SAY SO, naming where the file is — otherwise the channel list just reads as empty, and the
// operator has no way to know their lists still exist. The pair is asserted: the note appears when
// there is a notice AND stays away when there is none, or a test that always finds it would pass
// against a panel that shows the warning permanently.
import { describe, expect, it, vi, beforeEach } from 'vitest'
import { cleanup, render, screen } from '@testing-library/react'

const radioprogListProjects = vi.fn()
const radioprogFileNotice = vi.fn()

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  radioprogListProjects: () => radioprogListProjects(),
  radioprogFileNotice: () => radioprogFileNotice(),
}))

import { RadioProgView } from './RadioProgView'

const KEPT = '/home/op/.config/nexus/radioprog.unreadable-20260930-142233.json'

describe('a saved-projects file Program could not read', () => {
  beforeEach(() => {
    cleanup()
    radioprogListProjects.mockReset().mockResolvedValue([])
    radioprogFileNotice.mockReset()
    localStorage.clear()
  })

  it('is named on the panel, with where the kept file is', async () => {
    radioprogFileNotice.mockResolvedValue({ path: KEPT, keptInPlace: false })
    render(<RadioProgView myGrid="FN31" />)
    const note = await screen.findByText(/could not read your saved channel lists/i)
    expect(note.closest('.settings-note')?.textContent).toContain(KEPT)
    expect(note.closest('.settings-note')?.textContent).toMatch(/Nothing was deleted/)
  })

  it('says saving stops when the file could not be moved aside', async () => {
    radioprogFileNotice.mockResolvedValue({ path: '/home/op/.config/nexus/radioprog.json', keptInPlace: true })
    render(<RadioProgView myGrid="FN31" />)
    const note = await screen.findByText(/could not read your saved channel lists/i)
    expect(note.closest('.settings-note')?.textContent).toMatch(/will not save until it is moved or repaired/)
  })

  it('says nothing when the file was read', async () => {
    radioprogFileNotice.mockResolvedValue(null)
    render(<RadioProgView myGrid="FN31" />)
    await vi.waitFor(() => expect(radioprogFileNotice).toHaveBeenCalled())
    expect(screen.queryByText(/could not read your saved channel lists/i)).toBeNull()
  })
})
