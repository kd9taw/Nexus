// @vitest-environment jsdom
//
// A memory's recorded screen, on the surfaces that show or set it: the toolbar's Save and ＋ New
// stamp it, and the editor's Opens on field shows it and changes it. Which screens that field offers
// is `screenFits`, the same rule a recall obeys, so it never offers one a recall would ignore.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { cleanup, fireEvent, render } from '@testing-library/react'
import { MemoriesView } from './MemoriesView'
import { addMemory, emptyBank, memoriesStore, type MemoryScreen } from '../features/memories'
import { pushToast } from '../toast'
import { t } from '../i18n'

vi.mock('../toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
}))

const view = (dialScreen?: MemoryScreen) => (
  <MemoriesView dialMhz={144.8875} dialMode="FM" dialScreen={dialScreen} onRecall={() => {}} />
)
const toolbarButton = (root: HTMLElement, text: string): HTMLButtonElement => {
  const b = [...root.querySelectorAll<HTMLButtonElement>('.mv-tool')].find((x) => x.textContent?.includes(text))
  if (!b) throw new Error(`no toolbar button with ${text}`)
  return b
}
const FREQ = (144.8875).toFixed(3)
const saveButton = (root: HTMLElement) => toolbarButton(root, FREQ)
const rows = () => memoriesStore.get().memories

function editorSelect(root: HTMLElement): HTMLSelectElement {
  const label = [...root.querySelectorAll('.mv-field')].find(
    (f) => f.querySelector('span')?.textContent === t('memories.editor.screen.label'),
  )
  if (!label) throw new Error('no Opens on field in the editor')
  return label.querySelector('select') as HTMLSelectElement
}
const openEditor = (root: HTMLElement) => fireEvent.click(root.querySelector('.mv-row-edit') as HTMLElement)
const offered = (s: HTMLSelectElement) => [...s.options].map((o) => o.value)

beforeEach(() => {
  localStorage.clear()
  memoriesStore.set(emptyBank())
  vi.mocked(pushToast).mockClear()
})
afterEach(cleanup)

describe('saving', () => {
  it('Save names the screen, in its tooltip and on the row', () => {
    const { container } = render(view('sstv'))
    const save = saveButton(container)
    expect(save.title).toBe(t('memories.toolbar.save.title.screen', { screen: 'SSTV' }))
    fireEvent.click(save)
    expect(rows()[0]).toMatchObject({ rxMhz: 144.8875, mode: 'FM', screen: 'sstv' })
  })

  it('with no screen to name, Save reads and saves as it always did', () => {
    const { container } = render(view())
    const save = saveButton(container)
    expect(save.title).toBe(t('memories.toolbar.save.title'))
    fireEvent.click(save)
    expect(rows()[0]).not.toHaveProperty('screen')
    expect(pushToast).toHaveBeenCalledWith(t('memories.toast.saved', { freq: FREQ, mode: 'FM' }), 'success', 2500)
  })

  it('＋ New names it too', () => {
    const { container } = render(view('sstv'))
    fireEvent.click(toolbarButton(container, t('memories.toolbar.new.label')))
    expect(rows()[0].screen).toBe('sstv')
  })

  it('an equivalent row moves, and the operator is told it did', () => {
    memoriesStore.set(addMemory(emptyBank(), { rxMhz: 144.8875, mode: 'FM' }))
    const { container } = render(view('sstv'))
    fireEvent.click(saveButton(container))
    expect(rows()).toHaveLength(1)
    expect(rows()[0].screen).toBe('sstv')
    expect(pushToast).toHaveBeenCalledWith(
      t('memories.toast.moved', { freq: FREQ, mode: 'FM', screen: 'SSTV' }),
      'success',
      2500,
    )
  })

  it('saved again from the same screen, it is only already saved', () => {
    memoriesStore.set(addMemory(emptyBank(), { rxMhz: 144.8875, mode: 'FM', screen: 'sstv' }))
    const { container } = render(view('sstv'))
    fireEvent.click(saveButton(container))
    expect(pushToast).toHaveBeenCalledWith(t('memories.toast.alreadySaved', { freq: FREQ, mode: 'FM' }), 'info', 2500)
  })
})

describe('the editor’s Opens on field', () => {
  it('offers the screens that can take the memory, and By its mode first', () => {
    memoriesStore.set(addMemory(emptyBank(), { rxMhz: 144.8875, mode: 'FM' }))
    const { container } = render(view())
    openEditor(container)
    const s = editorSelect(container)
    expect(offered(s)).toEqual(['', 'phone', 'sstv', 'aprs'])
    expect(s.value).toBe('')
    expect(s.options[0].textContent).toBe(t('memories.editor.screen.auto'))
  })

  it('a memory off 2 m is not offered APRS, and a digital one no voice screen', () => {
    memoriesStore.set(addMemory(emptyBank(), { rxMhz: 14.074, mode: 'FT8' }))
    const { container } = render(view())
    openEditor(container)
    expect(offered(editorSelect(container))).toEqual(['', 'operate', 'rtty', 'psk', 'jtty', 'js8'])
  })

  it('sets the screen, leaves a pack’s row the pack’s, and clears it again', () => {
    memoriesStore.set(addMemory(emptyBank(), { rxMhz: 144.8875, mode: 'FM', source: 'curated' }))
    const { container } = render(view())
    openEditor(container)
    fireEvent.change(editorSelect(container), { target: { value: 'sstv' } })
    expect(rows()[0].screen).toBe('sstv')
    expect(rows()[0].source).toBe('curated')
    fireEvent.change(editorSelect(container), { target: { value: '' } })
    expect(rows()[0]).not.toHaveProperty('screen')
  })

  it('a screen the mode no longer fits reads as By its mode, which is what a recall then does', () => {
    memoriesStore.set(addMemory(emptyBank(), { rxMhz: 14.03, mode: 'CW', screen: 'sstv' }))
    const { container } = render(view())
    openEditor(container)
    expect(editorSelect(container).value).toBe('')
  })
})
