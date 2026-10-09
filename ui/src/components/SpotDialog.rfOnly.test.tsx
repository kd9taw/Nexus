// @vitest-environment jsdom
//
// While a contest that allows spotting only over amateur RF runs (Winter Field Day 2027: "You may
// spot yourself and others only via amateur RF."), the station refuses a DX cluster spot with a
// token. The dialog turns it into the sentence that says why; any other refusal is still shown as
// the station worded it.
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { SpotDialog } from './SpotDialog'
import { postSpot } from '../api'
import { pushToast } from '../toast'
import { t } from '../i18n'

vi.mock('../api', () => ({ postSpot: vi.fn() }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

beforeEach(() => vi.clearAllMocks())
afterEach(() => cleanup())

const post = () => {
  render(<SpotDialog open onClose={() => {}} initialCall="JA2DEF" freqMhz={14.074} defaultComment="" />)
  fireEvent.click(screen.getByRole('button', { name: t('spots.post.submit') }))
}

it('says why when the station refuses a spot during a contest that allows spotting only over RF', async () => {
  vi.mocked(postSpot).mockRejectedValueOnce('spotRfOnly')
  post()
  await waitFor(() => expect(pushToast).toHaveBeenCalledWith(t('spots.post.rfOnly'), 'error', 6000))
})

it('CONTROL — another refusal is shown as the station worded it', async () => {
  vi.mocked(postSpot).mockRejectedValueOnce('no DX cluster connected — set a cluster host in Settings')
  post()
  await waitFor(() =>
    expect(pushToast).toHaveBeenCalledWith('no DX cluster connected — set a cluster host in Settings', 'error', 3500),
  )
})
