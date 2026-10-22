// @ts-check
// Lo que se guardaba en el navegador con nombres en español pasa a su nombre
// de ahora, una sola vez: quien ya tenía DanPlay conserva lo suyo (la
// velocidad, el clic, las salidas, la onda…). Se importa lo primero en cada
// entrada (main.js y mobile/main.js), antes de que nadie lea nada.

/** [nombre de antes, nombre de ahora] */
export const LEGACY_KEYS = [
  ['danplay.clic.auto', 'danplay.click.auto'],
  ['danplay.clic.cuenta', 'danplay.click.countIn'],
  ['danplay.clic.mano', 'danplay.click.manual'],
  ['danplay.clic.modo', 'danplay.click.mode'],
  ['danplay.clic.sonido', 'danplay.click.sound'],
  ['danplay.diarioContado', 'danplay.journalTold'],
  ['danplay.exportar', 'danplay.export'],
  ['danplay.fundido', 'danplay.crossfade'],
  ['danplay.guias.iman', 'danplay.cues.magnet'],
  ['danplay.guias.volumen', 'danplay.cues.volume'],
  ['danplay.guias.voz', 'danplay.cues.voice'],
  ['danplay.movil.compacta', 'danplay.mobile.compact'],
  ['danplay.movil.pantalla', 'danplay.mobile.keepAwake'],
  ['danplay.movil.toque', 'danplay.mobile.tapPlays'],
  ['danplay.novedadesVistas', 'danplay.releasesSeen'],
  ['danplay.ondaEstilo', 'danplay.waveStyle'],
  ['danplay.salidas', 'danplay.outputs'],
  ['danplay.study.alAcabar', 'danplay.study.atEnd'],
  ['danplay.study.clicMas', 'danplay.study.clickMore'],
  ['danplay.study.grande', 'danplay.study.large'],
  ['danplay.study.irConClic', 'danplay.study.seekClick'],
  ['danplay.study.recorridoVisto', 'danplay.study.tourSeen'],
  ['danplay.study.verCanales', 'danplay.study.showRoutes'],
  ['danplay.study.verMezcla', 'danplay.study.showMix'],
  ['danplay.terminosVistos', 'danplay.termsSeen'],
  ['danplay.vel', 'danplay.speed']
]

/** Los estilos de la onda también tenían nombre en español. */
const WAVE_STYLES = /** @type {Record<string, string>} */ ({
  barras: 'bars',
  bloques: 'blocks',
  rayitas: 'lines',
  suave: 'smooth',
  contorno: 'outline',
  reflejo: 'mirror'
})

/**
 * Mueve cada valor guardado con el nombre de antes al de ahora (si el de
 * ahora no tiene ya uno) y borra el de antes.
 * @param {Storage | undefined} [storage]
 */
export function migrateLegacyStorage(storage = globalThis.localStorage) {
  if (!storage) return
  for (const [old, key] of LEGACY_KEYS) {
    try {
      const value = storage.getItem(old)
      if (value == null) continue
      if (storage.getItem(key) == null) {
        storage.setItem(key, key === 'danplay.waveStyle' ? (WAVE_STYLES[value] ?? value) : value)
      }
      storage.removeItem(old)
    } catch {
      /* sin almacenamiento (modo privado): no hay nada que mover */
    }
  }
  try {
    const wave = storage.getItem('danplay.waveStyle')
    if (wave != null && wave in WAVE_STYLES) storage.setItem('danplay.waveStyle', WAVE_STYLES[wave])
  } catch {
    /* sin almacenamiento */
  }
  migrateCueRoutes(storage)
}

/**
 * En «Salidas», los carriles de guías se llamaban `guias_<carril>`; ahora
 * `cues_<carril>`. Lo repartido a mano (y el mono) de cada uno se conserva.
 * @param {Storage} storage
 */
function migrateCueRoutes(storage) {
  try {
    const raw = storage.getItem('danplay.outputs')
    if (!raw || !raw.includes('"guias_')) return
    const prefs = JSON.parse(raw)
    for (const part of [prefs?.custom, prefs?.mono]) {
      const stems = part?.stems
      if (!stems || typeof stems !== 'object') continue
      for (const key of Object.keys(stems)) {
        if (!key.startsWith('guias_')) continue
        const renamed = 'cues_' + key.slice('guias_'.length)
        if (!(renamed in stems)) stems[renamed] = stems[key]
        delete stems[key]
      }
    }
    storage.setItem('danplay.outputs', JSON.stringify(prefs))
  } catch {
    /* sin almacenamiento, o algo que no se entiende: se queda como estaba */
  }
}

migrateLegacyStorage()
