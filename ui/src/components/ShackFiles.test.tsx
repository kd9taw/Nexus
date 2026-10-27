// @vitest-environment jsdom
// THE STATION'S FILES FROM AFAR — by value. On a computer that drives the station (client mode)
// a gallery picture is the station's bytes, asked of the station, never this computer's disk read
// at the station's path; and a station file saved here is the station's bytes, written piece by
// piece at the right places into this computer's Downloads folder. At the station nothing changes:
// the picture still comes over the asset protocol and nothing is asked of the link.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import * as api from '../api'
import { EN } from '../i18n/en'
import { setRemoteStation } from '../features/durableStore'
import { saveStationFile } from '../features/shackFiles'
import { GalleryThumb } from './SstvView'
import { ShackFilesSave } from './ShackFilesSave'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../api')>()
  return {
    ...actual,
    shackGalleryImage: vi.fn(),
    shackFileList: vi.fn(),
    shackFileRead: vi.fn(),
    shackFileSave: vi.fn(),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => {
    try {
      return await action()
    } catch {
      return null
    }
  }),
}))

const shackGalleryImage = api.shackGalleryImage as unknown as ReturnType<typeof vi.fn>
const shackFileList = api.shackFileList as unknown as ReturnType<typeof vi.fn>
const shackFileRead = api.shackFileRead as unknown as ReturnType<typeof vi.fn>
const shackFileSave = api.shackFileSave as unknown as ReturnType<typeof vi.fn>

const b64 = (bytes: Uint8Array) => btoa(String.fromCharCode(...bytes))
const fromB64 = (s: string) => Uint8Array.from(atob(s), (c) => c.charCodeAt(0))

/** A station file of `n` bytes, served in pieces of `piece` as `shack_file_read` serves them. */
function station(n: number, piece: number) {
  const file = Uint8Array.from({ length: n }, (_, i) => (i * 7 + 3) & 0xff)
  shackFileRead.mockImplementation(async (_folder: string, _name: string, offset: number) => ({
    data: b64(file.slice(offset, offset + piece)),
    offset,
    size: file.length,
  }))
  return file
}

/** What `shack_file_save` was handed, put back together the way the Rust side writes it. */
function savedHere(): Uint8Array {
  const out: number[] = []
  for (const [, data, offset] of shackFileSave.mock.calls as [string, string, number][]) {
    expect(offset).toBe(out.length)
    out.push(...fromB64(data))
  }
  return Uint8Array.from(out)
}

const ENTRY = {
  path: 'C:\\Users\\op\\Pictures\\Nexus SSTV\\20261017_120000_PD120.png',
  mode: 'PD-120',
  finishedUtc: '2026-10-17T12:00:00Z',
  freqMhz: 14.23,
  lines: 496,
  fskId: null,
}

const created: Blob[] = []
beforeEach(() => {
  vi.clearAllMocks()
  created.length = 0
  shackFileSave.mockImplementation(async (name: string) => ({ name, path: `/home/op/Downloads/${name}` }))
  URL.createObjectURL = vi.fn((blob: Blob) => {
    created.push(blob)
    return `blob:picture-${created.length}`
  }) as unknown as typeof URL.createObjectURL
  URL.revokeObjectURL = vi.fn()
  ;(window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {
    convertFileSrc: (p: string) => `asset://localhost/${encodeURIComponent(p)}`,
  }
})
afterEach(() => {
  cleanup()
  setRemoteStation(false)
  delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
})

describe('a gallery picture from afar', () => {
  it('in client mode is the station’s bytes, asked of the station by its gallery path', async () => {
    setRemoteStation(true)
    const picture = Uint8Array.from({ length: 2000 }, (_, i) => (i * 31) & 0xff)
    shackGalleryImage.mockResolvedValue(b64(picture))
    const { container } = render(<GalleryThumb entry={ENTRY} />)
    await waitFor(() => expect(container.querySelector('img')?.getAttribute('src')).toBe('blob:picture-1'))
    expect(shackGalleryImage).toHaveBeenCalledWith(ENTRY.path)
    expect(created[0].type).toBe('image/png')
    expect(new Uint8Array(await created[0].arrayBuffer())).toEqual(picture)
  })

  it('at the station is the asset protocol’s, and nothing is asked of the link (CONTROL)', () => {
    const { container } = render(<GalleryThumb entry={ENTRY} />)
    expect(container.querySelector('img')?.getAttribute('src')).toBe(
      `asset://localhost/${encodeURIComponent(ENTRY.path)}`,
    )
    expect(shackGalleryImage).not.toHaveBeenCalled()
  })
})

describe('a station file saved on this computer', () => {
  it('arrives whole, each piece written where the last ended', async () => {
    const file = station(2500, 1000)
    const progress: number[] = []
    const path = await saveStationFile('recordings', 'qso-KD9TAW-1.wav', (d) => progress.push(d))
    expect(path).toBe('/home/op/Downloads/qso-KD9TAW-1.wav')
    expect(shackFileRead.mock.calls.map((c) => c[2])).toEqual([0, 1000, 2000])
    expect(savedHere()).toEqual(file)
    expect(progress).toEqual([0.4, 0.8, 1])
  })

  it('never over a file already in Downloads: every piece goes to the free name the first one took', async () => {
    const file = station(2500, 1000)
    shackFileSave.mockImplementation(async (name: string, _data: string, offset: number) => {
      const taken = offset === 0 ? 'qso-KD9TAW-1 (1).wav' : name
      return { name: taken, path: `/home/op/Downloads/${taken}` }
    })
    const path = await saveStationFile('recordings', 'qso-KD9TAW-1.wav')
    expect(shackFileSave.mock.calls.map((c) => c[0])).toEqual([
      'qso-KD9TAW-1.wav',
      'qso-KD9TAW-1 (1).wav',
      'qso-KD9TAW-1 (1).wav',
    ])
    expect(savedHere()).toEqual(file)
    expect(path).toBe('/home/op/Downloads/qso-KD9TAW-1 (1).wav')
  })

  it('only a recording, an SSTV picture or ALL.TXT: any other name is refused before a byte is read', async () => {
    station(1500, 1000)
    const names = ['setup.exe', 'notes', 'all.txt', 'qso-KD9TAW-1.wav.exe', 'gallery.json']
    const outcomes: string[] = []
    for (const name of names) {
      outcomes.push(await saveStationFile('recordings', name).then(() => 'saved', (e: Error) => e.message))
    }
    expect(shackFileRead).not.toHaveBeenCalled()
    expect(shackFileSave).not.toHaveBeenCalled()
    expect(outcomes).toEqual(names.map((name) => EN['remote.stationFiles.save.notShared'].replace('{{name}}', name)))

    // CONTROL: each kind the station shares is saved, whatever the case of its extension.
    const kinds = [
      ['recordings', 'QSO-W1AW-2.WAV'],
      ['sstv', '20261017_120000_PD120.png'],
      ['sstv', '20261017_120000_Robot36.BMP'],
      ['allTxt', 'ALL.TXT'],
    ] as const
    for (const [folder, name] of kinds) await saveStationFile(folder, name)
    expect(shackFileSave.mock.calls.filter((c) => c[2] === 0).map((c) => c[0])).toEqual(kinds.map(([, name]) => name))
  })

  it('an empty file is saved empty, in one call', async () => {
    station(0, 1000)
    await saveStationFile('allTxt', 'ALL.TXT')
    expect(shackFileSave.mock.calls).toEqual([['ALL.TXT', '', 0]])
  })

  it('ALL.TXT saves from its button; the folders list the station’s files, each with its Save', async () => {
    setRemoteStation(true)
    const file = station(1500, 1000)
    render(<ShackFilesSave folder="allTxt" className="settings-linkbtn" />)
    fireEvent.click(screen.getByRole('button', { name: EN['remote.stationFiles.save.label'] }))
    await waitFor(() => expect(shackFileSave).toHaveBeenCalledTimes(2))
    expect(shackFileRead.mock.calls[0].slice(0, 2)).toEqual(['allTxt', 'ALL.TXT'])
    expect(savedHere()).toEqual(file)
    cleanup()
    vi.clearAllMocks()

    shackFileList.mockResolvedValue([
      { name: 'qso-W1AW-2.wav', bytes: 1500, modifiedMs: 2 },
      { name: 'qso-KD9TAW-1.wav', bytes: 90, modifiedMs: 1 },
    ])
    render(<ShackFilesSave folder="recordings" className="settings-linkbtn" />)
    fireEvent.click(screen.getByRole('button', { name: EN['remote.stationFiles.list.label'] }))
    const one = await screen.findByRole('button', {
      name: EN['remote.stationFiles.save.one'].replace('{{name}}', 'qso-W1AW-2.wav'),
    })
    expect(shackFileList).toHaveBeenCalledWith('recordings')
    fireEvent.click(one)
    await waitFor(() => expect(shackFileSave).toHaveBeenCalledTimes(2))
    expect(shackFileRead.mock.calls[0].slice(0, 2)).toEqual(['recordings', 'qso-W1AW-2.wav'])
    expect(savedHere()).toEqual(file)
  })
})
