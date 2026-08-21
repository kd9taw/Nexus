// @vitest-environment jsdom
import { afterEach, describe, expect, it } from 'vitest'
import { cleanup, render, screen } from '@testing-library/react'
import { SetupHealth } from './SetupHealth'

afterEach(cleanup)

const radio = (rxLevel: number) => ({
  catOk: true,
  catDetail: 'CAT ok',
  rxLevel,
  audioError: null,
  txEnabled: false,
})

describe('SetupHealth RX-audio dot', () => {
  // Mac QA audit merged[46]: the old `rxDb > -60` was a dBFS threshold tested against
  // rxLevelDb's WSJT-X-style 0..90 scale — every representable value passed it, so with a
  // radio present and no audioError the dot was ALWAYS green, even on a stone-dead capture.
  // (Shipped in every 1.5.0–1.6.1 macOS DMG, where a denied mic delivers exactly this.)
  it('a dead-silent capture (rxLevel 0) must say "No RX audio"', () => {
    render(<SetupHealth radio={radio(0)} catResult={null} />)
    expect(screen.getByTitle(/No RX audio/)).toBeTruthy()
  })

  it('a live level reads "Receiving audio"', () => {
    // 0.01 RMS ≈ 50 dB on the meter's scale — a healthy FT8-ish capture level.
    render(<SetupHealth radio={radio(0.01)} catResult={null} />)
    expect(screen.getByTitle('Receiving audio')).toBeTruthy()
  })

  it('an audio error outranks the level', () => {
    render(
      <SetupHealth
        radio={{ ...radio(0.01), audioError: 'Sound card stopped' }}
        catResult={null}
      />,
    )
    expect(screen.getByTitle('Sound card stopped')).toBeTruthy()
  })
})
