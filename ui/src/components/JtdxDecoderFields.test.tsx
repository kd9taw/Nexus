// @vitest-environment jsdom
import { describe, it, expect } from 'vitest'
import { useState } from 'react'
import { render, screen, fireEvent, within } from '@testing-library/react'
import { JtdxDecoderFields, PRESET_KNOBS, autoThreads } from './JtdxDecoderFields'
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

  it('picks the standard decoder, and then offers no preset', () => {
    const seen: Settings[] = []
    render(<Harness start={{}} seen={seen} cores={8} />)
    fireEvent.click(screen.getByRole('button', { name: 'Standard (WSJT-X)' }))
    expect(last(seen).ft8Decoder).toBe('standard')
    expect(screen.queryByRole('button', { name: 'Ludicrous' })).toBeNull()
  })

  it('warns, plainly, how many cores Ludicrous may take against what this computer has', () => {
    const seen: Settings[] = []
    const { container } = render(<Harness start={{}} seen={seen} cores={4} />)
    expect(container.querySelector('[data-ludicrous-warning]')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Ludicrous' }))
    expect(last(seen).jtdxPreset).toBe('ludicrous')
    expect(container.querySelector('[data-ludicrous-warning]')?.textContent).toBe(
      'Ludicrous decodes on up to 12 cores; this computer has 4. On a Raspberry Pi or a small laptop, decodes may arrive too late to answer.',
    )
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
