// Los formateadores y la agrupación compartidos: estaban copiados por los
// componentes y cada copia decía «sin dato» a su manera.
import { describe, it, expect } from 'vitest'
import {
  formatDuration,
  formatTime,
  formatMinutes,
  formatAgo,
  formatMegabytes,
  formatGigabytes,
  fullName
} from '../src/utils/format.js'
import { groupKey, groupSongs, groupedOrder } from '../src/utils/groups.js'
import { song } from './support/backend.js'

describe('formatting', () => {
  it('duration as m:ss, and a dash (or whatever is requested) without data', () => {
    expect(formatDuration(0)).toBe('—')
    expect(formatDuration(null)).toBe('—')
    expect(formatDuration(Number.NaN)).toBe('—')
    expect(formatDuration(-3)).toBe('—')
    expect(formatDuration(59.9)).toBe('0:59')
    expect(formatDuration(125)).toBe('2:05')
    expect(formatDuration(3600)).toBe('60:00')
    expect(formatDuration(0, '')).toBe('')
    expect(formatTime(0)).toBe('0:00')
    expect(formatTime(61)).toBe('1:01')
  })

  it('the length of a group, in whole minutes', () => {
    expect(formatMinutes(0)).toBe('0 min')
    expect(formatMinutes(undefined)).toBe('0 min')
    expect(formatMinutes(59)).toBe('0 min')
    expect(formatMinutes(240)).toBe('4 min')
    expect(formatMinutes(299)).toBe('4 min')
  })

  it('how long ago', () => {
    const now = 1_000_000 * 1000
    const ago = (s) => formatAgo(1_000_000 - s, now)
    expect(formatAgo(0, now)).toBe('nunca')
    expect(ago(10)).toBe('ahora mismo')
    expect(ago(5 * 60)).toBe('hace 5 min')
    expect(ago(3 * 3600)).toBe('hace 3 h')
    expect(ago(47 * 3600)).toBe('hace 47 h')
    expect(ago(3 * 86400)).toBe('hace 3 días')
  })

  it('sizes', () => {
    expect(formatMegabytes(0)).toBe('—')
    expect(formatMegabytes(1572864)).toBe('1.5 MB')
    expect(formatGigabytes(1073741824 * 12.3)).toBe('12.30')
  })
})

describe('grouping', () => {
  const list = [
    song(1, { title: 'A Una Voz', artist: 'New Wine' }),
    song(2, { title: 'Mi Gozo', artist: 'Barak' }),
    song(3, { title: 'Que Se Abra El Cielo', artist: 'Miel San Marcos' }),
    song(4, { title: 'Sera Llena La Tierra', artist: 'Barak' }),
    song(5, { title: 'Ángel', artist: '' })
  ]

  it('groups go in alphabetical order and, inside, in list order', () => {
    const groups = groupSongs(list, 'artist')
    expect(groups.map(([k, l]) => [k, l.map((s) => s.id)])).toEqual([
      ['Barak', [2, 4]],
      ['Miel San Marcos', [3]],
      ['New Wine', [1]],
      ['Sin artista', [5]]
    ])
    expect(groupedOrder(list, 'artist').map((s) => s.id)).toEqual([2, 4, 3, 1, 5])
  })

  it('items without data go to their own named group', () => {
    const empty = song(9, { title: '', file: 'x.mp3', artist: '', album: '', genre: '', key: '' })
    expect(groupKey(empty, 'artist')).toBe('Sin artista')
    expect(groupKey(empty, 'album')).toBe('Sin álbum')
    expect(groupKey(empty, 'genre')).toBe('Sin género')
    expect(groupKey(empty, 'key')).toBe('Sin tono')
    expect(groupKey(empty, 'initial')).toBe('X')
    expect(groupKey({ ...empty, folder: '' }, 'folder')).toBe('(raíz)')
    expect(groupKey(empty, 'otra cosa')).toBe('—')
  })
})

describe('the full name of a song', () => {
  it('is its file name without the extension', () => {
    expect(
      fullName({
        file: 'New Wine - A Una Voz + Shekinah (En Vivo, Version Larga).mp3',
        title: 'A Una Voz + Shekinah',
        artist: 'New Wine'
      })
    ).toBe('New Wine - A Una Voz + Shekinah (En Vivo, Version Larga)')
    expect(fullName({ file: 'Barak - Mi Gozo.flac' })).toBe('Barak - Mi Gozo')
  })
  it('without a file, «Artista - Titulo»', () => {
    expect(fullName({ title: 'Mi Gozo', artist: 'Barak' })).toBe('Barak - Mi Gozo')
    expect(fullName({ title: 'Mi Gozo' })).toBe('Mi Gozo')
    expect(fullName(null)).toBe('')
  })
})
