// @vitest-environment jsdom
// THE MIC SOURCE PICKER — the transmit-path half of the PTT-row pills.
//
// Two things here are rulings rather than implementation detail, and both are tested as such:
// the rig's own RX codec is NOT LISTED at all (operator, 2026-08-22), and the default is the
// rig's own mic so that a station which configures nothing streams nothing from this computer.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react'

import { PttAudioPills, forbiddenMic, micSourceOf } from './PttAudioPills'
import type { AudioDevices, Settings } from '../types'

afterEach(cleanup)

function settingsWith(liveMicDevice: string, audioIn = 'USB Audio Device'): Settings {
  return {
    audioIn,
    activeRadio: 0,
    radios: [{ id: 0, name: 'FT-710', liveMicDevice }],
  } as unknown as Settings
}

const devices = {
  input: [{ name: 'USB Audio Device' }, { name: 'Headset' }, { name: 'Studio Mic' }],
  output: [],
} as unknown as AudioDevices

function renderPills(s: Settings, onMicSource = vi.fn()) {
  render(
    <PttAudioPills
      settings={s}
      devices={devices}
      micGain={0.5}
      onMicGain={() => {}}
      onOutput={() => {}}
      onVolume={() => {}}
      onMicSource={onMicSource}
    />,
  )
  return onMicSource
}

function openPicker() {
  fireEvent.contextMenu(screen.getByLabelText(/Microphone — click for gain/i))
}

describe('micSourceOf', () => {
  it('reads the ACTIVE radio profile, and defaults to the rig', () => {
    expect(micSourceOf(settingsWith(''))).toBe('rig')
    expect(micSourceOf(settingsWith('Headset'))).toEqual({ device: 'Headset' })
    // Control: whitespace is not a device name. Without the trim this reads as a device and
    // the loop would try to open "   ".
    expect(micSourceOf(settingsWith('   '))).toBe('rig')
  })

  it('reads the rig when the roster does not contain the active radio', () => {
    // The roster can change under a cockpit mid-render. "I do not know" must resolve to the
    // rig's own mic, which streams nothing, never to a device name.
    const s = { activeRadio: 7, radios: [{ id: 0, liveMicDevice: 'Headset' }] } as unknown as Settings
    expect(micSourceOf(s)).toBe('rig')
    expect(micSourceOf(null)).toBe('rig')
  })
})

describe('forbiddenMic', () => {
  it("matches the rig's own RX codec and nothing else", () => {
    const s = settingsWith('', 'USB Audio Device')
    expect(forbiddenMic(s, 'USB Audio Device')).toBe(true)
    expect(forbiddenMic(s, 'Headset')).toBe(false)
    // Control: with no RX device configured nothing is forbidden — otherwise an unconfigured
    // station would have every microphone filtered out and an empty picker.
    expect(forbiddenMic(settingsWith('', ''), 'Headset')).toBe(false)
    expect(forbiddenMic(settingsWith('', ''), '')).toBe(false)
  })
})

describe('the picker', () => {
  it("does not list the rig's own codec at all — not listed, not disabled", () => {
    renderPills(settingsWith(''))
    openPicker()
    // The control that makes this meaningful: the OTHER devices ARE listed, so an empty menu
    // cannot pass this test.
    expect(screen.getByRole('menuitemradio', { name: 'Headset' })).toBeTruthy()
    expect(screen.getByRole('menuitemradio', { name: 'Studio Mic' })).toBeTruthy()
    expect(screen.queryByRole('menuitemradio', { name: 'USB Audio Device' })).toBeNull()
  })

  it('offers the rig\'s own mic as the default, and marks it current', () => {
    renderPills(settingsWith(''))
    openPicker()
    const rig = screen.getByRole('menuitemradio', { name: /Rig's own mic/i })
    expect(rig.getAttribute('aria-checked')).toBe('true')
  })

  it('writes the chosen device, and writes "" for the rig', () => {
    const onMicSource = renderPills(settingsWith(''))
    openPicker()
    fireEvent.click(screen.getByRole('menuitemradio', { name: 'Headset' }))
    expect(onMicSource).toHaveBeenCalledWith('Headset')
  })

  it('going back to the rig clears the device rather than naming one', () => {
    const onMicSource = renderPills(settingsWith('Headset'))
    openPicker()
    fireEvent.click(screen.getByRole('menuitemradio', { name: /Rig's own mic/i }))
    expect(onMicSource).toHaveBeenCalledWith('')
  })

  // Scoped to the MIC pill: the output pill legitimately reads "RIG" too (its own default is
  // the radio's speaker), so an unscoped getByText finds two and proves nothing about either.
  const micPill = () => screen.getByLabelText(/Microphone — click for gain/i)

  it('the pill shows which mic is talking', () => {
    renderPills(settingsWith('Headset'))
    expect(within(micPill()).getByText('Headset')).toBeTruthy()
  })

  it('the pill says RIG when no live mic is chosen', () => {
    renderPills(settingsWith(''))
    expect(within(micPill()).getByText('RIG')).toBeTruthy()
  })
})
