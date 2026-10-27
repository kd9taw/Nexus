// @vitest-environment jsdom
//
// Settings ▸ Digital ▸ JS8's lists, marks and sounds (components/Js8ListsSettings): JS8Call's
// defaults shown for a form from before them, each list parsed as JS8Call reads it, the marks as
// typed, each sound on or off in JS8Call's table order with a Test that plays it and its WAV file chosen,
// shown and reset to the built-in tone, and the startup reset.
import { describe, expect, it, vi, afterEach } from 'vitest'
import { render, cleanup, fireEvent, screen, act } from '@testing-library/react'
import { Js8ListsSettings, parseJs8Words } from './Js8ListsSettings'
import type { Settings } from '../types'
import * as api from '../api'

vi.mock('../api', () => ({ playJs8SoundHere: vi.fn(async () => ({ fileRefused: null })) }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))
afterEach(cleanup)

const mount = (form: Partial<Settings>) => {
  const setList = vi.fn()
  const setMark = vi.fn()
  const setSounds = vi.fn()
  const setReset = vi.fn()
  render(<Js8ListsSettings form={form as Settings} setList={setList} setMark={setMark} setSounds={setSounds} setReset={setReset} />)
  return { setList, setMark, setSounds, setReset }
}

describe('the JS8 lists, marks and sounds in Settings', () => {
  it('shows JS8Call’s defaults for a form from before them: empty lists, ♢ and ……, every sound off', () => {
    mount({})
    expect((screen.getByLabelText('Hide these callsigns in the RX pane') as HTMLInputElement).value).toBe('')
    expect((screen.getByLabelText('End of transmission mark') as HTMLInputElement).value).toBe('♢')
    expect((screen.getByLabelText('Missing frame mark') as HTMLInputElement).value).toBe('……')
    const switches = screen.getAllByRole('switch')
    expect(switches, 'the startup reset and the seven sounds').toHaveLength(8)
    expect(switches.every((s) => s.getAttribute('aria-checked') === 'false')).toBe(true)
  })

  it('reads a list as JS8Call does — comma separated, trimmed, upper-cased — and a mark as typed', () => {
    expect(parseJs8Words(' w1aw, ,K1abc ')).toEqual(['W1AW', 'K1ABC'])
    const { setList, setMark } = mount({ js8RxBlocklist: ['N0BAD'], js8Eot: '♢' })
    fireEvent.change(screen.getByLabelText('Highlight these words or callsigns (primary colour)'), { target: { value: 'net, k1abc' } })
    expect(setList).toHaveBeenCalledWith('js8HighlightPrimary', ['NET', 'K1ABC'])
    fireEvent.change(screen.getByLabelText('End of transmission mark'), { target: { value: '#' } })
    expect(setMark).toHaveBeenCalledWith('js8Eot', '#')
  })

  it('turns a sound on in JS8Call’s table order, off again, and tests one', async () => {
    const { setSounds } = mount({ js8Sounds: ['inbox'] })
    fireEvent.click(screen.getByRole('switch', { name: 'A CQ' }))
    expect(setSounds).toHaveBeenLastCalledWith(['cq', 'inbox'])
    fireEvent.click(screen.getByRole('switch', { name: 'A message filed in your Inbox' }))
    expect(setSounds).toHaveBeenLastCalledWith([])
    fireEvent.click(screen.getByRole('button', { name: 'Test the sound for A heartbeat' }))
    expect(api.playJs8SoundHere).toHaveBeenLastCalledWith('hb', null)
  })

  // JS8Call's "Reset the Band Activity, Call Activity, and RX history at startup" (Configuration.ui:429-433,
  // off by default, Configuration.cpp:2303).
  it('offers JS8Call’s startup reset, off for a form from before it, and writes the switch as clicked', () => {
    const name = 'Reset Band activity, the call list and the RX pane at startup'
    const off = mount({})
    expect(screen.getByRole('switch', { name }).getAttribute('aria-checked')).toBe('false')
    fireEvent.click(screen.getByRole('switch', { name }))
    expect(off.setReset).toHaveBeenLastCalledWith(true)
    cleanup()
    const on = mount({ js8ResetActivity: true })
    expect(screen.getByRole('switch', { name }).getAttribute('aria-checked')).toBe('true')
    fireEvent.click(screen.getByRole('switch', { name }))
    expect(on.setReset).toHaveBeenLastCalledWith(false)
  })

  it('names each sound’s WAV file or the built-in tone; Choose… picks one, Reset to built-in clears it', async () => {
    const pickSoundFile = vi.fn(async () => '/home/op/sounds/hb.wav')
    const setSoundFile = vi.fn()
    render(
      <Js8ListsSettings
        form={{ js8Sounds: ['cq'], js8SoundFiles: { cq: '/home/op/sounds/cq.wav' } } as unknown as Settings}
        setList={vi.fn()}
        setMark={vi.fn()}
        setSounds={vi.fn()}
        setReset={vi.fn()}
        pickSoundFile={pickSoundFile}
        setSoundFile={setSoundFile}
      />,
    )
    const row = (name: string) => screen.getByRole('switch', { name }).closest('.js8-sound') as HTMLElement
    const named = (name: string) => row(name).querySelector<HTMLElement>('.js8-sound-file-name')
    expect(named('A CQ')?.textContent).toBe('cq.wav')
    expect(named('A CQ')?.title, 'the whole path on hover').toBe('/home/op/sounds/cq.wav')
    expect(named('A heartbeat')?.textContent).toBe('Built-in tone')
    expect(screen.queryByRole('button', { name: 'Play the built-in tone for A heartbeat' }), 'nothing to reset').toBeNull()
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Choose a WAV file for A heartbeat' }))
    })
    expect(pickSoundFile).toHaveBeenCalledTimes(1)
    expect(setSoundFile).toHaveBeenCalledWith('hb', '/home/op/sounds/hb.wav')
    pickSoundFile.mockResolvedValueOnce(null as unknown as string)
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Choose a WAV file for A heartbeat' }))
    })
    expect(setSoundFile, 'a cancelled picker changes nothing').toHaveBeenCalledTimes(1)
    fireEvent.click(screen.getByRole('button', { name: 'Play the built-in tone for A CQ' }))
    expect(setSoundFile).toHaveBeenLastCalledWith('cq', null)
  })

  it('Test plays the row’s file, as the form holds it before Save', async () => {
    render(
      <Js8ListsSettings
        form={{ js8SoundFiles: { ack: 'C:\\Sounds\\ack.wav' } } as unknown as Settings}
        setList={vi.fn()}
        setMark={vi.fn()}
        setSounds={vi.fn()}
        setReset={vi.fn()}
        pickSoundFile={vi.fn()}
        setSoundFile={vi.fn()}
      />,
    )
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Test the sound for An ACK for you' }))
    })
    expect(api.playJs8SoundHere).toHaveBeenCalledWith('ack', 'C:\\Sounds\\ack.wav')
  })

  it('picks the sounds’ output: the system default first, the radio’s TX audio devices shown but not choosable', () => {
    const setSoundDevice = vi.fn()
    render(
      <Js8ListsSettings
        form={{ audioOut: 'USB Audio CODEC', radios: [{ audioOut: 'Rig 2 CODEC' }] } as unknown as Settings}
        setList={vi.fn()}
        setMark={vi.fn()}
        setSounds={vi.fn()}
        setReset={vi.fn()}
        outputs={['Speakers', 'usb audio codec', 'Rig 2 CODEC']}
        setSoundDevice={setSoundDevice}
      />,
    )
    const select = screen.getByLabelText('Play the sounds on') as HTMLSelectElement
    expect(select.value, 'the system default until one is picked').toBe('')
    expect([...select.options].map((o) => [o.value, o.textContent, o.disabled])).toEqual([
      ['', 'System default', false],
      ['Speakers', 'Speakers', false],
      ['usb audio codec', 'usb audio codec (radio TX audio, not used for sounds)', true],
      ['Rig 2 CODEC', 'Rig 2 CODEC (radio TX audio, not used for sounds)', true],
    ])
    fireEvent.change(select, { target: { value: 'Speakers' } })
    expect(setSoundDevice).toHaveBeenLastCalledWith('Speakers')
  })
})
