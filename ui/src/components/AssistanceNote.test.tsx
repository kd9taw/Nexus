// @vitest-environment jsdom
// This component makes CONTEST RULES claims, and an operator may pick a category on the
// strength of them. These tests are less about rendering than about pinning the claims to what
// the primary sources actually say, so a later edit cannot quietly invent a rule. Sources are
// cited in AssistanceNote.tsx's header; both were fetched and read 2026-07-30.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { AssistanceNote } from './AssistanceNote'

afterEach(cleanup)

describe('AssistanceNote', () => {
  it('states the current posture plainly in both directions', () => {
    const off = render(<AssistanceNote unassisted={false} />)
    expect(off.container.querySelector('.assist-note-state')?.textContent).toBe('ASSISTED')
    off.unmount()
    const on = render(<AssistanceNote unassisted />)
    expect(on.container.querySelector('.assist-note-state')?.textContent).toBe('UNASSISTED')
  })

  it('names every suppressed source, so the switch cannot over-claim', () => {
    const { container } = render(<AssistanceNote unassisted />)
    const line = container.querySelector('.assist-note-line')?.textContent ?? ''
    for (const src of ['AI CW decoder', 'DX cluster / RBN', 'PSK Reporter']) {
      expect(line).toContain(src)
    }
    // It must NOT claim that ALL assistance is off: POTA/SOTA activator spots keep arriving,
    // and a universal claim would be false. The gap is named in the details instead.
    expect(line).not.toMatch(/no .*assistance is running/i)
  })

  it('admits the gap it does not close, rather than implying there is none', () => {
    const { container } = render(<AssistanceNote unassisted />)
    const why = container.querySelector('.assist-note-why')?.textContent ?? ''
    expect(why).toMatch(/does not cover/i)
    expect(why).toContain('POTA')
  })

  it('cites CQ WW by its real category name and rule number', () => {
    const { container } = render(<AssistanceNote unassisted={false} />)
    const why = container.querySelector('.assist-note-why')?.textContent ?? ''
    // CQ WW V.A.2 is "Single Operator Assisted", defined by rule VIII.2.
    expect(why).toContain('VIII.2')
    expect(why).toContain('Single Operator Assisted')
    expect(why).toContain('Reverse Beacon Network')
  })

  it('cites ARRL by its real category name and rule number', () => {
    const { container } = render(<AssistanceNote unassisted={false} />)
    const why = container.querySelector('.assist-note-why')?.textContent ?? ''
    // ARRL's assisted class is "Single Operator Unlimited" (HCAT.2), NOT "Assisted".
    expect(why).toContain('Single Operator Unlimited')
    expect(why).toContain('HCAT.2')
    // ARRL names PSKReporter explicitly — the source the CW spec missed.
    expect(why).toContain('PSKReporter')
  })

  it('does not claim ARRL bans a single-signal CW decoder', () => {
    // The rulesets genuinely differ here. ARRL's glossary names automated MULTI-channel
    // decoders and defines them as software showing several decoded signals at once; Nexus
    // decodes one. Flattening that into "ARRL bans CW decoders" would be a false claim.
    const { container } = render(<AssistanceNote unassisted={false} />)
    const why = container.querySelector('.assist-note-why')?.textContent ?? ''
    expect(why).toContain('multi-channel')
    expect(why).toContain('one signal at a time')
  })

  it('tells the operator to check their own contest, and does not rule on a category', () => {
    const { container } = render(<AssistanceNote unassisted={false} />)
    const why = container.querySelector('.assist-note-why')?.textContent ?? ''
    expect(why).toMatch(/check the rules of the contest you are entering/i)
    expect(why).toMatch(/not a category ruling/i)
  })

  it('promises the operator their own settings survive', () => {
    const { container } = render(<AssistanceNote unassisted />)
    expect(container.querySelector('.assist-note-keep')?.textContent).toMatch(
      /settings are never rewritten/i,
    )
  })

  it('offers the switch only when a handler is given, and reports the new state', () => {
    const onToggle = vi.fn()
    const { container, unmount } = render(<AssistanceNote unassisted={false} />)
    expect(container.querySelector('.assist-note-btn')).toBeNull()
    unmount()

    render(<AssistanceNote unassisted={false} onToggle={onToggle} />)
    fireEvent.click(screen.getByRole('button', { name: /declare unassisted entry/i }))
    expect(onToggle).toHaveBeenCalledWith(true)
  })

  it('stamps when the current posture began, in UTC', () => {
    // 1753970520 = 2026-07-31T14:02:00Z
    render(<AssistanceNote unassisted sinceUnix={1785506520} />)
    expect(screen.getByText(/since \d\d:\d\dZ/)).toBeTruthy()
  })
})
