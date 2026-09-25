// La carpeta de música se mueve por fuera de la app (el gestor de archivos,
// un disco que se desmonta): el vigilante del núcleo lo ve, la interfaz se
// entera sola (en el navegador, mirando `revision` cada dos segundos) y
// enseña «No encuentro tu música»; al elegir la carpeta en su sitio nuevo,
// vuelve todo, con las mismas canciones (su id, sus listas y sus notas).
import { test, expect } from '@playwright/test'
import { existsSync, renameSync } from 'node:fs'
import { SONGS } from './songs.js'
import { core, MUSIC, MOVED, putMusicBack, reset, rows, openApp, songsByTitle } from './helpers.js'

test.beforeEach(async ({ request }) => {
  await reset(request)
})

test('la carpeta que se mueve: «No encuentro tu música», se elige la nueva y vuelve todo', async ({
  page,
  request
}) => {
  test.setTimeout(150_000)
  await openApp(page)
  const ids = Object.values(await songsByTitle(request))
    .map((s) => s.id)
    .sort()
  try {
    renameSync(MUSIC, MOVED)
    // nadie recarga nada: lo trae el vigilante del núcleo
    await expect(page.getByRole('heading', { name: 'No encuentro tu música' })).toBeVisible({
      timeout: 60_000
    })
    await expect(page.locator('.missing-folders')).toContainText(MUSIC)

    await page.getByLabel('Ruta de la carpeta de música').fill(MOVED)
    await page.getByRole('button', { name: 'Analizar', exact: true }).click()
    await expect(page.locator('.toast', { hasText: 'es tu carpeta de antes' })).toBeVisible()
    await expect(rows(page)).toHaveCount(SONGS.length)
    const { folders } = await core(request, 'GET', '/folders')
    expect(folders.map((f) => f.path)).toEqual([MOVED])
    // las mismas canciones, no otras nuevas con los mismos archivos
    const back = Object.values(await songsByTitle(request))
      .map((s) => s.id)
      .sort()
    expect(back).toEqual(ids)
  } finally {
    // la deja donde estaba, con sus mismas canciones, para las demás pruebas
    const { folders } = await core(request, 'GET', '/folders')
    if (putMusicBack() && folders.some((f) => f.path === MOVED)) {
      await core(request, 'POST', '/folders/relocate', { from: MOVED, to: MUSIC })
    }
    expect(existsSync(MUSIC)).toBe(true)
  }
})
