// El aviso de lo que el diario recuperó al abrir (utils/journal.js y
// tellJournal en useLibrary): corto, en castellano y una sola vez.
import { describe, it, expect, beforeEach } from 'vitest'
import { journalMessage } from '../src/utils/journal.js'
import { tellJournal } from '../src/composables/useLibrary.js'
import { useNotices, clearNotices } from '../src/composables/useNotices.js'

const { notices } = useNotices()

beforeEach(() => {
  localStorage.clear()
  clearNotices()
})

describe('what the journal says', () => {
  it('nothing to report, nothing', () => {
    expect(journalMessage(null)).toBe('')
    expect(journalMessage({ songs: 0, lists: 0, entries: 0, ghosts: 0 })).toBe('')
  })

  it('moved songs that go back to their playlists', () => {
    expect(journalMessage({ entries: 3, lists: 1 })).toBe(
      'Tus listas siguen intactas: 3 canciones que se movieron volvieron a sus listas.'
    )
    expect(journalMessage({ entries: 1 })).toBe(
      'Tus listas siguen intactas: una canción que se movió volvió a su lista.'
    )
  })

  it('a new database: rebuilt playlists, the data of each song and what is missing', () => {
    expect(journalMessage({ fresh: true, lists: 2, entries: 9, songs: 4, ghosts: 1 })).toBe(
      'Tus listas siguen intactas: 9 canciones que se movieron volvieron a sus listas; ' +
        '2 listas rehechas; 4 canciones recuperaron sus estrellas y su estudio; ' +
        'una que ya no está quedó en su lista como «No está».'
    )
  })

  it('is reported once', () => {
    tellJournal({ entries: 2, at: 100 })
    tellJournal({ entries: 2, at: 100 })
    expect(notices.value.filter((n) => n.message.includes('volvieron'))).toHaveLength(1)
    tellJournal({ entries: 1, at: 200 })
    expect(notices.value).toHaveLength(2)
    tellJournal(null)
    expect(notices.value).toHaveLength(2)
  })
})
