// ⭐ "SHARE STATS" — the Contest screen's Stats tab as one 1200×630 picture (the social-card aspect,
// as the Journey share card draws it), saved to this computer's Downloads folder through the same
// Rust-side save every other export uses. Nothing is posted anywhere: the operator shares the file
// where they like. The words come in already translated; this only draws them.
import { savePngToDownloads } from '../api'
import { pushToast } from '../toast'
import { t } from '../i18n'

export interface StatsCardData {
  call: string
  /** The contest, as the screen's header names it. */
  contest: string
  /** Label and value pairs, in reading order — eight fit. */
  tiles: { label: string; value: string }[]
  /** Contacts per UTC hour, oldest first. */
  hours: number[]
  footer: string
}

const W = 1200
const H = 630

/** Draw the card. */
export function renderStatsCard(d: StatsCardData): HTMLCanvasElement {
  const canvas = document.createElement('canvas')
  canvas.width = W
  canvas.height = H
  const ctx = canvas.getContext('2d')
  if (!ctx) return canvas
  const bg = ctx.createLinearGradient(0, 0, W, H)
  bg.addColorStop(0, '#0b1220')
  bg.addColorStop(1, '#101c2e')
  ctx.fillStyle = bg
  ctx.fillRect(0, 0, W, H)

  ctx.textBaseline = 'top'
  ctx.font = '600 44px "Cascadia Code", "JetBrains Mono", Consolas, monospace'
  ctx.fillStyle = '#5eead4'
  ctx.fillText(d.call.toUpperCase(), 60, 48)
  ctx.font = '700 40px system-ui, "Segoe UI", sans-serif'
  ctx.fillStyle = '#f1f5f9'
  ctx.fillText(d.contest, 60, 108, W - 120)

  // Two rows of four tiles.
  const tileW = (W - 120) / 4
  d.tiles.slice(0, 8).forEach((tile, i) => {
    const x = 60 + (i % 4) * tileW
    const y = 190 + Math.floor(i / 4) * 120
    ctx.font = '800 48px system-ui, "Segoe UI", sans-serif'
    ctx.fillStyle = '#f1f5f9'
    ctx.fillText(tile.value, x, y, tileW - 16)
    ctx.font = '500 24px system-ui, "Segoe UI", sans-serif'
    ctx.fillStyle = '#94a3b8'
    ctx.fillText(tile.label, x, y + 58, tileW - 16)
  })

  // The hourly bars along the bottom.
  const top = Math.max(1, ...d.hours)
  const n = d.hours.length
  if (n > 0) {
    const span = W - 120
    const step = span / n
    d.hours.forEach((c, i) => {
      const h = (c / top) * 120
      ctx.fillStyle = 'rgba(94, 234, 212, 0.75)'
      ctx.fillRect(60 + i * step, H - 80 - h, Math.max(1, step - 2), h)
    })
  }
  ctx.font = '500 24px system-ui, "Segoe UI", sans-serif'
  ctx.fillStyle = '#64748b'
  ctx.fillText(`${d.footer} · ${new Date().toISOString().slice(0, 10)}`, 60, H - 56)
  return canvas
}

/** Save the drawn card to Downloads as `name`. The toast says what happened: the path written, or
 *  why not — never a success for a save that did not land. */
export async function saveStatsCard(canvas: HTMLCanvasElement, name: string): Promise<void> {
  try {
    const dataUrl = canvas.toDataURL('image/png')
    const path = await savePngToDownloads(name, dataUrl.slice(dataUrl.indexOf(',') + 1))
    pushToast(t('contestStats.saved', { path }), 'success', 5000)
  } catch (e) {
    pushToast(t('contestStats.saveFailed', { detail: e instanceof Error ? e.message : String(e) }), 'error')
  }
}
