import { useCallback, useEffect, useState } from 'react'
import { activeSkin, skinOf, type SkinId } from './features/skins'
import { PALETTE_EVENT } from './usePaletteRoles'
import type { Theme } from './useTheme'

// THE BUILT-IN THEMES AT RUNTIME (features/skins.ts; Settings ▸ Appearance ▸ Theme).
//
// `useSkin` is the OWNER, called once per document where useTheme is (App, and a pop-out's
// DetachedPanel): it holds the operator's pick and is the ONLY writer of `data-skin` in its
// document. Settings is handed the theme and the setter as props rather than calling this itself,
// for the reason usePaletteRoles gives: two owners in one document race on mount.
//
// A THEME PAINTS ONLY ON ITS BASE. The attribute is set while the page is the theme's own base
// (`activeSkin`), and never otherwise: a dark theme's wells rule has no theme in its selector, so a
// dark theme left on the light page would paint the light page's wells. Picking a theme sets the
// page to its base as well (the gallery does both), and picking Dark, Light or System clears it.
//
// PER MACHINE, webview-local, like the theme itself: a fact about this screen and the room, not
// station data (the backup carries it as `appearance.skin`, beside the theme). The standard
// themes are NO attribute and NO key. index.html seeds the attribute before first paint
// (index-preseed.test.ts executes that copy against this hook). Like the theme, an open pop-out
// picks up a change the next time it opens.
//
// A change fires PALETTE_EVENT: the canvases that cache token colours key their caches on
// usePaletteKey, which reads this attribute too.

// ⚠️ The localStorage calls name SKIN_STORAGE_KEY directly: storage-scope.test.ts resolves a key
// only when it is a literal or a same-file const at the call site (see useFieldMode.ts).
const SKIN_STORAGE_KEY = 'nexus-skin'

function readStored(): SkinId | null {
  try {
    return skinOf(localStorage.getItem(SKIN_STORAGE_KEY))?.id ?? null
  } catch {
    return null
  }
}

/** [the theme painting now — the pick, while the page is its base — and the setter; null is the
 *  standard theme]. `theme` is the theme the page paints (useTheme's first value). */
export function useSkin(theme: Theme): [SkinId | null, (id: SkinId | null) => void] {
  const [picked, setPicked] = useState<SkinId | null>(readStored)
  const skin = activeSkin(picked, theme)

  useEffect(() => {
    const root = document.documentElement
    if (skin) root.setAttribute('data-skin', skin)
    else root.removeAttribute('data-skin')
    window.dispatchEvent(new Event(PALETTE_EVENT))
  }, [skin])

  // Storage is written by the pick, never by the mount: opening a window must not rewrite a
  // choice made in another one.
  const setSkin = useCallback((id: SkinId | null) => {
    const next = skinOf(id)?.id ?? null
    try {
      if (next) localStorage.setItem(SKIN_STORAGE_KEY, next)
      else localStorage.removeItem(SKIN_STORAGE_KEY)
    } catch {
      /* storage unavailable — the theme still applies for this session */
    }
    setPicked(next)
  }, [])

  return [skin, setSkin]
}
