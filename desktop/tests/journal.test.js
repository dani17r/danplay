// El aviso de lo que el diario recuperó al abrir (utils/diario.js y
// tellDiario en useLibrary): corto, en castellano y una sola vez.
import { describe, it, expect, beforeEach } from 'vitest'
import { diarioMessage } from '../src/utils/diario.js'
import { tellDiario } from '../src/composables/useLibrary.js'
import { useNotices, clearNotices } from '../src/composables/useNotices.js'

const { notices } = useNotices()

beforeEach(() => {
  localStorage.clear()
  clearNotices()
})

describe('lo que dice el diario', () => {
  it('nada que contar, nada', () => {
    expect(diarioMessage(null)).toBe('')
    expect(diarioMessage({ songs: 0, lists: 0, entries: 0, ghosts: 0 })).toBe('')
  })

  it('canciones movidas que vuelven a sus listas', () => {
    expect(diarioMessage({ entries: 3, lists: 1 })).toBe(
      'Tus listas siguen intactas: 3 canciones que se movieron volvieron a sus listas.'
    )
    expect(diarioMessage({ entries: 1 })).toBe(
      'Tus listas siguen intactas: una canción que se movió volvió a su lista.'
    )
  })

  it('una base nueva: listas rehechas, lo de cada canción y lo que no está', () => {
    expect(diarioMessage({ fresh: true, lists: 2, entries: 9, songs: 4, ghosts: 1 })).toBe(
      'Tus listas siguen intactas: 9 canciones que se movieron volvieron a sus listas; ' +
        '2 listas rehechas; 4 canciones recuperaron sus estrellas y su estudio; ' +
        'una que ya no está quedó en su lista como «No está».'
    )
  })

  it('se cuenta una vez', () => {
    tellDiario({ entries: 2, at: 100 })
    tellDiario({ entries: 2, at: 100 })
    expect(notices.value.filter((n) => n.message.includes('volvieron'))).toHaveLength(1)
    tellDiario({ entries: 1, at: 200 })
    expect(notices.value).toHaveLength(2)
    tellDiario(null)
    expect(notices.value).toHaveLength(2)
  })
})
