// @vitest-environment jsdom
//
// A ranged number box that keeps what is typed until the operator is done. A box that clamped
// each keystroke could not be typed to most values (a 300–1200 box turned the "6" of "650" into
// 300 at once). Typed ONE KEY AT A TIME onto what the box holds after React answered the last key.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { useState } from 'react'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { DraftNumberInput } from './DraftNumberInput'

afterEach(cleanup)

function Harness({ onValue, onBlur }: { onValue: (v: number) => void; onBlur?: () => void }) {
  const [v, setV] = useState(600)
  return (
    <DraftNumberInput
      aria-label="Pitch"
      min={300}
      max={1200}
      value={v}
      onValue={(n) => {
        onValue(n)
        setV(n)
      }}
      onBlur={onBlur}
    />
  )
}

function setup(onBlur?: () => void) {
  const onValue = vi.fn()
  render(<Harness onValue={onValue} onBlur={onBlur} />)
  return { box: screen.getByRole('spinbutton', { name: 'Pitch' }) as HTMLInputElement, onValue }
}

function typeOver(input: HTMLInputElement, text: string) {
  fireEvent.change(input, { target: { value: '' } })
  for (const ch of text) fireEvent.change(input, { target: { value: input.value + ch } })
}

describe('DraftNumberInput', () => {
  it('keeps every typed key and hands on only the in-range value', () => {
    const { box, onValue } = setup()
    typeOver(box, '650')
    expect(box.value).toBe('650')
    expect(onValue.mock.calls).toEqual([[650]])
  })

  it('clamps on blur and on Enter, never while typing', () => {
    const { box, onValue } = setup()
    typeOver(box, '5')
    expect(box.value).toBe('5')
    expect(onValue).not.toHaveBeenCalled()
    fireEvent.blur(box)
    expect(box.value).toBe('300')
    typeOver(box, '5000')
    fireEvent.keyDown(box, { key: 'Enter' })
    expect(box.value).toBe('1200')
    // "500" on the way to "5000" is itself a value in range, so it went out as it was typed.
    expect(onValue.mock.calls).toEqual([[300], [500], [1200]])
  })

  it('a value already handed on is not sent again on blur', () => {
    const { box, onValue } = setup()
    typeOver(box, '700')
    fireEvent.blur(box)
    expect(onValue.mock.calls).toEqual([[700]])
  })

  it('blank or non-numeric text is dropped on blur; Escape drops the edit', () => {
    const { box, onValue } = setup()
    typeOver(box, '')
    fireEvent.blur(box)
    expect(box.value).toBe('600')
    typeOver(box, '9')
    fireEvent.keyDown(box, { key: 'Escape' })
    expect(box.value).toBe('600')
    expect(onValue).not.toHaveBeenCalled()
  })

  it("still runs the caller's own onBlur, after its commit", () => {
    const onBlur = vi.fn()
    const { box, onValue } = setup(onBlur)
    typeOver(box, '5')
    fireEvent.blur(box)
    expect(onValue.mock.calls).toEqual([[300]])
    expect(onBlur).toHaveBeenCalledTimes(1)
  })
})
