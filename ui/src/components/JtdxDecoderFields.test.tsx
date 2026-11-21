// @vitest-environment jsdom
import { describe, it, expect } from 'vitest'
import { useState } from 'react'
import { render, screen, fireEvent, within } from '@testing-library/react'
import { JTDX_MAX_THREADS, JtdxDecoderFields, PRESET_KNOBS, autoThreads, leaveACore } from './JtdxDecoderFields'
import type { Settings } from '../types'

/** The fields over a live form, the way SettingsPanel holds it; `seen` gets every form. */
function Harness({ start, seen, cores }: { start: Partial<Settings>; seen: Settings[]; cores: number }) {
  const [form, setForm] = useState<Settings | null>(start as Settings)
  if (form) seen.push(form)
  return form ? (
    <JtdxDecoderFields form={form} setForm={setForm} markDirty={() => {}} locked={() => false} cores={cores} />
  ) : null
}

const last = (seen: Settings[]) => seen[seen.length - 1]

describe('Settings ▸ FT8 decoder', () => {
  it('defaults to the JTDX-style decoder on Easy', () => {
    render(<Harness start={{}} seen={[]} cores={8} />)
    expect(screen.getByRole('button', { name: 'JTDX-style (multithreaded)' }).getAttribute('aria-pressed')).toBe('true')
    expect(screen.getByRole('button', { name: 'Easy' }).getAttribute('aria-pressed')).toBe('true')
  })

  it('picks the standard decoder, and offers no preset once FT4 is standard too', () => {
    const seen: Settings[] = []
    render(<Harness start={{}} seen={seen} cores={8} />)
    const ft8 = screen.getByRole('group', { name: 'FT8 decoder' })
    fireEvent.click(within(ft8).getByRole('button', { name: 'Standard (WSJT-X)' }))
    expect(last(seen).ft8Decoder).toBe('standard')
    // FT4's decoder is still JTDX-style, and the preset is still its.
    expect(screen.getByRole('button', { name: 'Ludicrous' })).toBeTruthy()
    const ft4 = screen.getByRole('group', { name: 'FT4 decoder' })
    fireEvent.click(within(ft4).getByRole('button', { name: 'Standard (WSJT-X)' }))
    expect(last(seen).ft4Decoder).toBe('standard')
    expect(screen.queryByRole('button', { name: 'Ludicrous' })).toBeNull()
  })

  it('warns, plainly, how many cores Ludicrous may take against what this computer has', () => {
    const seen: Settings[] = []
    const { container } = render(<Harness start={{}} seen={seen} cores={4} />)
    expect(container.querySelector('[data-ludicrous-warning]')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Ludicrous' }))
    expect(last(seen).jtdxPreset).toBe('ludicrous')
    expect(container.querySelector('[data-ludicrous-warning]')?.textContent).toBe(
      'Ludicrous decodes on up to 3 cores; this computer has 4. On a Raspberry Pi or a small laptop, decodes may arrive too late to answer.',
    )
  })

  it('counts Ludicrous’s cores as the decoder takes them: one under this computer’s, up to 12, never none', () => {
    // The decoder holds an explicit count one under the logical cores (ft8 `leave_a_core`).
    expect([1, 2, 4, 8, 12, 13, 32].map((c) => leaveACore(JTDX_MAX_THREADS, c))).toEqual([1, 1, 3, 7, 11, 12, 12])
    const { container } = render(<Harness start={{ jtdxPreset: 'ludicrous' }} seen={[]} cores={32} />)
    expect(container.querySelector('[data-ludicrous-warning]')?.textContent).toMatch(/^Ludicrous decodes on up to 12 cores; this computer has 32\./)
  })

  it('warns on Keen too, with the threads WSJT-X picks for this computer', () => {
    const { container } = render(<Harness start={{ jtdxPreset: 'keen' }} seen={[]} cores={4} />)
    expect(container.querySelector('[data-cpu-warning]')?.textContent).toBe(
      'Keen decodes on 3 cores (WSJT-X picks the count from this computer); this computer has 4. On a Raspberry Pi or a small laptop, decodes may arrive too late to answer.',
    )
    // WSJT-X's automatic count: 1 core 1; 2-4 one less; 5-8 two less; 9-15 three less; then 12.
    expect([1, 2, 4, 5, 8, 9, 15, 16, 32].map(autoThreads)).toEqual([1, 1, 3, 3, 6, 6, 12, 12, 12])
  })

  it('shows the CPU warning on Keen and Ludicrous and none on Easy', () => {
    for (const [preset, warns] of [['easy', false], ['keen', true], ['ludicrous', true]] as const) {
      const { container, unmount } = render(<Harness start={{ jtdxPreset: preset }} seen={[]} cores={8} />)
      expect(container.querySelector('[data-cpu-warning]') !== null, preset).toBe(warns)
      unmount()
    }
  })

  it('turns a knob under a preset into Custom, starting from what that preset runs', () => {
    const seen: Settings[] = []
    render(<Harness start={{ jtdxPreset: 'keen' }} seen={seen} cores={8} />)
    const cycles = screen.getByRole('group', { name: 'Decoding cycles' })
    fireEvent.click(within(cycles).getByRole('button', { name: '2' }))
    expect(last(seen).jtdxPreset).toBe('custom')
    expect(last(seen).jtdxCustom).toEqual({ ...PRESET_KNOBS.keen, cycles: 2 })
  })
})

describe('Settings ▸ FT4 decoder', () => {
  it('defaults to the JTDX-style decoder, sharing FT8\'s preset', () => {
    render(<Harness start={{}} seen={[]} cores={8} />)
    const ft4 = screen.getByRole('group', { name: 'FT4 decoder' })
    expect(within(ft4).getByRole('button', { name: 'JTDX-style' }).getAttribute('aria-pressed')).toBe('true')
  })

  it('keeps the preset on offer for FT4 alone when FT8 runs the standard decoder', () => {
    const seen: Settings[] = []
    render(<Harness start={{ ft8Decoder: 'standard' }} seen={seen} cores={8} />)
    fireEvent.click(screen.getByRole('button', { name: 'Ludicrous' }))
    expect(last(seen).jtdxPreset).toBe('ludicrous')
    expect(last(seen).ft8Decoder).toBe('standard')
    expect(last(seen).ft4Decoder ?? 'jtdx').toBe('jtdx')
  })
})
