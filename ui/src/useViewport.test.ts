// @vitest-environment jsdom
import { afterEach, describe, it, expect } from 'vitest'
import { renderHook } from '@testing-library/react'
import { classifyViewport, useViewport } from './useViewport'

describe('classifyViewport', () => {
  it('maps effective width to the right size class at each boundary', () => {
    expect(classifyViewport(320)).toBe('xs')
    expect(classifyViewport(767)).toBe('xs')
    expect(classifyViewport(768)).toBe('sm')
    expect(classifyViewport(1099)).toBe('sm')
    expect(classifyViewport(1100)).toBe('md')
    expect(classifyViewport(1599)).toBe('md')
    expect(classifyViewport(1600)).toBe('lg')
    expect(classifyViewport(2399)).toBe('lg')
    expect(classifyViewport(2400)).toBe('xl')
    expect(classifyViewport(3840)).toBe('xl')
  })
})

// The visible area (the Remote pages' opt-in). A phone's visual viewport as Chrome reports it: the area
// on screen in CSS pixels, and its scale. The on-screen keyboard shrinks the area at scale 1; a pinch
// shrinks it by the scale, and Chrome 140 measured a 5x pinch as 82 x 183 of a 412 x 915 phone.
describe('useViewport, the visible area', () => {
  type Area = EventTarget & { width: number; height: number; scale: number }
  const set = (area: Area, next: { width: number; height: number; scale: number }) => { Object.assign(area, next); area.dispatchEvent(new Event('resize')) }
  const frame = () => new Promise(resolve => requestAnimationFrame(resolve))
  const effective = () => {
    const style = document.documentElement.style
    return { width: parseFloat(style.getPropertyValue('--vw-eff')), height: parseFloat(style.getPropertyValue('--vh-eff')) }
  }
  // A fresh area each test, so nothing a previous test left listening can answer for this one.
  const phone = (): Area => {
    const area = Object.assign(new EventTarget(), { width: 412, height: 915, scale: 1 })
    Object.defineProperty(window, 'visualViewport', { configurable: true, value: area })
    Object.defineProperty(window, 'innerWidth', { configurable: true, value: 412 })
    Object.defineProperty(window, 'innerHeight', { configurable: true, value: 915 })
    return area
  }
  afterEach(() => { delete (window as { visualViewport?: unknown }).visualViewport })

  it('follows the on-screen keyboard and never a pinch, which once shrank the stream page into a corner of the zoomed view', async () => {
    const area = phone()
    renderHook(() => useViewport(1, true))
    await frame()
    expect(effective()).toEqual({ width: 412, height: 915 })
    // The keyboard: the area above it, at the page's own scale.
    set(area, { width: 412, height: 500, scale: 1 })
    await frame()
    expect(effective(), 'the keyboard').toEqual({ width: 412, height: 500 })
    // A 5x pinch: the page keeps its size, and the browser magnifies it.
    set(area, { width: 82.4, height: 183, scale: 5 })
    await frame()
    expect(effective().width, 'pinched').toBeCloseTo(412, 6)
    expect(effective().height, 'pinched').toBeCloseTo(915, 6)
    // Pinched with the keyboard up: still the area above the keyboard.
    set(area, { width: 82.4, height: 100, scale: 5 })
    await frame()
    expect(effective().height, 'pinched, keyboard up').toBeCloseTo(500, 6)
    // And back at scale 1 with the keyboard gone, exactly as it was.
    set(area, { width: 412, height: 915, scale: 1 })
    await frame()
    expect(effective()).toEqual({ width: 412, height: 915 })
  })

  it('CONTROL: the desktop views never read the visual viewport, pinched or not', async () => {
    const area = phone()
    renderHook(() => useViewport(1))
    set(area, { width: 82.4, height: 100, scale: 5 })
    window.dispatchEvent(new Event('resize'))
    await frame()
    expect(effective()).toEqual({ width: 412, height: 915 })
  })
})
