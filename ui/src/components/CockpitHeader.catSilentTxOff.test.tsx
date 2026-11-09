// @vitest-environment jsdom
//
// WHAT AN OPERATOR READS ON THE FT SCREEN when a silent CAT link has turned transmit off: the CAT
// mark in the header carries the status, in their language, as the backend's snapshot sends it.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { CockpitHeader } from './CockpitHeader'
import { EN, installCatalog, setLocale } from '../i18n'
import { DE } from '../i18n/de'
import type { AppSnapshot } from '../types'

vi.mock('../api', () => ({ setFrequency: vi.fn(() => Promise.resolve(null)) }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))
vi.mock('../useWheelTune', () => ({ useWheelTune: () => undefined }))

const SENT = `14.0740 MHz CAT link unreachable (1/3) ${EN['settings.rigControl.catSilentTxOff']}`
const snap = {
  radio: {
    dialMhz: 14.074, catOk: false, catDetail: SENT, sideband: 'USB', transmitting: false,
    txEnabled: false, tuning: false, txAllowed: true,
  },
} as unknown as AppSnapshot

const reads = () => {
  render(<CockpitHeader snap={snap} modeIndicator={<span>FT8</span>} bandControl={<span>20m</span>} />)
  return document.querySelector('.cockpit-cat')?.getAttribute('title')
}

afterEach(() => {
  cleanup()
  setLocale('en')
})

describe('the CAT mark when a silent link has turned transmit off', () => {
  it('says so and how to transmit again', () => {
    expect(reads()).toBe(SENT)
  })

  it('says it in the operator\'s language', () => {
    installCatalog('de', DE)
    setLocale('de')
    expect(reads()).toBe(`14.0740 MHz CAT link unreachable (1/3) ${String(DE['settings.rigControl.catSilentTxOff'])}`)
  })
})
