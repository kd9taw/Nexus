// @vitest-environment jsdom
//
// The launch picker's timeout: nobody answering for `autoSeconds` repeats the last answer, once, with
// the countdown on screen; any click or key press cancels it (someone is here to choose); an answer
// the backend refused (the radio is open in another window) is said in the picker.
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { RadioPicker } from './RadioPicker'
import type { RadioLaunchInfo } from '../api'
import { t } from '../i18n'

const info = (autoChoice: 'windows' | 'one' = 'windows'): RadioLaunchInfo => ({
  showPicker: true,
  radios: [
    { id: 0, name: 'HF', inUse: false },
    { id: 1, name: 'VHF', inUse: false },
  ],
  autoChoice,
  autoSeconds: 30,
})

beforeEach(() => {
  vi.useFakeTimers()
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

/** Let `seconds` pass one tick at a time, so each second's render schedules the next. */
function pass(seconds: number) {
  for (let i = 0; i < seconds; i++) {
    act(() => {
      vi.advanceTimersByTime(1_000)
    })
  }
}

function picker(i: RadioLaunchInfo, error: string | null = null) {
  const onTimeout = vi.fn()
  const onChoose = vi.fn()
  render(
    <RadioPicker info={i} onChoose={onChoose} onSingleRadio={() => {}} onTimeout={onTimeout} error={error} />,
  )
  return { onTimeout, onChoose }
}

it('repeats the last answer once nobody answers for 30 s, counting down on screen', () => {
  const { onTimeout } = picker(info('windows'))
  expect(screen.getByRole('status').textContent).toBe(t('radios.picker.auto.windows', { seconds: '30' }))
  pass(29)
  expect(screen.getByRole('status').textContent).toBe(t('radios.picker.auto.windows', { seconds: '1' }))
  expect(onTimeout).not.toHaveBeenCalled()
  pass(1)
  expect(onTimeout).toHaveBeenCalledTimes(1)
  pass(60)
  expect(onTimeout).toHaveBeenCalledTimes(1)
  expect(screen.queryByRole('status')).toBeNull()
})

it('says what one radio means when that is the answer it would repeat', () => {
  picker(info('one'))
  expect(screen.getByRole('status').textContent).toBe(t('radios.picker.auto.one', { seconds: '30' }))
})

it('a key press cancels the countdown: someone is here to choose', () => {
  const { onTimeout } = picker(info())
  pass(5)
  fireEvent.keyDown(window, { key: 'ArrowDown' })
  pass(60)
  expect(onTimeout).not.toHaveBeenCalled()
  expect(screen.queryByRole('status')).toBeNull()
})

it('a click cancels it too, and the click still chooses', () => {
  const { onTimeout, onChoose } = picker(info())
  const hf = screen.getByText('HF')
  fireEvent.pointerDown(hf)
  fireEvent.click(hf)
  pass(60)
  expect(onTimeout).not.toHaveBeenCalled()
  expect(onChoose).toHaveBeenCalledWith(0)
})

it('says why an answer was refused, in the operator\'s words', () => {
  picker(info(), 'That radio is open in another Nexus window — switch to that window.')
  expect(screen.getByRole('alert').textContent).toBe(t('radios.openElsewhere'))
})

it('CONTROL: any other refusal is said with the backend\'s reason', () => {
  picker(info(), 'the radio window could not start')
  expect(screen.getByRole('alert').textContent).toBe(
    t('radios.picker.failed', { reason: 'the radio window could not start' }),
  )
})
