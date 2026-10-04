// Entry for the Stations on this network window's page (lan.html), a separate Vite entry, NOT the
// app's main.tsx: this page is served from the loopback origin and reaches no Nexus command, so
// none of the desktop's start-up (settings, pop-outs, the stream input) belongs here.
import { createRoot } from 'react-dom/client'
import { initLocale, installCatalog, setLocale } from '../i18n'
import { DE } from '../i18n/de'
import { ES } from '../i18n/es'
import { FR } from '../i18n/fr'
import { JA } from '../i18n/ja'
import { LanApp } from './LanApp'
import '../styles.css'

// Every shipped language, then the one the window was opened in (the app's own, passed in the
// address), else this page's stored or the OS one.
installCatalog('de', DE)
installCatalog('es', ES)
installCatalog('fr', FR)
installCatalog('ja', JA)
initLocale()
const asked = new URLSearchParams(location.search).get('lang')
if (asked) setLocale(asked)
createRoot(document.getElementById('root')!).render(<LanApp />)
