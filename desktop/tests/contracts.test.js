import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import {
  api,
  mediaUrl,
  mediaUrlAlt,
  COVER_SIZES,
  errorMessage,
  ApiError,
  fromBridge
} from '../src/api.js'
import { createState, buildApiDouble } from './support/backend.js'
import { coreSource } from './support/core.js'

// Cinco fallos de la interfaz eran el mismo: un nombre que dejó de existir al
// pasar el código a inglés y que nadie comparó con el núcleo. Estas pruebas
// comparan los dos lados de verdad.
const HERE = dirname(fileURLToPath(import.meta.url))
const SRC = join(HERE, '..', 'src')
const read = (...p) => readFileSync(join(...p), 'utf8')

describe('the test double covers the real api', () => {
  it('no method is left unimplemented', () => {
    const double = buildApiDouble({ api }, createState())
    const missing = Object.keys(api).filter((k) => !(k in double))
    expect(missing, 'metodos del api que el doble no conoce').toEqual([])
  })

  it('and there is no extra made-up one', () => {
    const double = buildApiDouble({ api }, createState())
    const extra = Object.keys(double).filter((k) => !(k in api))
    expect(extra, 'el doble ofrece cosas que el api no tiene').toEqual([])
  })
})

describe('the names the UI sends to the core', () => {
  // que las calidades que ofrece Ajustes son las que entiende el nucleo se
  // prueba eligiendolas de verdad (settings.test.js)

  // que «conservar el original» viaja como `keep` (y no como `keepOne`, que
  // el nucleo ignoraba) se prueba pulsando «Convertir ahora» (settings.test.js)

  it('the AI status is read with the name the API returns', () => {
    // api.py devuelve "ai"; se leía status.ia, así que la insignia decía
    // siempre «sin clave»
    for (const file of ['components/SettingsPage.vue', 'App.vue']) {
      expect(read(SRC, file), `${file} lee status.ia`).not.toMatch(/status\??\.ia\b/)
    }
    expect(coreSource()).toMatch(/"ai":/)
  })

  it('the actions when adding a folder are the API ones', () => {
    const helper = read(SRC, 'utils/folders.js')
    for (const action of ['added', 'already_there', 'replaced', 'confirm']) {
      expect(helper, `falta la acción «${action}»`).toContain(action)
    }
    const core = coreSource()
    for (const action of ['already_there', 'replaced', 'confirm']) {
      expect(core, `el núcleo no devuelve «${action}»`).toContain(`"${action}"`)
    }
    // y nadie compara ya con los nombres viejos
    for (const legacy of ['confirmar', 'ya_estaba', 'reemplaza']) {
      expect(read(SRC, 'components/SettingsPage.vue')).not.toContain(`'${legacy}'`)
    }
  })

  it('the editable fields of a song are accepted by the core', () => {
    const core = coreSource()
    expect(core, 'la API deberia limitar que campos se editan').toMatch(/EDITABLE|SongEdit/)
  })
})

describe('audio and cover URLs', () => {
  it('outside the app they are plain API paths', () => {
    expect(mediaUrl('audio', 7)).toBe('/api/song/7/audio')
    expect(mediaUrl('cover', 7)).toBe('/api/song/7/cover')
    expect(mediaUrlAlt('cover', 7)).toBe(null)
  })

  it('the thumbnail is requested at a size the core can cache', () => {
    for (const size of COVER_SIZES) {
      expect(api.coverUrl(7, size)).toContain(`size=${size}`)
    }
    // un tamaño raro no se manda: el núcleo solo cachea los suyos
    expect(api.coverUrl(7, 137)).not.toContain('size=')
    expect(api.coverUrl(7)).not.toContain('size=')
  })

  it('the core accepts those same sizes', () => {
    const core = coreSource()
    for (const size of COVER_SIZES) {
      expect(core, `el núcleo no genera miniaturas de ${size}`).toContain(String(size))
    }
  })
})

describe('errors are told in a single sentence', () => {
  it('without the «Error:» the browser adds', () => {
    expect(errorMessage(new Error('no existe'))).toBe('no existe')
    expect(errorMessage(new ApiError('404: no existe', 404))).toBe('404: no existe')
    expect(errorMessage('algo suelto')).toBe('algo suelto')
  })

  // Rust reenvia el fallo del nucleo como `<codigo>: <cuerpo JSON>`. En el
  // chat se leia «400: {"detail":"hace falta algo que descargar"}».
  it('what comes through the Rust bridge keeps the detail', () => {
    const e = fromBridge(new Error('409: {"detail":"ya hay una descarga en marcha"}'))
    expect(e).toBeInstanceOf(ApiError)
    expect(e.message).toBe('ya hay una descarga en marcha')
    expect(e.status).toBe(409)
    // sin JSON, el texto tal cual; sin codigo, el mensaje entero
    expect(fromBridge('500: se rompio').message).toBe('se rompio')
    expect(fromBridge('el nucleo no contesta').message).toBe('el nucleo no contesta')
  })
})
