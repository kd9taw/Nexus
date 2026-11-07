// @vitest-environment jsdom
//
// The Band Advisor says its words come from the simple model, while Band Outlook runs P.533, so the
// two can be told apart rather than read as one engine disagreeing with itself.
import { describe, it, expect, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { BandAdvisor } from './BandAdvisor'

afterEach(cleanup)

describe('the Band Advisor', () => {
  it('labels its model as the simple one, in both views', () => {
    const { container } = render(<BandAdvisor bands={[]} />)
    expect(container.querySelector('.ba-caption')!.textContent).toContain('the simple model, not P.533')
  })
})
