// @vitest-environment jsdom
//
// The Band Advisor says its words come from the simple model, while Band Outlook runs P.533, so the
// two can be told apart rather than read as one engine disagreeing with itself.
import { describe, it, expect, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { BandAdvisor } from './BandAdvisor'

afterEach(cleanup)

describe('the Band Advisor', () => {
  it('labels its model as the simple one for a station on P.533', () => {
    const { container } = render(<BandAdvisor simpleModelNote bands={[]} />)
    expect(container.querySelector('.ba-caption')!.textContent).toContain('the simple model, not P.533')
  })

  it('says nothing about P.533 to a station that chose the simple model', () => {
    const { container } = render(<BandAdvisor bands={[]} />)
    expect(container.querySelector('.ba-caption')!.textContent).not.toContain('P.533')
  })
})
