// «Acerca de DanPlay», las novedades y los términos (AboutPage, TermsModal,
// useTerms, data/releases.js): quién lo hizo a la vista, los términos que
// hay que leer hasta el final para aceptarlos y lo que se bloquea sin ellos.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { readFileSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const held = vi.hoisted(() => ({ state: null, api: null, app: null }))
vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { createState, buildApiDouble, createAppDouble } = await import('./support/backend.js')
  held.state = createState()
  held.api = buildApiDouble(actual, held.state)
  held.app = createAppDouble()
  return { ...actual, api: held.api, app: held.app }
})

import AboutPage from '../src/components/AboutPage.vue'
import TermsModal from '../src/components/TermsModal.vue'
import Sidebar from '../src/components/Sidebar.vue'
import { useTerms, resetTerms, requireTerms } from '../src/composables/useTerms.js'
import { useSeparation, resetSeparation } from '../src/composables/useSeparation.js'
import { AUTHOR, TERMS, TERMS_VERSION } from '../src/data/terms.js'
import { RELEASES, compareVersions, newSince } from '../src/data/releases.js'
import { version } from '../package.json'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..')
const terms = useTerms()
const mounted = []

/** El núcleo dice que no se aceptaron. */
function revokeTerms() {
  held.state.terms = { accepted: '', at: null, app: '' }
}

/** El lector del texto, con un alto de verdad (jsdom no mide nada). */
function readerWithHeight(el, { scroll = 1000, height = 300, top = 0 } = {}) {
  Object.defineProperty(el, 'scrollHeight', { configurable: true, value: scroll })
  Object.defineProperty(el, 'clientHeight', { configurable: true, value: height })
  Object.defineProperty(el, 'scrollTop', { configurable: true, writable: true, value: top })
}

async function modal() {
  const w = mount(TermsModal, { attachTo: document.body })
  mounted.push(w)
  await flushPromises()
  return w
}

beforeEach(() => {
  resetTerms()
  localStorage.clear()
  held.state.terms = { accepted: TERMS_VERSION, at: 1, app: '1.0.0' }
  vi.clearAllMocks()
})
afterEach(() => {
  for (const w of mounted.splice(0)) w.unmount()
  document.body.innerHTML = ''
})

describe('la ventana de los términos', () => {
  it('la casilla no se puede marcar hasta bajar al final, ni aceptar sin ella', async () => {
    revokeTerms()
    // un texto largo: hay que bajarlo (las alturas, antes de que se abra)
    const height = vi.spyOn(HTMLElement.prototype, 'scrollHeight', 'get').mockReturnValue(1000)
    const seen = vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(300)
    const w = await modal()
    terms.show('separar canciones en pistas')
    await flushPromises()
    const box = document.querySelector('.terms-modal')
    expect(box).toBeTruthy()
    expect(box.textContent).toContain(AUTHOR.name)
    expect(box.textContent).toContain('Para separar canciones en pistas necesitas aceptar')
    for (const sec of TERMS) expect(box.textContent).toContain(sec.title)
    const checkbox = box.querySelector('.terms-agree input')
    const accept = [...box.querySelectorAll('button')].find(
      (b) => b.textContent.trim() === 'Aceptar'
    )
    expect(checkbox.disabled, 'se podía marcar sin leer').toBe(true)
    expect(accept.disabled).toBe(true)
    expect(box.textContent).toContain('Baja hasta el final')

    // a medio camino, aún no
    const reader = box.querySelector('.terms-reader')
    readerWithHeight(reader, { top: 300 })
    reader.dispatchEvent(new Event('scroll'))
    await flushPromises()
    expect(checkbox.disabled).toBe(true)

    // al final, sí; y aceptar pide además la casilla
    reader.scrollTop = 700
    reader.dispatchEvent(new Event('scroll'))
    await flushPromises()
    expect(checkbox.disabled).toBe(false)
    expect(accept.disabled).toBe(true)
    checkbox.click()
    await flushPromises()
    expect(accept.disabled).toBe(false)
    accept.click()
    await flushPromises()
    expect(held.api.acceptTerms).toHaveBeenCalledWith(TERMS_VERSION)
    expect(terms.accepted.value).toBe(true)
    expect(document.querySelector('.terms-modal')).toBeNull()
    height.mockRestore()
    seen.mockRestore()
    w.unmount()
  })

  it('salen solos la primera vez, y no otra vez al volver a abrir', async () => {
    revokeTerms()
    expect(await terms.showIfFirstTime()).toBe(true)
    expect(terms.state.open).toBe(true)
    terms.close()
    resetTerms()
    expect(await terms.showIfFirstTime(), 'salieron otra vez').toBe(false)
    // aceptados, nunca
    resetTerms()
    localStorage.clear()
    held.state.terms = { accepted: TERMS_VERSION, at: 1, app: '' }
    expect(await terms.showIfFirstTime()).toBe(false)
  })

  it('si los términos cambian, se vuelven a pedir', async () => {
    held.state.terms = { accepted: '2020-01-01', at: 1, app: '' }
    localStorage.setItem('danplay.termsSeen', '2020-01-01')
    expect(await terms.showIfFirstTime()).toBe(true)
    expect(terms.accepted.value).toBe(false)
  })
})

describe('lo que se bloquea sin aceptarlos', () => {
  it('separar en pistas abre los términos y, sin aceptarlos, no separa', async () => {
    revokeTerms()
    resetSeparation()
    const request = useSeparation().request([{ id: 1, title: 'Mi Gozo' }])
    await flushPromises()
    expect(terms.state.open).toBe(true)
    expect(terms.state.reason).toBe('separar canciones en pistas')
    terms.close()
    expect(await request).toBe(false)
    expect(held.api.separate).not.toHaveBeenCalled()
  })

  it('aceptándolos, sigue con lo que se pidió', async () => {
    revokeTerms()
    const request = requireTerms('chat')
    await flushPromises()
    expect(terms.state.reason).toBe('usar el asistente de IA')
    await terms.accept()
    expect(await request).toBe(true)
    // y ya no se vuelve a preguntar
    expect(await requireTerms('transform')).toBe(true)
    expect(terms.state.open).toBe(false)
  })
})

describe('Acerca de DanPlay', () => {
  const page = async (tab = 'about') => {
    const w = mount(AboutPage, { props: { tab }, attachTo: document.body })
    mounted.push(w)
    await flushPromises()
    return w
  }

  it('dice quién lo hizo y cómo escribirle para colaborar', async () => {
    const w = await page()
    expect(w.text()).toContain('Creado y desarrollado por Daniel Rangel')
    expect(w.text()).toContain('DanPlay es obra de Daniel Rangel')
    expect(w.text()).toContain('@Dani17r')
    expect(w.text()).toContain('https://t.me/Dani17r')
    const button = w.findAll('button').find((b) => b.text().includes('Telegram'))
    await button.trigger('click')
    expect(held.app.openInBrowser).toHaveBeenCalledWith('https://t.me/Dani17r')
  })

  it('las novedades: cada versión, corta, y la tuya señalada', async () => {
    const w = await page('news')
    const versions = w.findAll('.release-version').map((e) => e.text())
    expect(versions).toEqual(RELEASES.map((r) => r.version))
    const mine = w.find('.release.mine')
    expect(mine.exists()).toBe(true)
    expect(mine.find('.release-version').text()).toBe(version)
    expect(mine.text()).toContain('Tu versión')
  })

  it('los términos: el texto, y si no se aceptaron, el botón para hacerlo', async () => {
    revokeTerms()
    await terms.load()
    const w = await page('terms')
    expect(w.text()).toContain('Aún no los aceptaste')
    for (const sec of TERMS) expect(w.text()).toContain(sec.title)
    await w
      .findAll('button')
      .find((b) => b.text() === 'Leer y aceptar')
      .trigger('click')
    expect(terms.state.open).toBe(true)
  })

  it('las pestañas avisan para cambiar', async () => {
    const w = await page()
    await w.findAll('.about-tab')[1].trigger('click')
    expect(w.emitted('tab')[0]).toEqual(['news'])
  })
})

describe('el crédito en la barra lateral', () => {
  it('siempre a la vista, y lleva a «Acerca de»', async () => {
    const w = mount(Sidebar, { props: { view: { kind: 'all' } } })
    mounted.push(w)
    expect(w.find('.nav-credit').text()).toContain('Daniel Rangel')
    await w.find('.nav-credit').trigger('click')
    expect(w.emitted('go').at(-1)).toEqual([{ kind: 'about', tab: 'about' }])
  })

  it('tras actualizar, «Acerca de DanPlay» dice «Nuevo» y abre las novedades', async () => {
    const w = mount(Sidebar, { props: { view: { kind: 'all' }, newsBadge: true } })
    mounted.push(w)
    const button = w.findAll('.nav-link').find((b) => b.text().includes('Acerca de DanPlay'))
    expect(button.text()).toContain('Nuevo')
    await button.trigger('click')
    expect(w.emitted('go').at(-1)).toEqual([{ kind: 'about', tab: 'news' }])
  })
})

describe('las novedades y el CHANGELOG', () => {
  const changelog = readFileSync(join(ROOT, 'CHANGELOG.md'), 'utf8')
  const inChangelog = [...changelog.matchAll(/^## (\d+\.\d+\.\d+) — (.+)$/gm)].map((m) => ({
    version: m[1],
    date: /^\d{4}-\d{2}-\d{2}$/.test(m[2].trim()) ? m[2].trim() : ''
  }))

  it('cada versión desde la 1.20.0 está, con su fecha', () => {
    const since = inChangelog.filter((v) => compareVersions(v.version, '1.20.0') >= 0)
    expect(RELEASES.map((r) => [r.version, r.date])).toEqual(since.map((v) => [v.version, v.date]))
  })

  it('son cortas: una línea de resumen y frases breves', () => {
    for (const r of RELEASES) {
      expect(r.summary.length, r.version).toBeLessThanOrEqual(100)
      expect(r.items.length, r.version).toBeGreaterThan(0)
      for (const it of r.items) expect(it.length, it).toBeLessThanOrEqual(120)
    }
  })

  it('lo nuevo desde la última vez que se miró', () => {
    expect(newSince('', '1.27.0')).toEqual([])
    expect(newSince('1.25.0', '1.27.0').map((r) => r.version)).toEqual(['1.27.0', '1.26.0'])
    expect(compareVersions('1.10.0', '1.9.9')).toBeGreaterThan(0)
  })
})
