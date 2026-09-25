import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// Ajustes: analizar y convertir son tareas largas del nucleo que se siguen
// hasta que acaban, las carpetas y lo omitido se cambian sin salir de aqui, y
// los temas se eligen, se crean y se editan.
const held = vi.hoisted(() => ({ state: null, api: null, pickFolder: null }))

vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { createState, buildApiDouble, createAppDouble } = await import('./support/backend.js')
  const { vi: v } = await import('vitest')
  held.state = createState()
  held.api = buildApiDouble(actual, held.state)
  held.pickFolder = v.fn(async () => null)
  return { ...actual, api: held.api, app: createAppDouble(), pickFolder: held.pickFolder }
})

import SettingsPage from '../src/components/SettingsPage.vue'
import { finishedJob } from './support/backend.js'
import { clearNotices, useNotices } from '../src/composables/useNotices.js'
import { dialogOk, dialogCancel, useDialog } from '../src/composables/useDialog.js'
import { resetPreferences, usePreferences } from '../src/composables/usePreferences.js'
import { customThemes, saveCustomTheme, CATALOG } from '../src/themes.js'
import { coreSource } from './support/core.js'

const api = () => held.api
let w = null
beforeEach(() => {
  vi.clearAllMocks()
  clearNotices()
  dialogCancel()
  resetPreferences()
  localStorage.clear()
  held.state.status.configured = true
  held.state.jobs = {}
})
afterEach(() => {
  w?.unmount()
  w = null
})
const montar = async () => {
  w = mount(SettingsPage, { attachTo: document.body })
  await flushPromises()
  await flushPromises()
  return w
}
const boton = (text) => w.findAll('button').find((b) => b.text().includes(text))
const avisos = () => useNotices().notices.value.map((n) => n.message)

describe('analizar e indexar todo', () => {
  it('es una tarea: se ve como va y se avisa al acabar', async () => {
    // el nucleo contesta al momento y luego se pregunta hasta que acaba
    const vuelta = [
      {
        name: 'escaneo',
        active: true,
        done: 10,
        total: 40,
        message: 'leyendo etiquetas',
        result: null,
        error: ''
      },
      {
        name: 'escaneo',
        active: true,
        done: 30,
        total: 40,
        message: 'leyendo etiquetas',
        result: null,
        error: ''
      },
      finishedJob(
        'escaneo',
        { stats: { total: 40 } },
        { done: 40, total: 40, message: '40 canciones (2 nuevas)' }
      )
    ]
    api().scan.mockResolvedValueOnce({ job: vuelta[0] })
    let paso = 0
    let seguir
    api().job.mockImplementation(
      () =>
        new Promise((r) => {
          seguir = () => r(vuelta[++paso])
        })
    )
    await montar()
    await boton('Analizar e indexar todo').trigger('click')
    await flushPromises()
    expect(w.find('.job-progress').text()).toContain('10 / 40')
    expect(w.find('.job-progress').text()).toContain('leyendo etiquetas')
    expect(boton('Analizando').attributes('disabled')).toBeDefined()
    await vi.waitFor(() => expect(seguir).toBeTypeOf('function'))
    seguir()
    await flushPromises()
    await vi.waitFor(() => expect(w.find('.job-progress').text()).toContain('30 / 40'))
    seguir()
    await flushPromises()
    await flushPromises()
    await vi.waitFor(() => expect(w.find('.job-progress').exists()).toBe(false))
    expect(avisos().some((m) => m.includes('40 canciones (2 nuevas)'))).toBe(true)
    expect(w.emitted('reindexed')).toBeTruthy()
    api().job.mockReset()
  })

  it('si la tarea falla, lo dice en castellano y el boton vuelve', async () => {
    api().scan.mockResolvedValueOnce({
      job: {
        name: 'escaneo',
        active: false,
        done: 0,
        total: 0,
        message: '',
        result: null,
        error: 'la carpeta ya no existe'
      }
    })
    await montar()
    await boton('Analizar e indexar todo').trigger('click')
    await flushPromises()
    await flushPromises()
    expect(avisos()).toContain('No se pudo analizar: la carpeta ya no existe')
    expect(boton('Analizar e indexar todo').attributes('disabled')).toBeUndefined()
  })
})

describe('convertir a mp3', () => {
  beforeEach(() => {
    api().convertible.mockResolvedValue({
      total: 3,
      files: ['a.m4a', 'b.flac', 'c.ogg'],
      protected: []
    })
  })

  it('simular contesta al momento, sin tarea', async () => {
    await montar()
    await boton('Simular').trigger('click')
    await flushPromises()
    expect(api().convert).toHaveBeenCalledWith(expect.objectContaining({ dry_run: true }))
    expect(api().runJob).not.toHaveBeenCalled()
    expect(useDialog().dialog.value.message).toContain('Se convertirían 0 archivos')
    dialogOk()
  })

  it('convertir de verdad es una tarea, y «conservar el original» viaja como `keep`', async () => {
    // Se mandaba «keepOne», que el nucleo no lee: la casilla no hacia nada y
    // los archivos de partida se borraban igual.
    await montar()
    await w
      .findAll('.toggle')
      .find((t) => t.text().includes('Conservar el archivo original'))
      .find('button')
      .trigger('click')
    await flushPromises()
    await boton('Convertir ahora').trigger('click')
    await flushPromises()
    await flushPromises()
    const cuerpo = api().convert.mock.calls.at(-1)[0]
    expect(cuerpo).toMatchObject({ dry_run: false, keep: true })
    expect(cuerpo).not.toHaveProperty('keepOne')
    expect(api().runJob).toHaveBeenCalledWith(
      expect.any(Function),
      'conversion',
      expect.any(Object)
    )
    expect(useDialog().dialog.value.message).toContain('Convertidos 0, fallos 0')
    dialogOk()
    expect(w.emitted('changed')).toBeTruthy()
  })
})

describe('la calidad de conversion', () => {
  it('cada calidad que se ofrece es una que el nucleo entiende', async () => {
    // config.py: MP3_QUALITY = high | medium | variable. Ajustes ofrecia
    // «alta» y «media», que el nucleo no conoce: elegir Media acababa en 320k
    // sin decir nada.
    await montar()
    const campo = w
      .findAll('.field')
      .find((f) => f.find('.field-label').exists() && f.find('.field-label').text() === 'Calidad')
    const elegir = async (nombre) => {
      await campo.find('.select-box').trigger('click')
      await campo
        .findAll('.select-opt')
        .find((o) => o.text().startsWith(nombre))
        .trigger('click')
      await flushPromises()
    }
    // empieza en «Alta»: se recorren las otras y se vuelve a ella
    const elegidas = []
    for (const nombre of ['Media', 'Variable', 'Alta']) {
      await elegir(nombre)
      elegidas.push(api().saveSettings.mock.calls.at(-1)[0].quality)
    }
    expect(elegidas).toEqual(['medium', 'variable', 'high'])
    // elegir la que ya estaba no guarda nada
    const veces = api().saveSettings.mock.calls.length
    await elegir('Alta')
    expect(api().saveSettings).toHaveBeenCalledTimes(veces)
    const core = coreSource()
    for (const q of elegidas)
      expect(core, `el nucleo no conoce la calidad «${q}»`).toMatch(new RegExp(`"${q}"`))
  })
})

describe('carpetas', () => {
  const carpetas = (folders, exclusions = []) => ({
    folders,
    exclusions,
    always_excluded: ['Revisar']
  })

  it('una carpeta que no esta se busca con «¿Donde esta?»', async () => {
    api().folders.mockResolvedValue(carpetas([{ path: '/viejo/Musica', n: 0, exists: false }]))
    api().relocateFolder.mockResolvedValueOnce({
      from: '/viejo/Musica',
      to: '/nuevo/Musica',
      back: 12,
      ...carpetas([{ path: '/nuevo/Musica', n: 12, exists: true }])
    })
    held.pickFolder.mockResolvedValueOnce('/nuevo/Musica')
    await montar()
    expect(w.text()).toContain('no está')
    await boton('¿Dónde está?').trigger('click')
    await flushPromises()
    expect(api().relocateFolder).toHaveBeenCalledWith('/viejo/Musica', '/nuevo/Musica')
    expect(avisos().some((m) => m.includes('vuelven 12 canciones'))).toBe(true)
    expect(w.text()).toContain('/nuevo/Musica')
    expect(w.emitted('changed')).toBeTruthy()
  })

  it('quitar una carpeta y omitir o dejar de omitir otras', async () => {
    api().folders.mockResolvedValue(carpetas([{ path: '/musica', n: 3, exists: true }]))
    api().removeFolder.mockResolvedValueOnce(carpetas([]))
    api().addExclusion.mockResolvedValueOnce(carpetas([], [{ pattern: 'Secuencias', note: '' }]))
    api().removeExclusion.mockResolvedValueOnce(carpetas([]))
    await montar()
    await boton('Quitar').trigger('click')
    await flushPromises()
    expect(api().removeFolder).toHaveBeenCalledWith('/musica')
    expect(w.text()).not.toContain('/musica')
    const patron = w
      .findAll('input')
      .find((i) => i.attributes('placeholder')?.includes('Secuencias'))
    await patron.setValue('Secuencias')
    await boton('Omitir').trigger('click')
    await flushPromises()
    expect(api().addExclusion).toHaveBeenCalledWith('Secuencias')
    const chip = w.findAll('button.chip').find((b) => b.text().includes('Secuencias'))
    expect(chip.attributes('aria-label')).toBe('Dejar de omitir Secuencias')
    await chip.trigger('click')
    await flushPromises()
    expect(api().removeExclusion).toHaveBeenCalledWith('Secuencias')
    expect(w.findAll('button.chip').some((b) => b.text().includes('Secuencias'))).toBe(false)
  })

  it('si el nucleo no puede quitarla, lo dice', async () => {
    api().folders.mockResolvedValue(carpetas([{ path: '/musica', n: 3, exists: true }]))
    api().removeFolder.mockRejectedValueOnce(new Error('sin permiso'))
    await montar()
    await boton('Quitar').trigger('click')
    await flushPromises()
    expect(avisos()).toContain('No se pudo quitar: sin permiso')
  })
})

describe('temas', () => {
  it('cada tarjeta se pinta con los colores de su tema, ninguno sin definir', async () => {
    // un color que no existe (`t.v.acento` tras pasar a ingles) salia como
    // `undefined` y la tarjeta se quedaba sin fondo ni borde
    await montar()
    const tarjetas = w.findAll('.theme-card')
    expect(tarjetas.length).toBe(Object.keys(CATALOG).length)
    for (const t of tarjetas) {
      const estilo = t.attributes('style')
      expect(estilo).toMatch(/background:\s*rgb/)
      expect(estilo).toMatch(/border-color:\s*rgb/)
      for (const m of t.findAll('.theme-swatch'))
        expect(m.attributes('style')).toMatch(/background:\s*rgb/)
      expect(t.find('.theme-name').attributes('style')).toMatch(/color:\s*rgb/)
    }
  })

  it('cada tema es un boton que dice si esta puesto', async () => {
    await montar()
    const tarjetas = w.findAll('.theme-pick')
    expect(tarjetas.length).toBe(Object.keys(CATALOG).length)
    const oceano = tarjetas.find((t) => t.text().includes('Oceano'))
    await oceano.trigger('click')
    expect(usePreferences().theme.value).toBe('ocean')
    expect(oceano.attributes('aria-pressed')).toBe('true')
  })

  it('editar otro tema con el editor abierto lo empieza con sus colores', async () => {
    saveCustomTheme('propio-rojo', {
      name: 'Rojo',
      kind: 'dark',
      custom: true,
      v: { ...CATALOG.night.v, accent: '#aa0000' }
    })
    saveCustomTheme('propio-azul', {
      name: 'Azul',
      kind: 'dark',
      custom: true,
      v: { ...CATALOG.night.v, accent: '#0000aa' }
    })
    await montar()
    const editar = (nombre) =>
      w
        .findAll('.theme-card')
        .find((c) => c.text().includes(nombre))
        .findAll('button')
        .find((b) => b.text() === 'Editar')
    await editar('Rojo').trigger('click')
    await editar('Azul').trigger('click')
    await flushPromises()
    await boton('Guardar tema').trigger('click')
    expect(customThemes()['propio-azul'].v.accent).toBe('#0000aa')
    expect(customThemes()['propio-rojo'].v.accent).toBe('#aa0000')
  })

  it('crear uno propio y borrarlo', async () => {
    await montar()
    await boton('Crear el mio').trigger('click')
    expect(w.find('h3').text()).toBeTruthy()
    await boton('Guardar tema').trigger('click')
    await flushPromises()
    const nuevo = Object.keys(customThemes())[0]
    expect(nuevo).toMatch(/^propio-/)
    expect(usePreferences().theme.value).toBe(nuevo)
    await w
      .findAll('.theme-card')
      .find((c) => c.find('.theme-actions').exists())
      .findAll('button')
      .find((b) => b.text() === 'Borrar')
      .trigger('click')
    await flushPromises()
    dialogOk()
    await flushPromises()
    expect(customThemes()).toEqual({})
    expect(usePreferences().theme.value).toBe('night')
  })
})
