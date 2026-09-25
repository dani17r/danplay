// Lo que comparten los flujos: hablar con el núcleo (por el proxy de Vite,
// con la cabecera del modo desarrollo), dejar la biblioteca como al empezar
// y esperar a que la lista esté pintada.
import { expect } from '@playwright/test'
import { existsSync, renameSync, rmSync } from 'node:fs'
import { join } from 'node:path'
import { SONGS } from './songs.js'

/** La carpeta de música de prueba (la genera e2e/server.mjs). */
export const MUSIC = join(process.env.DANPLAY_E2E_ROOT || '', 'Musica')
/** Donde la deja la prueba que la mueve por fuera. */
export const MOVED = MUSIC + '-movida'
/** La biblioteca grande (ver songs.js): solo la da de alta la prueba que la usa. */
export const BIG = join(process.env.DANPLAY_E2E_ROOT || '', 'Grande')

/**
 * Una petición al núcleo. La cabecera va aquí, en la petición, y no en la
 * configuración de Playwright: si fuera allí la pondría también en las
 * peticiones de la página, y no se vería si la interfaz se la olvida.
 * @param {import('@playwright/test').APIRequestContext} request
 * @param {string} method
 * @param {string} path  sin el /api
 * @param {any} [body]
 */
export async function core(request, method, path, body) {
  const r = await request.fetch('/api' + path, {
    method,
    headers: { 'X-DanPlay': '1' },
    data: body
  })
  const text = await r.text()
  if (!r.ok()) throw new Error(`${method} ${path}: ${r.status()} ${text}`)
  return text ? JSON.parse(text) : null
}

/** Una tarea larga del núcleo, de principio a fin (como `api.runJob`). */
export async function runJob(request, path, name, body) {
  const started = await core(request, 'POST', path, body)
  let job = started?.job
  for (let i = 0; i < 600 && (!job || job.active); i++) {
    await new Promise((r) => setTimeout(r, 250))
    job = await core(request, 'GET', `/jobs/${encodeURIComponent(name)}`)
  }
  if (job.error) throw new Error(job.error)
  return job.result
}

/** Devuelve la música a su sitio si una prueba la dejó movida. */
export function putMusicBack() {
  if (!existsSync(MOVED)) return false
  // lo que haya en el sitio de antes (una carpeta vacía que alguien volvió
  // a crear) sobra
  rmSync(MUSIC, { recursive: true, force: true })
  renameSync(MOVED, MUSIC)
  return true
}

/**
 * Deja la biblioteca como al empezar: la carpeta de música en su sitio, dada
 * de alta, analizada y sin repertorios. Una prueba que falla a medias (la que
 * quita la carpeta, la que la mueve) no arrastra a las siguientes.
 */
export async function reset(request) {
  putMusicBack()
  const { folders } = await core(request, 'GET', '/folders')
  for (const f of folders) {
    if (f.path !== MUSIC) {
      await core(request, 'DELETE', `/folders?path=${encodeURIComponent(f.path)}`)
    }
  }
  if (!folders.some((f) => f.path === MUSIC)) {
    await core(request, 'POST', '/folders', { path: MUSIC, label: '', force: false })
  }
  const status = await core(request, 'GET', '/status')
  if (status.stats.total !== SONGS.length || status.missing_folders.length) {
    await runJob(request, '/scan', 'escaneo')
  }
  expect((await core(request, 'GET', '/status')).stats.total).toBe(SONGS.length)
  for (const l of (await core(request, 'GET', '/playlists')).playlists || []) {
    await core(request, 'DELETE', `/playlists/${l.id}`)
  }
}

/** Las canciones del índice, por título. */
export async function songsByTitle(request) {
  const { songs } = await core(request, 'GET', '/search?limit=100')
  return Object.fromEntries(songs.map((s) => [s.title, s]))
}

/** Las filas de la lista que se ve (tabla, lista fina, fichas o cuadrícula). */
export const rows = (page) => page.locator('[data-song-row]')

/** Los títulos de las filas, en el orden en que se ven. */
export const titles = (page) => page.locator('[data-song-row] td.title')

/** El texto de esos títulos, limpio. */
export const titleTexts = async (page) => (await titles(page).allInnerTexts()).map((t) => t.trim())

/** Abre la app y espera a que la biblioteca esté pintada entera. */
export async function openApp(page) {
  await page.goto('/')
  await expect(rows(page)).toHaveCount(SONGS.length)
}

/** Pulsa una entrada de la barra lateral. */
export async function go(page, name) {
  await page.locator('.sidebar .nav-link', { hasText: name }).first().click()
}
