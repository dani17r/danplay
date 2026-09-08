import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { api, mediaUrl, mediaUrlAlt, COVER_SIZES, errorMessage, ApiError } from '../src/api.js'
import { createState, buildApiDouble } from './support/backend.js'

// Cinco fallos de la interfaz eran el mismo: un nombre que dejó de existir al
// pasar el código a inglés y que nadie comparó con el núcleo. Estas pruebas
// comparan los dos lados de verdad.
const HERE = dirname(fileURLToPath(import.meta.url))
const CORE = join(HERE, '..', '..', 'danplay')
const SRC = join(HERE, '..', 'src')
const read = (...p) => readFileSync(join(...p), 'utf8')

describe('el doble de las pruebas cubre el api de verdad', () => {
  it('no se queda ningun metodo sin programar', () => {
    const double = buildApiDouble({ api }, createState())
    const faltan = Object.keys(api).filter((k) => !(k in double))
    expect(faltan, 'metodos del api que el doble no conoce').toEqual([])
  })

  it('y no sobra ninguno inventado', () => {
    const double = buildApiDouble({ api }, createState())
    const sobran = Object.keys(double).filter((k) => !(k in api))
    expect(sobran, 'el doble ofrece cosas que el api no tiene').toEqual([])
  })
})

describe('los nombres que la interfaz manda al nucleo', () => {
  it('la calidad de conversion es la que entiende Python', () => {
    // config.py: MP3_QUALITY = high | medium | variable. Ajustes ofrecía
    // «alta» y «media», que el núcleo no conoce: elegir Media acababa en 320k
    // sin decir nada.
    const settings = read(SRC, 'components/SettingsPage.vue')
    const ofrecidas = [...settings.matchAll(/\{v:\s*'(\w+)'\s*,\s*n:\s*'(?:Alta|Media|Variable)'/g)].map(
      (m) => m[1]
    )
    expect(ofrecidas.length, 'no encuentro el selector de calidad').toBeGreaterThan(1)
    const core = read(CORE, 'convert.py')
    for (const q of ofrecidas) {
      expect(core, `el núcleo no conoce la calidad «${q}»`).toMatch(new RegExp(`"${q}"`))
    }
  })

  it('conservar el original se manda con el nombre que la API lee', () => {
    // api.py: body.get("keep"). Se mandaba «keepOne», así que la casilla no
    // hacía nada y los archivos de partida se borraban igual.
    const settings = read(SRC, 'components/SettingsPage.vue')
    const llamada = settings.match(/api\.convert\(\{[\s\S]{0,200}?\}\)/)
    expect(llamada, 'no encuentro la llamada a convertir').toBeTruthy()
    expect(llamada[0]).toMatch(/\bkeep:/)
    expect(llamada[0], 'keepOne no existe en la API').not.toMatch(/keepOne/)
  })

  it('el estado de la IA se lee con el nombre que devuelve la API', () => {
    // api.py devuelve "ai"; se leía status.ia, así que la insignia decía
    // siempre «sin clave»
    for (const file of ['components/SettingsPage.vue', 'App.vue']) {
      expect(read(SRC, file), `${file} lee status.ia`).not.toMatch(/status\??\.ia\b/)
    }
    expect(read(CORE, 'api.py')).toMatch(/"ai":/)
  })

  it('las acciones al añadir una carpeta son las de la API', () => {
    const helper = read(SRC, 'utils/folders.js')
    for (const action of ['added', 'already_there', 'replaced', 'confirm']) {
      expect(helper, `falta la acción «${action}»`).toContain(action)
    }
    const core = read(CORE, 'api.py')
    for (const action of ['already_there', 'replaced', 'confirm']) {
      expect(core, `el núcleo no devuelve «${action}»`).toContain(`"${action}"`)
    }
    // y nadie compara ya con los nombres viejos
    for (const viejo of ['confirmar', 'ya_estaba', 'reemplaza']) {
      expect(read(SRC, 'components/SettingsPage.vue')).not.toContain(`'${viejo}'`)
    }
  })

  it('los campos que se pueden editar de una cancion los admite el nucleo', () => {
    const core = read(CORE, 'api.py')
    expect(core, 'la API deberia limitar que campos se editan').toMatch(/EDITABLE|SongEdit/)
  })
})

describe('las direcciones de audio y portada', () => {
  it('fuera de la app son rutas normales de la API', () => {
    expect(mediaUrl('audio', 7)).toBe('/api/song/7/audio')
    expect(mediaUrl('cover', 7)).toBe('/api/song/7/cover')
    expect(mediaUrlAlt('cover', 7)).toBe(null)
  })

  it('la miniatura se pide con un tamaño que el nucleo sabe cachear', () => {
    for (const size of COVER_SIZES) {
      expect(api.coverUrl(7, size)).toContain(`size=${size}`)
    }
    // un tamaño raro no se manda: el núcleo solo cachea los suyos
    expect(api.coverUrl(7, 137)).not.toContain('size=')
    expect(api.coverUrl(7)).not.toContain('size=')
  })

  it('el nucleo acepta esos mismos tamaños', () => {
    const core = read(CORE, 'api.py')
    for (const size of COVER_SIZES) {
      expect(core, `el núcleo no genera miniaturas de ${size}`).toContain(String(size))
    }
  })
})

describe('los errores se cuentan en una sola frase', () => {
  it('sin el «Error:» que añade el navegador', () => {
    expect(errorMessage(new Error('no existe'))).toBe('no existe')
    expect(errorMessage(new ApiError('404: no existe', 404))).toBe('404: no existe')
    expect(errorMessage('algo suelto')).toBe('algo suelto')
  })
})
