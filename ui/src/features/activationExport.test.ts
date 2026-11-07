// The way from an exported activation to POTA's upload page. Nexus opens a page and shows a file:
// it holds no POTA login and asks POTA for nothing, so nothing comes back to say the log arrived,
// and no word here may claim it did. Each half can fail alone, and the operator still has to be
// told what to do next.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import * as api from '../api'
import { pushToast } from '../toast'
import { POTA_UPLOAD_URL, isPotaActivation, openPotaUpload } from './activationExport'

vi.mock('../api', () => ({
  openExternalUrl: vi.fn(async () => {}),
  revealInDownloads: vi.fn(async () => true),
}))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

const FILE = {
  filename: 'KD9TAW@US-1234-20260909.adi',
  path: '/home/op/Downloads/KD9TAW@US-1234-20260909.adi',
}

const said = () => vi.mocked(pushToast).mock.calls.map((c) => String(c[0]))

describe('openPotaUpload', () => {
  const fetchSpy = vi.fn()

  beforeEach(() => {
    vi.mocked(pushToast).mockClear()
    vi.mocked(api.openExternalUrl).mockReset().mockResolvedValue(undefined)
    vi.mocked(api.revealInDownloads).mockReset().mockResolvedValue(true)
    fetchSpy.mockClear()
    vi.stubGlobal('fetch', fetchSpy)
  })
  afterEach(() => vi.unstubAllGlobals())

  it("opens the page POTA's documentation names and shows the exported file", async () => {
    await openPotaUpload(FILE)
    expect(POTA_UPLOAD_URL).toBe('https://pota.app/#/user/logs')
    expect(api.openExternalUrl).toHaveBeenCalledWith(POTA_UPLOAD_URL)
    expect(api.revealInDownloads).toHaveBeenCalledWith(FILE.filename)
  })

  it('makes no request of its own: the browser is the only thing that reaches POTA', async () => {
    await openPotaUpload(FILE)
    expect(fetchSpy).not.toHaveBeenCalled()
  })

  it('says the next step is logging in and dropping the file, and never that it went', async () => {
    await openPotaUpload(FILE)
    expect(said()).toHaveLength(1)
    expect(said()[0]).toContain(FILE.filename)
    expect(said()[0]).toMatch(/log in/i)
    expect(said()[0]).toMatch(/drop/i)
    expect(said()[0]).toMatch(/nothing has gone to POTA/i)
    expect(said()[0]).not.toMatch(/uploaded|submitted/i)
  })

  it('with no browser, gives the address to type and still shows the file', async () => {
    vi.mocked(api.openExternalUrl).mockRejectedValue(new Error('no browser'))
    await openPotaUpload(FILE)
    expect(api.revealInDownloads).toHaveBeenCalledWith(FILE.filename)
    expect(said()).toHaveLength(1)
    expect(said()[0]).toContain(POTA_UPLOAD_URL)
    expect(said()[0]).toContain(FILE.filename)
    expect(vi.mocked(pushToast).mock.calls[0][1]).toBe('error')
  })

  it('a file moved away is named, and the page still opens for a copy kept elsewhere', async () => {
    vi.mocked(api.revealInDownloads).mockResolvedValue(false)
    await openPotaUpload(FILE)
    expect(api.openExternalUrl).toHaveBeenCalled()
    expect(said()).toHaveLength(1)
    expect(said()[0]).toContain(FILE.filename)
    expect(said()[0]).toMatch(/moved/i)
    expect(said()[0]).toMatch(/export activation/i)
    expect(vi.mocked(pushToast).mock.calls[0][1]).toBe('error')
  })

  it('with no file manager, says where the file is', async () => {
    vi.mocked(api.revealInDownloads).mockRejectedValue(new Error('no file manager'))
    await openPotaUpload(FILE)
    expect(said()).toHaveLength(1)
    expect(said()[0]).toContain(FILE.path)
    expect(said()[0]).toMatch(/log in/i)
    expect(vi.mocked(pushToast).mock.calls[0][1]).toBe('error')
  })

  it('when both fail, says both and never rejects', async () => {
    vi.mocked(api.openExternalUrl).mockRejectedValue(new Error('no browser'))
    vi.mocked(api.revealInDownloads).mockRejectedValue(new Error('no file manager'))
    await expect(openPotaUpload(FILE)).resolves.toBeUndefined()
    expect(said()).toHaveLength(2)
    expect(said().join(' ')).toContain(POTA_UPLOAD_URL)
    expect(said().join(' ')).toContain(FILE.path)
  })
})

describe('isPotaActivation', () => {
  const base = { reference: 'US-1234', dayStartUnix: 0, date: '2026-09-09', callsign: 'KD9TAW', qsos: 1 }

  it('takes POTA, and an older record with no program, and leaves a summit out', () => {
    expect(isPotaActivation({ ...base, program: 'POTA' })).toBe(true)
    expect(isPotaActivation({ ...base, program: 'pota' })).toBe(true)
    expect(isPotaActivation({ ...base, program: undefined })).toBe(true)
    expect(isPotaActivation({ ...base, program: 'SOTA' })).toBe(false)
  })
})
