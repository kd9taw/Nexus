// @vitest-environment jsdom
//
// #343: the TX meters' look, Bars or Needle — validated and clamped on load, Bars by default, and
// followed live by every meter in the window (and, through `storage`, in the others).
import { afterEach, describe, expect, it, vi } from 'vitest'
import { act, renderHook } from '@testing-library/react'
import { readTxMeterStyle, setTxMeterStyle, TX_METER_STYLE_KEY, useTxMeterStyle } from './txMeterStyle'

afterEach(() => {
  vi.restoreAllMocks()
  localStorage.clear()
})

describe('the stored look is clamped on load', () => {
  it('nothing stored is Bars, so nothing changes until Needle is picked', () => {
    expect(readTxMeterStyle()).toBe('bars')
  })

  it('Needle stored is Needle (the positive control)', () => {
    localStorage.setItem(TX_METER_STYLE_KEY, 'needle')
    expect(readTxMeterStyle()).toBe('needle')
  })

  it.each(['NEEDLE', 'needles', ' needle', 'analog', '1', '', 'null', '{"style":"needle"}'])(
    'anything else stored (%j) reads as Bars',
    (v) => {
      localStorage.setItem(TX_METER_STYLE_KEY, v)
      expect(readTxMeterStyle()).toBe('bars')
    },
  )

  it('storage that throws reads as Bars', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('denied')
    })
    expect(readTxMeterStyle()).toBe('bars')
  })
})

describe('a pick reaches the meters', () => {
  it('a pick in this window re-renders the meters here and is remembered', () => {
    const { result } = renderHook(() => useTxMeterStyle())
    expect(result.current).toBe('bars')
    act(() => setTxMeterStyle('needle'))
    expect(result.current).toBe('needle')
    expect(localStorage.getItem(TX_METER_STYLE_KEY)).toBe('needle')
    act(() => setTxMeterStyle('bars'))
    expect(result.current).toBe('bars')
  })

  it('a pick in another window arrives through the storage event', () => {
    const { result } = renderHook(() => useTxMeterStyle())
    localStorage.setItem(TX_METER_STYLE_KEY, 'needle')
    act(() => {
      window.dispatchEvent(new StorageEvent('storage', { key: TX_METER_STYLE_KEY }))
    })
    expect(result.current).toBe('needle')
  })
})
