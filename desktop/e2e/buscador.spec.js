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
const SUFIJO = ' e2e'

test.beforeEach(async ({ request }) => {
  await reset(request)
})

test.afterEach(async ({ request }) => {
  // El núcleo y su biblioteca son de todas las pruebas (un solo worker): lo que
  // crea esta, lo deshace esta. `reset` quita los repertorios al empezar, no al
  // acabar, y las favoritas no las toca.
  const { playlists } = await core(request, 'GET', '/playlists')
  for (const l of playlists.filter((p) => p.name.endsWith(SUFIJO))) {
    await core(request, 'DELETE', `/playlists/${l.id}`)
  }
  const { songs } = await core(request, 'GET', '/search?only_favorites=true&limit=100')
  for (const s of songs) {
    await core(request, 'POST', `/song/${s.id}/favorite`, { favorite: false })
  }
})

/** Un repertorio nuevo, por la API. Antes de abrir la app: lee las listas al arrancar. */
async function crearLista(request, name) {
  const { id } = await core(request, 'POST', '/playlists', { name: name + SUFIJO })
  return id
}

/**
 * Los títulos que tiene un repertorio, según el núcleo, ordenados: lo que se
 * prueba aquí es QUÉ canciones tiene, no en qué orden las guarda.
 */
async function titulosDe(request, id) {
  const { songs } = await core(request, 'GET', `/playlists/${id}/songs`)
  return songs.map((s) => s.title).sort()
}

/** ¿Es favorita, según el núcleo? */
async function esFavorita(request, id) {
  return Boolean((await core(request, 'GET', `/song/${id}`)).favorite)
}

const caja = (page) => page.getByRole('combobox', { name: 'Buscar en la biblioteca' })
const desplegable = (page) =>
  page.getByRole('listbox', { name: 'Resultados en toda la biblioteca' })
/** La fila del desplegable de esa canción. */
const resultado = (page, title) => page.locator('.sr-row').filter({ hasText: title })
/** El «⋯» de la fila de esa canción. */
const mas = (page, title) => resultado(page, title).locator('.sr-more')
const menu = (page) => page.getByRole('menu')
/** El menú de opciones de esa canción: su cabecera es el título. */
const menuDe = (page, title) => page.getByRole('menu', { name: title, exact: true })
/** Una opción del menú abierto: por su texto entero, o por una expresión. */
const opcion = (page, name) => page.getByRole('menuitem', { name, exact: typeof name === 'string' })

/**
 * Abre la app en una vista donde el buscador NO filtra la lista: Favoritos, o
 * el repertorio que se diga. Espera a que el lateral tenga ya los `listas`
 * repertorios que hay: el menú de una canción ofrece los que conoce al abrirse.
 */
async function abrir(page, { lista = '', listas = 0 } = {}) {
  await openApp(page)
  await expect(page.locator('.nav-playlist')).toHaveCount(listas)
  if (lista) {
    await page.locator('.nav-playlist', { hasText: lista }).click()
    await expect(page.locator('.sidebar .nav-playlist.active')).toContainText(lista)
  } else {
    await go(page, 'Favoritos')
    await expect(page.locator('.sidebar .nav-link.active')).toContainText('Favoritos')
  }
}

/** Escribe en la caja de búsqueda y espera al desplegable con exactamente esos resultados. */
async function escribir(page, texto, esperados) {
  await caja(page).fill(texto)
  await expect(desplegable(page)).toBeVisible()
  await expect
    .poll(async () => (await page.locator('.sr-row .sr-title').allTextContents()).sort())
    .toEqual([...esperados].sort())
}

/**
 * Abre el menú de un resultado (clic derecho) y comprueba que ofrece esa opción.
 *
 * Tras cambiar algo desde el menú (una favorita) el núcleo ya lo tiene, pero la
 * app puede no haber recibido aún su respuesta: el menú se monta con lo que
 * sabe en ese momento y no se corrige solo. Si no la ofrece, se cierra y se
 * vuelve a abrir, que es lo que haría quien lo mirase.
 */
async function abrirMenuCon(page, title, option) {
  await expect(async () => {
    await resultado(page, title).click({ button: 'right' })
    await expect(menuDe(page, title)).toBeVisible()
    const ofrece = await opcion(page, option).isVisible()
    if (!ofrece) {
      await page.keyboard.press('Escape')
      await expect(menu(page)).toHaveCount(0)
    }
    expect(ofrece, `el menú de «${title}» no ofrece «${option}»`).toBe(true)
  }).toPass({ timeout: 15_000 })
}

/** Del menú abierto de una canción: «Añadir a una lista». */
async function abrirListas(page) {
  await opcion(page, 'Añadir a una lista').click()
  await expect(menu(page)).toHaveAccessibleName('Añadir a una lista')
}

/** Dentro de «Añadir a una lista»: ese repertorio. El menú se cierra, el desplegable no. */
async function elegirLista(page, lista) {
  await opcion(page, new RegExp(`^${lista}`)).click()
  await expect(menu(page)).toHaveCount(0)
}

test('en Favoritos, un resultado se añade a un repertorio con el clic derecho y con el «⋯», varias veces seguidas', async ({
  page,
  request
}) => {
  const domingo = await crearLista(request, 'Domingo')
  const ensayo = await crearLista(request, 'Ensayo')
  await abrir(page, { listas: 2 })
  await escribir(page, 'barak', ['Mi Gozo', 'Sera Llena La Tierra'])

  // clic derecho en un resultado: sus opciones, y a un repertorio
  await resultado(page, 'Mi Gozo').click({ button: 'right' })
  await expect(menuDe(page, 'Mi Gozo')).toBeVisible()
  await abrirListas(page)
  await elegirLista(page, 'Domingo')
  await expect.poll(() => titulosDe(request, domingo)).toEqual(['Mi Gozo'])
  await expect(page.locator('.toast', { hasText: 'Añadida a «Domingo e2e»' })).toBeVisible()
  // el desplegable no se ha cerrado: se puede seguir con lo encontrado
  await expect(desplegable(page)).toBeVisible()
  await expect(page.locator('.sr-row')).toHaveCount(2)

  // el «⋯» de otro resultado, a otro repertorio. El menú ofrece los repertorios
  // que conoce al abrirse: se espera a que la app ya cuente la canción nueva.
  await expect(page.locator('.nav-playlist', { hasText: 'Domingo' }).locator('.count')).toHaveText(
    '1'
  )
  await mas(page, 'Sera Llena La Tierra').click()
  await expect(menuDe(page, 'Sera Llena La Tierra')).toBeVisible()
  await abrirListas(page)
  // el menú ya sabe que el primer repertorio tiene una canción
  await expect(opcion(page, /^Domingo/).locator('.ctx-note')).toHaveText('1')
  await elegirLista(page, 'Ensayo')
  await expect.poll(() => titulosDe(request, ensayo)).toEqual(['Sera Llena La Tierra'])
  expect(await titulosDe(request, domingo), 'la primera sigue donde estaba').toEqual(['Mi Gozo'])
  await expect(desplegable(page)).toBeVisible()
  await expect(page.locator('.sr-row')).toHaveCount(2)

  // y una tercera vez, la primera a la otra lista: nada se ha quedado a medias
  await resultado(page, 'Mi Gozo').click({ button: 'right' })
  await expect(menuDe(page, 'Mi Gozo')).toBeVisible()
  await abrirListas(page)
  await elegirLista(page, 'Ensayo')
  await expect.poll(() => titulosDe(request, ensayo)).toEqual(['Mi Gozo', 'Sera Llena La Tierra'])
  await expect(page.locator('.nav-playlist', { hasText: 'Ensayo' }).locator('.count')).toHaveText(
    '2'
  )
  await expect(desplegable(page)).toBeVisible()
  await expect(page.locator('.sr-row')).toHaveCount(2)
})

test('el «⋯» de cada resultado se ve en el resaltado y al pasar el puntero', async ({ page }) => {
  await abrir(page)
  // el puntero, lejos del desplegable: ninguna fila lo tiene encima
  await page.mouse.move(700, 600)
  await escribir(page, 'barak', ['Mi Gozo', 'Sera Llena La Tierra'])
  // Por título y no por «la resaltada»: al pasar el puntero por la otra, la
  // resaltada pasa a ser ella.
  const [primera, segunda] = await page.locator('.sr-row .sr-title').allTextContents()
  const masDe = (title) => resultado(page, title).locator('.sr-more')
  await expect(resultado(page, primera)).toHaveClass(/\bon\b/)
  await expect(masDe(primera)).toHaveCSS('opacity', '1')
  await expect(masDe(segunda)).toHaveCSS('opacity', '0')
  await resultado(page, segunda).hover()
  await expect(masDe(segunda)).toHaveCSS('opacity', '1')
  // dentro de un «option» sus hijos son presentacionales: el nombre ya lo dice la
  // opcion, y quien no ve la pantalla llega con la tecla de menú
  await expect(masDe(segunda)).toHaveAttribute('aria-hidden', 'true')
  await expect(masDe(segunda)).toHaveAttribute('title', 'Opciones')
})

test('dentro de un repertorio, el menú de un resultado es el del resultado y no el de la lista', async ({
  page,
  request
}) => {
  const byTitle = await songsByTitle(request)
  const domingo = await crearLista(request, 'Domingo')
  const ensayo = await crearLista(request, 'Ensayo')
  await core(request, 'POST', `/playlists/${domingo}/songs`, { ids: [byTitle['Shekinah'].id] })
  await abrir(page, { lista: 'Domingo', listas: 2 })
  await expect(titles(page)).toHaveText(['Shekinah'])
  // el buscador mira toda la biblioteca, no la lista abierta: New Wine tiene dos
  await escribir(page, 'new wine', ['A Una Voz', 'Shekinah'])

  // la que ya está en la lista abierta: su menú no es el de la fila de la lista
  await resultado(page, 'Shekinah').click({ button: 'right' })
  await expect(menuDe(page, 'Shekinah')).toBeVisible()
  await expect(opcion(page, 'Añadir a una lista')).toBeVisible()
  await expect(opcion(page, 'Quitar de esta lista')).toHaveCount(0)
  // y no ha elegido la fila de detrás
  await expect(page.locator('[data-song-row].selected')).toHaveCount(0)
  await abrirListas(page)
  await elegirLista(page, 'Ensayo')
  await expect.poll(() => titulosDe(request, ensayo)).toEqual(['Shekinah'])
  expect(await titulosDe(request, domingo), 'la lista abierta no pierde nada').toEqual(['Shekinah'])
  await expect(desplegable(page)).toBeVisible()

  // la otra, a la lista que está abierta: también se puede
  await resultado(page, 'A Una Voz').click({ button: 'right' })
  await expect(menuDe(page, 'A Una Voz')).toBeVisible()
  await expect(opcion(page, 'Quitar de esta lista')).toHaveCount(0)
  await abrirListas(page)
  await elegirLista(page, 'Domingo')
  await expect.poll(() => titulosDe(request, domingo)).toEqual(['A Una Voz', 'Shekinah'])
  await expect(page.locator('[data-song-row].selected')).toHaveCount(0)
  await expect(desplegable(page)).toBeVisible()
})

test('con un menú abierto, el clic derecho en otro resultado abre el de ese resultado', async ({
  page,
  request
}) => {
  const lista = await crearLista(request, 'Domingo')
  await abrir(page, { listas: 1 })
  await escribir(page, 'barak', ['Mi Gozo', 'Sera Llena La Tierra'])
  // El menú nace bajo el puntero y tapa lo que queda debajo: se abre en la fila
  // de abajo y se pasa a la de arriba, que queda libre.
  const [arriba, abajo] = await page.locator('.sr-row .sr-title').allTextContents()

  // el menú de una, ya dentro de «Añadir a una lista»…
  await resultado(page, abajo).click({ button: 'right' })
  await expect(menuDe(page, abajo)).toBeVisible()
  await abrirListas(page)
  // …y sin cerrarlo, el clic derecho en la otra
  await resultado(page, arriba).click({ button: 'right' })
  await expect(menuDe(page, arriba)).toBeVisible()
  await expect(menu(page)).toHaveCount(1)
  // es su menú entero, no el submenú de la anterior
  await expect(opcion(page, 'Marcar como favorito')).toBeVisible()
  await expect(desplegable(page)).toBeVisible()
  await abrirListas(page)
  await elegirLista(page, 'Domingo')
  await expect.poll(() => titulosDe(request, lista)).toEqual([arriba])
})

test('Escape con el menú abierto cierra el menú y no el desplegable; un clic fuera lo cierra todo', async ({
  page
}) => {
  await abrir(page)
  await escribir(page, 'barak', ['Mi Gozo', 'Sera Llena La Tierra'])

  // Escape con el menú abierto: se va el menú, el desplegable se queda
  await resultado(page, 'Mi Gozo').click({ button: 'right' })
  await expect(menuDe(page, 'Mi Gozo')).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(menu(page)).toHaveCount(0)
  await expect(desplegable(page)).toBeVisible()
  await expect(page.locator('.sr-row')).toHaveCount(2)

  // dentro de «Añadir a una lista», Escape vuelve al menú, y otro lo cierra
  await resultado(page, 'Mi Gozo').click({ button: 'right' })
  await abrirListas(page)
  await page.keyboard.press('Escape')
  await expect(menuDe(page, 'Mi Gozo')).toBeVisible()
  await expect(desplegable(page)).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(menu(page)).toHaveCount(0)
  await expect(desplegable(page)).toBeVisible()

  // con el menú ya cerrado, Escape es del desplegable (lo de siempre)
  await page.keyboard.press('Escape')
  await expect(desplegable(page)).toHaveCount(0)

  // un clic en cualquier otro sitio cierra el menú y el desplegable a la vez
  await escribir(page, 'new wine', ['A Una Voz', 'Shekinah'])
  await resultado(page, 'Shekinah').click({ button: 'right' })
  await expect(menuDe(page, 'Shekinah')).toBeVisible()
  await page.locator('.brand').click()
  await expect(menu(page)).toHaveCount(0)
  await expect(desplegable(page)).toHaveCount(0)
})

test('marcar una favorita desde el menú de un resultado cambia lo que dice el menú la próxima vez', async ({
  page,
  request
}) => {
  const byTitle = await songsByTitle(request)
  const miGozo = byTitle['Mi Gozo'].id
  const sera = byTitle['Sera Llena La Tierra'].id
  await abrir(page)
  await escribir(page, 'barak', ['Mi Gozo', 'Sera Llena La Tierra'])

  // no es favorita: el menú ofrece marcarla
  await mas(page, 'Mi Gozo').click()
  await expect(menuDe(page, 'Mi Gozo')).toBeVisible()
  await expect(opcion(page, 'Quitar de favoritos')).toHaveCount(0)
  await opcion(page, 'Marcar como favorito').click()
  await expect(menu(page)).toHaveCount(0)
  await expect.poll(() => esFavorita(request, miGozo)).toBe(true)
  // el desplegable sigue ahí, y la otra no ha cambiado
  await expect(desplegable(page)).toBeVisible()
  expect(await esFavorita(request, sera)).toBe(false)

  // la próxima vez el menú dice lo contrario (y es el mismo con el clic derecho)
  await abrirMenuCon(page, 'Mi Gozo', 'Quitar de favoritos')
  await expect(opcion(page, 'Marcar como favorito')).toHaveCount(0)
  await opcion(page, 'Quitar de favoritos').click()
  await expect(menu(page)).toHaveCount(0)
  await expect.poll(() => esFavorita(request, miGozo)).toBe(false)

  // y otra vez al revés
  await abrirMenuCon(page, 'Mi Gozo', 'Marcar como favorito')
  await expect(opcion(page, 'Quitar de favoritos')).toHaveCount(0)
  await expect(desplegable(page)).toBeVisible()
})

test('con el teclado: Mayús+F10 y la tecla de menú abren las opciones del resultado resaltado', async ({
  page,
  request
}) => {
  const lista = await crearLista(request, 'Domingo')
  await abrir(page, { listas: 1 })
  await escribir(page, 'new wine', ['A Una Voz', 'Shekinah'])
  const resaltada = page.locator('.sr-row.on .sr-title')

  // la segunda: se baja a ella con la flecha y Mayús+F10 abre sus opciones
  const primera = await resaltada.innerText()
  await caja(page).press('ArrowDown')
  await expect(resaltada).not.toHaveText(primera)
  const segunda = await resaltada.innerText()
  await caja(page).press('Shift+F10')
  await expect(menuDe(page, segunda)).toBeVisible()
  await expect(page.getByRole('menuitem').first()).toBeFocused()
  // Escape cierra el menú, el foco vuelve a la caja y el desplegable sigue
  await page.keyboard.press('Escape')
  await expect(menu(page)).toHaveCount(0)
  await expect(caja(page)).toBeFocused()
  await expect(desplegable(page)).toBeVisible()

  // la tecla de menú, sobre la primera; y se elige con el teclado
  await caja(page).press('ArrowUp')
  await expect(resaltada).toHaveText(primera)
  await caja(page).press('ContextMenu')
  await expect(menuDe(page, primera)).toBeVisible()
  await opcion(page, 'Añadir a una lista').focus()
  await page.keyboard.press('Enter')
  await expect(menu(page)).toHaveAccessibleName('Añadir a una lista')
  await expect(page.getByRole('menuitem').first()).toBeFocused()
  await page.keyboard.press('Enter')
  await expect(menu(page)).toHaveCount(0)
  await expect.poll(() => titulosDe(request, lista)).toEqual([primera])
  await expect(caja(page)).toBeFocused()
  await expect(desplegable(page)).toBeVisible()
})

test('«Nueva lista…» desde un resultado crea el repertorio con esa canción', async ({
  page,
  request
}) => {
  await abrir(page)
  await escribir(page, 'shekinah', ['Shekinah'])
  await resultado(page, 'Shekinah').click({ button: 'right' })
  await expect(menuDe(page, 'Shekinah')).toBeVisible()
  await abrirListas(page)
  await opcion(page, 'Nueva lista…').click()
  const dialog = page.getByRole('dialog', { name: 'Nueva lista' })
  await expect(dialog).toBeVisible()
  await expect(dialog.getByRole('textbox')).toBeFocused()
  await page.keyboard.type('Nueva' + SUFIJO)
  await page.keyboard.press('Enter')
  await expect(dialog).toHaveCount(0)
  await expect(page.locator('.nav-playlist', { hasText: 'Nueva e2e' })).toBeVisible()
  const { playlists } = await core(request, 'GET', '/playlists')
  const nueva = playlists.find((l) => l.name === 'Nueva' + SUFIJO)
  expect(nueva, 'el núcleo tiene el repertorio nuevo').toBeTruthy()
  expect(await titulosDe(request, nueva.id)).toEqual(['Shekinah'])
})

test('«Reproducir» en el menú de un resultado pone esa canción y deja lo encontrado como cola', async ({
  page
}) => {
  await abrir(page)
  await escribir(page, 'barak', ['Mi Gozo', 'Sera Llena La Tierra'])
  const [primera, segunda] = await page.locator('.sr-row .sr-title').allTextContents()

  await resultado(page, primera).click({ button: 'right' })
  await expect(menuDe(page, primera)).toBeVisible()
  await opcion(page, 'Reproducir').click()
  await expect(menu(page)).toHaveCount(0)
  await expect(page.locator('.player .title')).toHaveText(primera)
  // «siguiente» recorre lo encontrado, no la lista de Favoritos (que está vacía)
  await page
    .locator('.player')
    .getByRole('button', { name: /^Siguiente/ })
    .click()
  await expect(page.locator('.player .title')).toHaveText(segunda)
})

// ------------------------------------------------- varios resultados a la vez
// El botón «Seleccionar» del desplegable (casillas), Ctrl/Mayús + clic y, desde
// la caja, Ctrl+Espacio y Mayús+flechas. Con alguno elegido la cabecera es una
// barra pequeña: cuántos, «Añadir a una lista ▾», «⋯» y «Ninguno».
const TODAS = ['Mi Gozo', 'Sera Llena La Tierra', 'Shekinah', 'A Una Voz', 'Que Se Abra El Cielo']
/** Los títulos de los resultados elegidos, en el orden en que se ven. */
const elegidos = (page) => page.locator('.sr-row.picked .sr-title')
const cuenta = (page) => page.locator('.search-results .sr-count')
const botonDe = (page, name) =>
  page.locator('.search-results .sr-head').getByRole('button', { name, exact: true })

test('con «Seleccionar» y las casillas, dos resultados a un repertorio de una vez, sin cerrar el desplegable', async ({
  page,
  request
}) => {
  const domingo = await crearLista(request, 'Domingo')
  await abrir(page, { listas: 1 })
  await escribir(page, 'a', TODAS)

  const modo = botonDe(page, 'Seleccionar resultados')
  await expect(modo).toHaveAttribute('aria-pressed', 'false')
  await modo.click()
  await expect(modo).toHaveAttribute('aria-pressed', 'true')
  await expect(page.locator('.sr-row .pick-box')).toHaveCount(5)
  // el clic en una fila la marca (no abre su ficha ni cierra), y el foco sigue en la caja
  await resultado(page, 'Shekinah').click()
  await resultado(page, 'Mi Gozo').locator('.pick-box').click()
  await expect(cuenta(page)).toHaveText('2 seleccionados')
  await expect(caja(page)).toBeFocused()
  const orden = await elegidos(page).allTextContents()
  expect([...orden].sort()).toEqual(['Mi Gozo', 'Shekinah'])

  await botonDe(page, 'Añadir a una lista').click()
  await expect(menu(page)).toHaveAccessibleName('Añadir 2 a una lista')
  await elegirLista(page, 'Domingo')
  await expect.poll(() => titulosDe(request, domingo)).toEqual(['Mi Gozo', 'Shekinah'])
  await expect(page.locator('.toast', { hasText: '2 añadidas a «Domingo e2e»' })).toBeVisible()
  // el desplegable sigue, con lo elegido, y lo que se escriba llega a la caja
  await expect(desplegable(page)).toBeVisible()
  await expect(elegidos(page)).toHaveText(orden)
  await expect(caja(page)).toBeFocused()

  // «Ninguno» suelta lo elegido; el modo sigue hasta quitarlo
  await botonDe(page, 'No seleccionar ninguno').click()
  await expect(elegidos(page)).toHaveCount(0)
  await expect(modo).toHaveAttribute('aria-pressed', 'true')
  await modo.click()
  await expect(page.locator('.sr-row .pick-box')).toHaveCount(0)
})

test('Ctrl+clic y Mayús+clic sin el modo; la papelera desde «⋯» pide confirmación y, cancelada, no borra', async ({
  page,
  request
}) => {
  await abrir(page)
  await escribir(page, 'a', TODAS)
  const filas = await page.locator('.sr-row .sr-title').allTextContents()

  await resultado(page, filas[0]).click({ modifiers: ['ControlOrMeta'] })
  await resultado(page, filas[2]).click({ modifiers: ['Shift'] })
  await expect(elegidos(page)).toHaveText(filas.slice(0, 3))
  await expect(cuenta(page)).toHaveText('3 seleccionados')
  // el clic derecho sobre una elegida: las opciones de las tres
  await resultado(page, filas[1]).click({ button: 'right' })
  await expect(page.getByRole('menu', { name: '3 canciones' })).toBeVisible()
  await expect(opcion(page, /^Quitar .* de esta lista/)).toHaveCount(0)
  await page.keyboard.press('Escape')
  await expect(menu(page)).toHaveCount(0)

  await botonDe(page, 'Más opciones').click()
  await expect(page.getByRole('menu', { name: '3 canciones' })).toBeVisible()
  await opcion(page, 'Mandar 3 a la papelera…').click()
  const dialogo = page.getByRole('dialog', { name: 'Mandar 3 canciones a la papelera' })
  await expect(dialogo).toBeVisible()
  await dialogo.getByRole('button', { name: 'Cancelar' }).click()
  await expect(dialogo).toHaveCount(0)
  // nada se ha borrado: ni en el núcleo ni en el desplegable, y lo elegido sigue
  const { songs } = await core(request, 'GET', '/search?q=a&limit=60')
  expect(songs.map((s) => s.title).sort()).toEqual([...TODAS].sort())
  await expect(desplegable(page)).toBeVisible()
  await expect(page.locator('.sr-row')).toHaveCount(5)
  await expect(elegidos(page)).toHaveCount(3)

  // Escape suelta lo elegido; otro Escape cierra el desplegable
  await caja(page).press('Escape')
  await expect(elegidos(page)).toHaveCount(0)
  await expect(desplegable(page)).toBeVisible()
  await caja(page).press('Escape')
  await expect(desplegable(page)).toHaveCount(0)
})

test('desde la caja: Ctrl+A es del texto; Ctrl+Espacio y Mayús+flechas eligen, y «Reproducir estas 2» las pone de cola', async ({
  page
}) => {
  await abrir(page)
  await escribir(page, 'a', TODAS)
  const filas = await page.locator('.sr-row .sr-title').allTextContents()

  // Ctrl+A selecciona lo escrito, no los resultados
  await caja(page).press('ControlOrMeta+a')
  expect(
    await caja(page).evaluate((el) => [el.selectionStart, el.selectionEnd, el.value.length])
  ).toEqual([0, 1, 1])
  await expect(elegidos(page)).toHaveCount(0)
  await caja(page).press('End')

  await caja(page).press('ArrowDown') // la segunda
  await caja(page).press('Control+Space')
  await expect(elegidos(page)).toHaveText([filas[1]])
  await caja(page).press('Shift+ArrowDown')
  await expect(elegidos(page)).toHaveText([filas[1], filas[2]])
  await expect(caja(page)).toHaveValue('a')

  // la tecla de menú sobre la resaltada (elegida): las opciones de las dos
  await caja(page).press('ContextMenu')
  await expect(page.getByRole('menu', { name: '2 canciones' })).toBeVisible()
  await opcion(page, 'Reproducir estas 2').click()
  await expect(page.locator('.player .title')).toHaveText(filas[1])
  // «siguiente» recorre las elegidas: la cola son ellas
  await page
    .locator('.player')
    .getByRole('button', { name: /^Siguiente/ })
    .click()
  await expect(page.locator('.player .title')).toHaveText(filas[2])
})
