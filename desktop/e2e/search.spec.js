// El desplegable de resultados del buscador y las opciones de cada resultado.
//
// Fuera de «Todas las canciones» y «Artistas» (en Favoritos, dentro de un
// repertorio…) el buscador no filtra la lista que se ve: busca en TODA la
// biblioteca y enseña lo que encuentra en un desplegable bajo la caja. Sus
// filas no tenían menú de opciones (el clic derecho está bloqueado en toda la
// app y cada sitio pone el suyo), y un menú abierto desde ahí se cerraba al
// pulsar en él: el menú cuelga del cuerpo, fuera de la caja del buscador, y el
// clic contaba como «fuera». Resultado: no se podía añadir lo encontrado a un
// repertorio. Ahora el clic derecho, el «⋯» y la tecla de menú abren las
// opciones de la fila, y el desplegable sigue abierto para hacer otra cosa.
import { test, expect } from '@playwright/test'
import { core, reset, titles, openApp, go, songsByTitle } from './helpers.js'

/** Los repertorios de estas pruebas acaban así: se borran al terminar cada una. */
const SUFFIX = ' e2e'

test.beforeEach(async ({ request }) => {
  await reset(request)
})

test.afterEach(async ({ request }) => {
  // El núcleo y su biblioteca son de todas las pruebas (un solo worker): lo que
  // crea esta, lo deshace esta. `reset` quita los repertorios al empezar, no al
  // acabar, y las favoritas no las toca.
  const { playlists } = await core(request, 'GET', '/playlists')
  for (const l of playlists.filter((p) => p.name.endsWith(SUFFIX))) {
    await core(request, 'DELETE', `/playlists/${l.id}`)
  }
  const { songs } = await core(request, 'GET', '/search?only_favorites=true&limit=100')
  for (const s of songs) {
    await core(request, 'POST', `/song/${s.id}/favorite`, { favorite: false })
  }
})

/** Un repertorio nuevo, por la API. Antes de abrir la app: lee las listas al arrancar. */
async function createPlaylist(request, name) {
  const { id } = await core(request, 'POST', '/playlists', { name: name + SUFFIX })
  return id
}

/**
 * Los títulos que tiene un repertorio, según el núcleo, ordenados: lo que se
 * prueba aquí es QUÉ canciones tiene, no en qué orden las guarda.
 */
async function titlesOf(request, id) {
  const { songs } = await core(request, 'GET', `/playlists/${id}/songs`)
  return songs.map((s) => s.title).sort()
}

/** ¿Es favorita, según el núcleo? */
async function isFavorite(request, id) {
  return Boolean((await core(request, 'GET', `/song/${id}`)).favorite)
}

const box = (page) => page.getByRole('combobox', { name: 'Buscar en la biblioteca' })
const dropdown = (page) => page.getByRole('listbox', { name: 'Resultados en toda la biblioteca' })
/** La fila del desplegable de esa canción. */
const result = (page, title) => page.locator('.sr-row').filter({ hasText: title })
/** El «⋯» de la fila de esa canción. */
const more = (page, title) => result(page, title).locator('.sr-more')
const menu = (page) => page.getByRole('menu')
/** El menú de opciones de esa canción: su cabecera es el título. */
const menuOf = (page, title) => page.getByRole('menu', { name: title, exact: true })
/** Una opción del menú abierto: por su texto entero, o por una expresión. */
const menuItem = (page, name) =>
  page.getByRole('menuitem', { name, exact: typeof name === 'string' })

/**
 * Abre la app en una vista donde el buscador NO filtra la lista: Favoritos, o
 * el repertorio que se diga. Espera a que el lateral tenga ya los `listas`
 * repertorios que hay: el menú de una canción ofrece los que conoce al abrirse.
 */
async function open(page, { list = '', lists = 0 } = {}) {
  await openApp(page)
  await expect(page.locator('.nav-playlist')).toHaveCount(lists)
  if (list) {
    await page.locator('.nav-playlist', { hasText: list }).click()
    await expect(page.locator('.sidebar .nav-playlist.active')).toContainText(list)
  } else {
    await go(page, 'Favoritos')
    await expect(page.locator('.sidebar .nav-link.active')).toContainText('Favoritos')
  }
}

/** Escribe en la caja de búsqueda y espera al desplegable con exactamente esos resultados. */
async function typeText(page, text, expected) {
  await box(page).fill(text)
  await expect(dropdown(page)).toBeVisible()
  await expect
    .poll(async () => (await page.locator('.sr-row .sr-title').allTextContents()).sort())
    .toEqual([...expected].sort())
}

/**
 * Abre el menú de un resultado (clic derecho) y comprueba que ofrece esa opción.
 *
 * Tras cambiar algo desde el menú (una favorita) el núcleo ya lo tiene, pero la
 * app puede no haber recibido aún su respuesta: el menú se monta con lo que
 * sabe en ese momento y no se corrige solo. Si no la ofrece, se cierra y se
 * vuelve a abrir, que es lo que haría quien lo mirase.
 */
async function openMenuWith(page, title, option) {
  await expect(async () => {
    await result(page, title).click({ button: 'right' })
    await expect(menuOf(page, title)).toBeVisible()
    const offered = await menuItem(page, option).isVisible()
    if (!offered) {
      await page.keyboard.press('Escape')
      await expect(menu(page)).toHaveCount(0)
    }
    expect(offered, `el menú de «${title}» no ofrece «${option}»`).toBe(true)
  }).toPass({ timeout: 15_000 })
}

/** Del menú abierto de una canción: «Añadir a una lista». */
async function openPlaylists(page) {
  await menuItem(page, 'Añadir a una lista').click()
  await expect(menu(page)).toHaveAccessibleName('Añadir a una lista')
}

/** Dentro de «Añadir a una lista»: ese repertorio. El menú se cierra, el desplegable no. */
async function pickPlaylist(page, list) {
  await menuItem(page, new RegExp(`^${list}`)).click()
  await expect(menu(page)).toHaveCount(0)
}

test('en Favoritos, un resultado se añade a un repertorio con el clic derecho y con el «⋯», varias veces seguidas', async ({
  page,
  request
}) => {
  const sunday = await createPlaylist(request, 'Domingo')
  const rehearsal = await createPlaylist(request, 'Ensayo')
  await open(page, { lists: 2 })
  await typeText(page, 'barak', ['Mi Gozo', 'Sera Llena La Tierra'])

  // clic derecho en un resultado: sus opciones, y a un repertorio
  await result(page, 'Mi Gozo').click({ button: 'right' })
  await expect(menuOf(page, 'Mi Gozo')).toBeVisible()
  await openPlaylists(page)
  await pickPlaylist(page, 'Domingo')
  await expect.poll(() => titlesOf(request, sunday)).toEqual(['Mi Gozo'])
  await expect(page.locator('.toast', { hasText: 'Añadida a «Domingo e2e»' })).toBeVisible()
  // el desplegable no se ha cerrado: se puede seguir con lo encontrado
  await expect(dropdown(page)).toBeVisible()
  await expect(page.locator('.sr-row')).toHaveCount(2)

  // el «⋯» de otro resultado, a otro repertorio. El menú ofrece los repertorios
  // que conoce al abrirse: se espera a que la app ya cuente la canción nueva.
  await expect(page.locator('.nav-playlist', { hasText: 'Domingo' }).locator('.count')).toHaveText(
    '1'
  )
  await more(page, 'Sera Llena La Tierra').click()
  await expect(menuOf(page, 'Sera Llena La Tierra')).toBeVisible()
  await openPlaylists(page)
  // el menú ya sabe que el primer repertorio tiene una canción
  await expect(menuItem(page, /^Domingo/).locator('.ctx-note')).toHaveText('1')
  await pickPlaylist(page, 'Ensayo')
  await expect.poll(() => titlesOf(request, rehearsal)).toEqual(['Sera Llena La Tierra'])
  expect(await titlesOf(request, sunday), 'la primera sigue donde estaba').toEqual(['Mi Gozo'])
  await expect(dropdown(page)).toBeVisible()
  await expect(page.locator('.sr-row')).toHaveCount(2)

  // y una tercera vez, la primera a la otra lista: nada se ha quedado a medias
  await result(page, 'Mi Gozo').click({ button: 'right' })
  await expect(menuOf(page, 'Mi Gozo')).toBeVisible()
  await openPlaylists(page)
  await pickPlaylist(page, 'Ensayo')
  await expect.poll(() => titlesOf(request, rehearsal)).toEqual(['Mi Gozo', 'Sera Llena La Tierra'])
  await expect(page.locator('.nav-playlist', { hasText: 'Ensayo' }).locator('.count')).toHaveText(
    '2'
  )
  await expect(dropdown(page)).toBeVisible()
  await expect(page.locator('.sr-row')).toHaveCount(2)
})

test('el «⋯» de cada resultado se ve en el resaltado y al pasar el puntero', async ({ page }) => {
  await open(page)
  // el puntero, lejos del desplegable: ninguna fila lo tiene encima
  await page.mouse.move(700, 600)
  await typeText(page, 'barak', ['Mi Gozo', 'Sera Llena La Tierra'])
  // Por título y no por «la resaltada»: al pasar el puntero por la otra, la
  // resaltada pasa a ser ella.
  const [first, second] = await page.locator('.sr-row .sr-title').allTextContents()
  const moreOf = (title) => result(page, title).locator('.sr-more')
  await expect(result(page, first)).toHaveClass(/\bon\b/)
  await expect(moreOf(first)).toHaveCSS('opacity', '1')
  await expect(moreOf(second)).toHaveCSS('opacity', '0')
  await result(page, second).hover()
  await expect(moreOf(second)).toHaveCSS('opacity', '1')
  // dentro de un «option» sus hijos son presentacionales: el nombre ya lo dice la
  // opcion, y quien no ve la pantalla llega con la tecla de menú
  await expect(moreOf(second)).toHaveAttribute('aria-hidden', 'true')
  await expect(moreOf(second)).toHaveAttribute('title', 'Opciones')
})

test('dentro de un repertorio, el menú de un resultado es el del resultado y no el de la lista', async ({
  page,
  request
}) => {
  const byTitle = await songsByTitle(request)
  const sunday = await createPlaylist(request, 'Domingo')
  const rehearsal = await createPlaylist(request, 'Ensayo')
  await core(request, 'POST', `/playlists/${sunday}/songs`, { ids: [byTitle['Shekinah'].id] })
  await open(page, { list: 'Domingo', lists: 2 })
  await expect(titles(page)).toHaveText(['Shekinah'])
  // el buscador mira toda la biblioteca, no la lista abierta: New Wine tiene dos
  await typeText(page, 'new wine', ['A Una Voz', 'Shekinah'])

  // la que ya está en la lista abierta: su menú no es el de la fila de la lista
  await result(page, 'Shekinah').click({ button: 'right' })
  await expect(menuOf(page, 'Shekinah')).toBeVisible()
  await expect(menuItem(page, 'Añadir a una lista')).toBeVisible()
  await expect(menuItem(page, 'Quitar de esta lista')).toHaveCount(0)
  // y no ha elegido la fila de detrás
  await expect(page.locator('[data-song-row].selected')).toHaveCount(0)
  await openPlaylists(page)
  await pickPlaylist(page, 'Ensayo')
  await expect.poll(() => titlesOf(request, rehearsal)).toEqual(['Shekinah'])
  expect(await titlesOf(request, sunday), 'la lista abierta no pierde nada').toEqual(['Shekinah'])
  await expect(dropdown(page)).toBeVisible()

  // la otra, a la lista que está abierta: también se puede
  await result(page, 'A Una Voz').click({ button: 'right' })
  await expect(menuOf(page, 'A Una Voz')).toBeVisible()
  await expect(menuItem(page, 'Quitar de esta lista')).toHaveCount(0)
  await openPlaylists(page)
  await pickPlaylist(page, 'Domingo')
  await expect.poll(() => titlesOf(request, sunday)).toEqual(['A Una Voz', 'Shekinah'])
  await expect(page.locator('[data-song-row].selected')).toHaveCount(0)
  await expect(dropdown(page)).toBeVisible()
})

test('con un menú abierto, el clic derecho en otro resultado abre el de ese resultado', async ({
  page,
  request
}) => {
  const list = await createPlaylist(request, 'Domingo')
  await open(page, { lists: 1 })
  await typeText(page, 'barak', ['Mi Gozo', 'Sera Llena La Tierra'])
  // El menú nace bajo el puntero y tapa lo que queda debajo: se abre en la fila
  // de abajo y se pasa a la de arriba, que queda libre.
  const [top, bottom] = await page.locator('.sr-row .sr-title').allTextContents()

  // el menú de una, ya dentro de «Añadir a una lista»…
  await result(page, bottom).click({ button: 'right' })
  await expect(menuOf(page, bottom)).toBeVisible()
  await openPlaylists(page)
  // …y sin cerrarlo, el clic derecho en la otra
  await result(page, top).click({ button: 'right' })
  await expect(menuOf(page, top)).toBeVisible()
  await expect(menu(page)).toHaveCount(1)
  // es su menú entero, no el submenú de la anterior
  await expect(menuItem(page, 'Marcar como favorito')).toBeVisible()
  await expect(dropdown(page)).toBeVisible()
  await openPlaylists(page)
  await pickPlaylist(page, 'Domingo')
  await expect.poll(() => titlesOf(request, list)).toEqual([top])
})

test('Escape con el menú abierto cierra el menú y no el desplegable; un clic fuera lo cierra todo', async ({
  page
}) => {
  await open(page)
  await typeText(page, 'barak', ['Mi Gozo', 'Sera Llena La Tierra'])

  // Escape con el menú abierto: se va el menú, el desplegable se queda
  await result(page, 'Mi Gozo').click({ button: 'right' })
  await expect(menuOf(page, 'Mi Gozo')).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(menu(page)).toHaveCount(0)
  await expect(dropdown(page)).toBeVisible()
  await expect(page.locator('.sr-row')).toHaveCount(2)

  // dentro de «Añadir a una lista», Escape vuelve al menú, y otro lo cierra
  await result(page, 'Mi Gozo').click({ button: 'right' })
  await openPlaylists(page)
  await page.keyboard.press('Escape')
  await expect(menuOf(page, 'Mi Gozo')).toBeVisible()
  await expect(dropdown(page)).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(menu(page)).toHaveCount(0)
  await expect(dropdown(page)).toBeVisible()

  // con el menú ya cerrado, Escape es del desplegable (lo de siempre)
  await page.keyboard.press('Escape')
  await expect(dropdown(page)).toHaveCount(0)

  // un clic en cualquier otro sitio cierra el menú y el desplegable a la vez
  await typeText(page, 'new wine', ['A Una Voz', 'Shekinah'])
  await result(page, 'Shekinah').click({ button: 'right' })
  await expect(menuOf(page, 'Shekinah')).toBeVisible()
  await page.locator('.brand').click()
  await expect(menu(page)).toHaveCount(0)
  await expect(dropdown(page)).toHaveCount(0)
})

test('marcar una favorita desde el menú de un resultado cambia lo que dice el menú la próxima vez', async ({
  page,
  request
}) => {
  const byTitle = await songsByTitle(request)
  const firstSongId = byTitle['Mi Gozo'].id
  const secondSongId = byTitle['Sera Llena La Tierra'].id
  await open(page)
  await typeText(page, 'barak', ['Mi Gozo', 'Sera Llena La Tierra'])

  // no es favorita: el menú ofrece marcarla
  await more(page, 'Mi Gozo').click()
  await expect(menuOf(page, 'Mi Gozo')).toBeVisible()
  await expect(menuItem(page, 'Quitar de favoritos')).toHaveCount(0)
  await menuItem(page, 'Marcar como favorito').click()
  await expect(menu(page)).toHaveCount(0)
  await expect.poll(() => isFavorite(request, firstSongId)).toBe(true)
  // el desplegable sigue ahí, y la otra no ha cambiado
  await expect(dropdown(page)).toBeVisible()
  expect(await isFavorite(request, secondSongId)).toBe(false)

  // la próxima vez el menú dice lo contrario (y es el mismo con el clic derecho)
  await openMenuWith(page, 'Mi Gozo', 'Quitar de favoritos')
  await expect(menuItem(page, 'Marcar como favorito')).toHaveCount(0)
  await menuItem(page, 'Quitar de favoritos').click()
  await expect(menu(page)).toHaveCount(0)
  await expect.poll(() => isFavorite(request, firstSongId)).toBe(false)

  // y otra vez al revés
  await openMenuWith(page, 'Mi Gozo', 'Marcar como favorito')
  await expect(menuItem(page, 'Quitar de favoritos')).toHaveCount(0)
  await expect(dropdown(page)).toBeVisible()
})

test('con el teclado: Mayús+F10 y la tecla de menú abren las opciones del resultado resaltado', async ({
  page,
  request
}) => {
  const list = await createPlaylist(request, 'Domingo')
  await open(page, { lists: 1 })
  await typeText(page, 'new wine', ['A Una Voz', 'Shekinah'])
  const highlighted = page.locator('.sr-row.on .sr-title')

  // la segunda: se baja a ella con la flecha y Mayús+F10 abre sus opciones
  const first = await highlighted.innerText()
  await box(page).press('ArrowDown')
  await expect(highlighted).not.toHaveText(first)
  const second = await highlighted.innerText()
  await box(page).press('Shift+F10')
  await expect(menuOf(page, second)).toBeVisible()
  await expect(page.getByRole('menuitem').first()).toBeFocused()
  // Escape cierra el menú, el foco vuelve a la caja y el desplegable sigue
  await page.keyboard.press('Escape')
  await expect(menu(page)).toHaveCount(0)
  await expect(box(page)).toBeFocused()
  await expect(dropdown(page)).toBeVisible()

  // la tecla de menú, sobre la primera; y se elige con el teclado
  await box(page).press('ArrowUp')
  await expect(highlighted).toHaveText(first)
  await box(page).press('ContextMenu')
  await expect(menuOf(page, first)).toBeVisible()
  await menuItem(page, 'Añadir a una lista').focus()
  await page.keyboard.press('Enter')
  await expect(menu(page)).toHaveAccessibleName('Añadir a una lista')
  await expect(page.getByRole('menuitem').first()).toBeFocused()
  await page.keyboard.press('Enter')
  await expect(menu(page)).toHaveCount(0)
  await expect.poll(() => titlesOf(request, list)).toEqual([first])
  await expect(box(page)).toBeFocused()
  await expect(dropdown(page)).toBeVisible()
})

test('«Nueva lista…» desde un resultado crea el repertorio con esa canción', async ({
  page,
  request
}) => {
  await open(page)
  await typeText(page, 'shekinah', ['Shekinah'])
  await result(page, 'Shekinah').click({ button: 'right' })
  await expect(menuOf(page, 'Shekinah')).toBeVisible()
  await openPlaylists(page)
  await menuItem(page, 'Nueva lista…').click()
  const dialog = page.getByRole('dialog', { name: 'Nueva lista' })
  await expect(dialog).toBeVisible()
  await expect(dialog.getByRole('textbox')).toBeFocused()
  await page.keyboard.type('Nueva' + SUFFIX)
  await page.keyboard.press('Enter')
  await expect(dialog).toHaveCount(0)
  await expect(page.locator('.nav-playlist', { hasText: 'Nueva e2e' })).toBeVisible()
  const { playlists } = await core(request, 'GET', '/playlists')
  const updated = playlists.find((l) => l.name === 'Nueva' + SUFFIX)
  expect(updated, 'el núcleo tiene el repertorio nuevo').toBeTruthy()
  expect(await titlesOf(request, updated.id)).toEqual(['Shekinah'])
})

test('«Reproducir» en el menú de un resultado pone esa canción y deja lo encontrado como cola', async ({
  page
}) => {
  await open(page)
  await typeText(page, 'barak', ['Mi Gozo', 'Sera Llena La Tierra'])
  const [first, second] = await page.locator('.sr-row .sr-title').allTextContents()

  await result(page, first).click({ button: 'right' })
  await expect(menuOf(page, first)).toBeVisible()
  await menuItem(page, 'Reproducir').click()
  await expect(menu(page)).toHaveCount(0)
  await expect(page.locator('.player .title')).toHaveText(first)
  // «siguiente» recorre lo encontrado, no la lista de Favoritos (que está vacía)
  await page
    .locator('.player')
    .getByRole('button', { name: /^Siguiente/ })
    .click()
  await expect(page.locator('.player .title')).toHaveText(second)
})

// ------------------------------------------------- varios resultados a la vez
// El botón «Seleccionar» del desplegable (casillas), Ctrl/Mayús + clic y, desde
// la caja, Ctrl+Espacio y Mayús+flechas. Con alguno elegido la cabecera es una
// barra pequeña: cuántos, «Añadir a una lista ▾», «⋯» y «Ninguno».
const ALL = ['Mi Gozo', 'Sera Llena La Tierra', 'Shekinah', 'A Una Voz', 'Que Se Abra El Cielo']
/** Los títulos de los resultados elegidos, en el orden en que se ven. */
const picked = (page) => page.locator('.sr-row.picked .sr-title')
const count = (page) => page.locator('.search-results .sr-count')
const buttonOf = (page, name) =>
  page.locator('.search-results .sr-head').getByRole('button', { name, exact: true })

test('con «Seleccionar» y las casillas, dos resultados a un repertorio de una vez, sin cerrar el desplegable', async ({
  page,
  request
}) => {
  const sunday = await createPlaylist(request, 'Domingo')
  await open(page, { lists: 1 })
  await typeText(page, 'a', ALL)

  const mode = buttonOf(page, 'Seleccionar resultados')
  await expect(mode).toHaveAttribute('aria-pressed', 'false')
  await mode.click()
  await expect(mode).toHaveAttribute('aria-pressed', 'true')
  await expect(page.locator('.sr-row .pick-box')).toHaveCount(5)
  // el clic en una fila la marca (no abre su ficha ni cierra), y el foco sigue en la caja
  await result(page, 'Shekinah').click()
  await result(page, 'Mi Gozo').locator('.pick-box').click()
  await expect(count(page)).toHaveText('2 seleccionados')
  await expect(box(page)).toBeFocused()
  const order = await picked(page).allTextContents()
  expect([...order].sort()).toEqual(['Mi Gozo', 'Shekinah'])

  await buttonOf(page, 'Añadir a una lista').click()
  await expect(menu(page)).toHaveAccessibleName('Añadir 2 a una lista')
  await pickPlaylist(page, 'Domingo')
  await expect.poll(() => titlesOf(request, sunday)).toEqual(['Mi Gozo', 'Shekinah'])
  await expect(page.locator('.toast', { hasText: '2 añadidas a «Domingo e2e»' })).toBeVisible()
  // el desplegable sigue, con lo elegido, y lo que se escriba llega a la caja
  await expect(dropdown(page)).toBeVisible()
  await expect(picked(page)).toHaveText(order)
  await expect(box(page)).toBeFocused()

  // «Ninguno» suelta lo elegido; el modo sigue hasta quitarlo
  await buttonOf(page, 'No seleccionar ninguno').click()
  await expect(picked(page)).toHaveCount(0)
  await expect(mode).toHaveAttribute('aria-pressed', 'true')
  await mode.click()
  await expect(page.locator('.sr-row .pick-box')).toHaveCount(0)
})

test('Ctrl+clic y Mayús+clic sin el modo; la papelera desde «⋯» pide confirmación y, cancelada, no borra', async ({
  page,
  request
}) => {
  await open(page)
  await typeText(page, 'a', ALL)
  const rows = await page.locator('.sr-row .sr-title').allTextContents()

  await result(page, rows[0]).click({ modifiers: ['ControlOrMeta'] })
  await result(page, rows[2]).click({ modifiers: ['Shift'] })
  await expect(picked(page)).toHaveText(rows.slice(0, 3))
  await expect(count(page)).toHaveText('3 seleccionados')
  // el clic derecho sobre una elegida: las opciones de las tres
  await result(page, rows[1]).click({ button: 'right' })
  await expect(page.getByRole('menu', { name: '3 canciones' })).toBeVisible()
  await expect(menuItem(page, /^Quitar .* de esta lista/)).toHaveCount(0)
  await page.keyboard.press('Escape')
  await expect(menu(page)).toHaveCount(0)

  await buttonOf(page, 'Más opciones').click()
  await expect(page.getByRole('menu', { name: '3 canciones' })).toBeVisible()
  await menuItem(page, 'Mandar 3 a la papelera…').click()
  const confirmDialog = page.getByRole('dialog', { name: 'Mandar 3 canciones a la papelera' })
  await expect(confirmDialog).toBeVisible()
  await confirmDialog.getByRole('button', { name: 'Cancelar' }).click()
  await expect(confirmDialog).toHaveCount(0)
  // nada se ha borrado: ni en el núcleo ni en el desplegable, y lo elegido sigue
  const { songs } = await core(request, 'GET', '/search?q=a&limit=60')
  expect(songs.map((s) => s.title).sort()).toEqual([...ALL].sort())
  await expect(dropdown(page)).toBeVisible()
  await expect(page.locator('.sr-row')).toHaveCount(5)
  await expect(picked(page)).toHaveCount(3)

  // Escape suelta lo elegido; otro Escape cierra el desplegable
  await box(page).press('Escape')
  await expect(picked(page)).toHaveCount(0)
  await expect(dropdown(page)).toBeVisible()
  await box(page).press('Escape')
  await expect(dropdown(page)).toHaveCount(0)
})

test('desde la caja: Ctrl+A es del texto; Ctrl+Espacio y Mayús+flechas eligen, y «Reproducir estas 2» las pone de cola', async ({
  page
}) => {
  await open(page)
  await typeText(page, 'a', ALL)
  const rows = await page.locator('.sr-row .sr-title').allTextContents()

  // Ctrl+A selecciona lo escrito, no los resultados
  await box(page).press('ControlOrMeta+a')
  expect(
    await box(page).evaluate((el) => [el.selectionStart, el.selectionEnd, el.value.length])
  ).toEqual([0, 1, 1])
  await expect(picked(page)).toHaveCount(0)
  await box(page).press('End')

  await box(page).press('ArrowDown') // la segunda
  await box(page).press('Control+Space')
  await expect(picked(page)).toHaveText([rows[1]])
  await box(page).press('Shift+ArrowDown')
  await expect(picked(page)).toHaveText([rows[1], rows[2]])
  await expect(box(page)).toHaveValue('a')

  // la tecla de menú sobre la resaltada (elegida): las opciones de las dos
  await box(page).press('ContextMenu')
  await expect(page.getByRole('menu', { name: '2 canciones' })).toBeVisible()
  await menuItem(page, 'Reproducir estas 2').click()
  await expect(page.locator('.player .title')).toHaveText(rows[1])
  // «siguiente» recorre las elegidas: la cola son ellas
  await page
    .locator('.player')
    .getByRole('button', { name: /^Siguiente/ })
    .click()
  await expect(page.locator('.player .title')).toHaveText(rows[2])
})
