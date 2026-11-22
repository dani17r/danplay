// El clic del modo estudio (StudyBar.vue, StudyClick.vue): el metrónomo de la
// canción que se estudia, su tempo, su compás y su «1», el clic manual (los
// pulsos que se ajustan sobre la onda, con useBeatEdit.js) y re-analizarlo.
//
// El pulso y el compas los da `useClick`: con el automático, los de cada
// cancion (Beat This! en el nucleo, o el analisis de Rust si aqui no se
// puede); con el manual, los que movio él sobre la onda, guardados con ella;
// con el tempo fijo, uno para todas. Lo que uno ajuste a una cancion (tempo,
// compas, «1», doble/mitad, sus pulsos) se guarda con ella en
// `study.metronome`; lo del tempo fijo, en el navegador (es de quien toca).
// Lo guardado conserva los nombres de antes ('auto', 'own', 'manual'): solo
// cambian las palabras. «A mano» a secas se entendía como «lo que moví yo» y
// era un tempo para todas.
//
// Sin `@ts-check` (los demás useStudy* lo llevan): el código viene tal cual de
// StudyBar.vue, que no se comprobaba, y el informe del re-análisis trae más
// de lo que dicen sus tipos.
import { ref, computed, watch, markRaw } from 'vue'
import { api, errorMessage } from '../api.js'
import { usePlayback } from './usePlayback.js'
import { notify } from './useNotices.js'
import { ask } from './useDialog.js'
import { openMenu } from './useContextMenu.js'
import { useBeatEdit } from './useBeatEdit.js'
import { isDefault, parse } from './useStudyDoc.js'
import { isMobileApp } from '../platform.js'
import { formatTime } from '../utils/format.js'
import { clamp, round2 } from '../utils/num.js'
import { readPref, savePref } from '../utils/prefs.js'
import {
  effectiveGrid,
  formatBpm,
  parseBpm,
  multFactor,
  clickTimes,
  cleanOwn,
  ownTouched,
  beatChanges,
  windowsText
} from '../utils/beats.js'

/**
 * Se arma después de los tramos (al oír unos pulsos ajustados se pone su
 * tramo) y antes de las guías (sus pulsos son los del imán). Al crearse deja
 * en el motor la cuenta, el sonido y el volumen del clic de quien toca.
 * @param {import('./studyContext.js').StudyContext} ctx
 */
export function useStudyClick(ctx) {
  const player = usePlayback()
  const { track, duration, speed, metronome } = player
  const { study, loadedFor, studyFor, scheduleSave, flushSave, warnIfOnlyHere } = ctx.doc
  const { click, timeline } = ctx
  const clickAuto = click.auto
  const clickMode = click.mode
  const clickWanted = click.wanted
  const grid = click.grid
  const { stems } = ctx.stems

  const CLICK_MODES = [
    {
      v: 'auto',
      n: 'Automático',
      title:
        'Lo que analizó DanPlay: cada canción con su clic, detectado solo (el pulso y el «1»). Si re-analizas, aquí se ve lo nuevo'
    },
    {
      v: 'own',
      n: 'Manual',
      title:
        'Lo que mueves tú en la onda, pulso a pulso: se guarda con cada canción y vuelve cuando eliges Manual'
    },
    {
      v: 'manual',
      n: 'Tempo fijo',
      title: 'Un solo tempo que pones tú, el mismo en todas las canciones'
    }
  ]
  /** El título de un modo: el de Manual dice si esta canción ya tiene lo suyo. */
  function modeTitle(m) {
    if (m.v !== 'own') return m.title
    return (
      m.title +
      (hasOwn.value ? ' · esta canción ya tiene el suyo' : ' · esta canción aún no tiene el suyo')
    )
  }
  /**
   * Cambiar de modo. Antes se guarda lo que esté pendiente: Manual se lee de lo
   * guardado, y un ajuste de hace medio segundo aún no lo estaría.
   */
  async function chooseMode(v) {
    await flushSave()
    await click.setMode(v)
  }
  /**
   * La rejilla tal como suena, para la onda: con el doble o la mitad, el
   * compas y el «1» corrido.
   */
  const shownGrid = computed(() => {
    // a mano el clic no sigue la cancion: su rejilla no pinta nada
    if (!click.auto.value) return null
    const m = study.value.metronome || {}
    return effectiveGrid(grid.value, {
      mult: m.mult || 0,
      // sin compás propio, el de la preferencia general (Ajustes)
      meter: m.meter ?? player.defaultMeter.value,
      shift: m.shift || 0
    })
  })

  /**
   * La rejilla del clic tal como SUENA. Con el tempo puesto a mano sobre una
   * canción con rejilla, el clic deja de seguir los pulsos detectados (que es
   * justo lo que falló): va a un tempo fijo desde el primer «1» de la canción
   * (como el motor, `click_plan`). La onda, el imán de las guías, «ajustar a
   * los pulsos» y la guía automática usaban la detectada tal cual. Sin tempo a
   * mano, es la de la canción.
   */
  const soundingGrid = computed(() => {
    const g = shownGrid.value
    const m = metronome.value
    if (!g || !m.free || !m.has_grid || !(m.bpm > 0)) return g
    const { beats, accents } = clickTimes(g, m, duration.value || 0)
    if (beats.length < 2) return g
    const bars = []
    accents.forEach((on, i) => {
      if (on) bars.push(i)
    })
    return {
      ...g,
      bpm: m.bpm,
      meter: m.meter ?? g.meter,
      beats,
      first_downbeat: bars[0] ?? 0,
      phase3: 0,
      phase4: 0,
      bars,
      closed: true
    }
  })

  /**
   * El tempo que se oye: el del clic por la velocidad. También con el tempo a
   * mano: el motor lleva sus golpes en tiempo de canción (así no se despega de
   * ella), y a 0,8× un 100 suena a 80. Se enseñaba 100, y al tocar «+»
   * siguiendo la canción a 0,8× (96 oídos) se guardaba 97, que sonaba a 77,6.
   */
  const bpmHeard = computed(() => (metronome.value.bpm || 0) * speed.value)
  const bpmShown = computed(() => (bpmHeard.value ? formatBpm(bpmHeard.value) : '—'))
  /** Lo que se esta escribiendo en el tempo (o null: se enseña el que va). */
  const bpmDraft = ref(null)
  /** Lo que lleva la rejilla que carga, en %. */
  const clickPercent = computed(() => Math.round(click.progress.value * 100))
  /** Cargando el clic de esta canción (en automático): no se puede poner aún. */
  const clickLoading = computed(() => click.auto.value && click.loading.value)
  /** Esta canción tiene su clic manual guardado. */
  const hasOwn = computed(() => !!study.value.metronome?.own?.beats?.length)
  /**
   * El carril del clic de la línea de tiempo (en el escritorio). Un `computed`:
   * escrito en la plantilla era un objeto nuevo en cada pintada, y la línea de
   * tiempo se repintaba entera aunque dijera lo mismo.
   */
  const clickLane = computed(() =>
    !isMobileApp && track.value
      ? { on: !!metronome.value.on, volume: metronome.value.volume ?? 0.8 }
      : null
  )
  /**
   * Dónde el clic es una suposición, para la onda: lo que dijo el último
   * análisis de esta canción (nada si no lo dijo, o si el clic no sigue a la
   * canción).
   */
  const weakRegions = computed(() =>
    clickAuto.value && click.weak.value.id === (track.value?.id ?? null)
      ? click.weak.value.regions
      : []
  )
  /**
   * Cómo encajaba el clic con los golpes antes y después del último
   * re-análisis de esta canción, si se midió (en el aviso de debajo del clic).
   */
  const fitTitle = computed(() => {
    const f = click.fit.value
    if (f.id !== (track.value?.id ?? null) || !f.before || !f.after) return ''
    const ms = (v) => `${Math.round(v)} ms`
    // negativo: el clic cae después del golpe
    const lag = (v) =>
      Math.abs(v) < 1 ? 'a tiempo' : `${ms(Math.abs(v))} ${v < 0 ? 'tarde' : 'antes'}`
    return `Último re-análisis · cómo encaja el clic con los golpes de la música (antes → ahora): ${lag(f.before.median)} → ${lag(f.after.median)}, dispersión ${ms(f.before.spread)} → ${ms(f.after.spread)}`
  })
  const metroStatus = computed(() => {
    const m = metronome.value
    if (player.state.counting_in) return 'contando…'
    if (!click.auto.value) return 'tempo fijo: el mismo en todas las canciones'
    if (clickLoading.value) return `cargando el clic de esta canción… ${clickPercent.value} %`
    if (click.reanalyzing.value != null) {
      return `oyendo la batería separada… ${Math.round(click.reanalyzing.value * 100)} %`
    }
    if (click.error.value) return 'sin compás detectado: va libre'
    if (!m.has_grid) return 'sin compás: va libre'
    // el detector no pudo con esta canción: lo que suena es el clic de
    // reserva, a un solo tempo. Antes no se decía y parecía cosa del clic
    if (click.source.value === 'rust' && click.fallbackWhy.value) {
      return `clic aproximado, a un solo tempo: el detector no pudo con esta canción (${click.fallbackWhy.value}). «Re-analizar» lo vuelve a intentar`
    }
    if (m.free) return 'tempo a mano: entra con el pulso de la canción y va a su aire'
    if (clickMode.value === 'own') {
      return hasOwn.value
        ? 'manual: los pulsos que moviste tú en esta canción'
        : 'manual: esta canción aún no tiene el tuyo (suena el automático). Pulsa «Empezar el mío»'
    }
    const sure = m.confidence >= 0.6 ? 'seguro' : m.confidence >= 0.3 ? 'probable' : 'dudoso'
    return `sigue la canción · el «1» ${sure}`
  })
  function metroPatch(patch) {
    // a mano, el tempo y el compas son de todas las canciones, no de esta
    if (!click.auto.value) return click.setManual(patch)
    const m = { ...(study.value.metronome || {}) }
    for (const [k, v] of Object.entries(patch)) {
      if (isDefault(k, v)) delete m[k]
      else m[k] = v
    }
    study.value.metronome = m
    scheduleSave()
    return player.setMetronome(patch)
  }
  async function toggleMetronome() {
    if (!track.value) return
    await click.toggle()
  }
  /** El carril del clic, en la onda: encenderlo o callarlo, y su volumen. */
  function onClickLane(patch) {
    if ('on' in patch && !!patch.on !== !!metronome.value.on) toggleMetronome()
    if (typeof patch.volume === 'number') setMetroVolume(patch.volume)
  }
  /**
   * Pone a mano el tempo que se oye, con decimales. Con el doble o la mitad
   * puestos se guarda el tempo sin ellos: al quitarlos, vuelve al que era.
   */
  function setBpm(heard) {
    // lo que se escribe es lo que se quiere oír: a otra velocidad, el tempo
    // del clic (en tiempo de canción) es ese entre la velocidad
    const own = heard / (speed.value || 1)
    // a mano no hay doble ni mitad
    if (!click.auto.value) return click.setManual({ bpm: round2(clamp(own, 20, 300)) })
    const base = own / multFactor(metronome.value.mult)
    return metroPatch({ bpm: round2(clamp(base, 20, 300)) })
  }
  function bumpBpm(delta) {
    const now = Math.round((bpmHeard.value || 100) * 10) / 10
    return setBpm(now + delta)
  }
  /** El campo del tempo, para saber si lo que llega se esta escribiendo en el. */
  let bpmInput = null
  function onBpmFocus(e) {
    bpmInput = e.target
    bpmDraft.value = bpmHeard.value ? bpmShown.value : ''
    e.target.select?.()
  }
  /**
   * Lo que llega del campo del tempo. Vacio sin estar escribiendo en el es la
   * «x» del campo, que solo sale con tempo a mano: vuelve al de la cancion.
   */
  function onBpmInput(v) {
    if (v === '' && document.activeElement !== bpmInput) {
      bpmDraft.value = null
      followSong()
      return
    }
    bpmDraft.value = v
  }
  function commitBpm() {
    const v = parseBpm(bpmDraft.value)
    bpmDraft.value = null
    if (v != null && Math.abs(v - bpmHeard.value) >= 0.05) setBpm(v)
  }
  /** En el tempo: Enter lo pone, Escape lo deja; las flechas lo mueven de décima en décima. */
  function onBpmKey(e) {
    if (e.key === 'Enter') {
      e.preventDefault()
      e.target.blur()
    } else if (e.key === 'Escape') {
      bpmDraft.value = null
      e.target.blur()
    } else if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
      e.preventDefault()
      const step = (e.key === 'ArrowUp' ? 1 : -1) * (e.shiftKey ? 1 : 0.1)
      const from = parseBpm(bpmDraft.value) ?? Math.round(bpmHeard.value * 10) / 10
      const to = Math.round(clamp(from + step, 10, 600) * 10) / 10
      bpmDraft.value = formatBpm(to)
      setBpm(to)
    }
  }
  const followSong = () => metroPatch({ bpm: null })
  /**
   * Otro compas. El que detecto el analisis no se guarda como ajuste: si otro
   * dia se analiza mejor, que mande el analisis. El «1» corrido se olvida: el
   * de otro compas se saca otra vez de donde caia.
   */
  const setMeter = (v) =>
    clickAuto.value
      ? metroPatch({
          // (con «sin acento» por defecto, el detectado SÍ se guarda: no
          // guardarlo lo devolvería a «sin acento» al tocarlo)
          meter: grid.value?.meter === v && player.accentDefault.value ? null : v,
          shift: 0
        })
      : click.setManual({ meter: v })
  const shiftOne = () =>
    metroPatch({ shift: ((metronome.value.shift || 0) + 1) % (metronome.value.meter || 4) })
  const setMult = (mult) => metroPatch({ mult: metronome.value.mult === mult ? 0 : mult })
  /** Lo que dice ×2 o ÷2, con los bpm de verdad: «68 → 136». */
  function multTitle(mult) {
    const plain = bpmHeard.value / multFactor(metronome.value.mult)
    if (metronome.value.mult === mult) return `Quitar: volver a ${formatBpm(plain)} bpm`
    const to = `${formatBpm(plain)} → ${formatBpm(plain * multFactor(mult))} bpm`
    return mult === 1
      ? `El doble de clics (${to}): para ir a corcheas en una lenta, o si el tempo salió a la mitad`
      : `La mitad de clics (${to})`
  }
  /** El volumen del clic: de quien toca, como la cuenta y el sonido (se recuerda). */
  function setMetroVolume(v) {
    savePref(PREF_VOLUME, v)
    return player.setMetronome({ volume: v })
  }

  // ---- el clic manual: los pulsos ajustados a mano sobre la onda
  /** Editando los pulsos en la onda. */
  const editingBeats = ref(false)
  watch(clickMode, (m) => {
    if (m !== 'own') editingBeats.value = false
  })
  /**
   * Todo lo que cambia el clic manual pasa por `useBeatEdit` (`commitOwn`): lo
   * apunta para deshacer, lo guarda con la canción y lo pone en el motor. Con
   * lo guardado van las zonas que tocó a mano (lo que cambió respecto a lo que
   * se veía), para que un re-análisis las respete y rehaga el resto.
   */
  const edit = useBeatEdit({
    songId: loadedFor,
    study,
    grid: soundingGrid,
    duration,
    editing: editingBeats,
    visible: ctx.visible,
    click,
    player,
    scheduleSave,
    flushSave,
    hasDrums: () => !!stems.value?.tracks?.some((t) => t.source === 'drums' && t.exists !== false),
    hear: hearSpan
  })
  /** Lo que se ajustó en la onda (la rejilla entera): un arrastre, un pulso nuevo, el menú de un pulso. */
  const onOwnGrid = (edited, label) => edit.onTimelineGrid(edited, label)
  /**
   * Poner un tramo de pulsos en bucle y que suene, con el clic puesto: para
   * oír lo que se acaba de ajustar.
   */
  async function hearSpan(a, b) {
    await ctx.loops.onLoops([[a, b]], { mode: 'select' })
    if (!clickWanted.value) await click.toggle()
    await ctx.loops.playNow(0)
  }
  /** Borrar el clic manual de esta canción (queda el automático). */
  async function resetOwn() {
    const id = loadedFor.value
    if (!id || !hasOwn.value) return
    const ok = await ask({
      kind: 'confirm',
      title: 'Borrar tu clic manual de esta canción',
      message:
        'Se olvidan los pulsos que moviste a mano en esta canción y queda solo el automático. Ctrl+Z lo recupera mientras no cambies de canción.',
      okLabel: 'Borrarlo',
      danger: true
    })
    if (!ok) return
    await edit.commitOwn(null, 'borrar el manual')
  }
  async function startEditing() {
    if (clickMode.value !== 'own') await click.setMode('own')
    editingBeats.value = !editingBeats.value
    if (editingBeats.value) {
      timeline.value?.zoomToBeats?.()
      notify(
        'Acerca la onda con la rueda del ratón: clic en un pulso lo elige, arrastra uno para moverlo, doble clic para añadir, clic derecho para quitar o hacer el «1». Ctrl+Z deshace. Lo que muevas queda en Manual',
        'info',
        8
      )
    }
  }

  // La cuenta y el sonido del clic son de quien toca, no de cada cancion: se
  // recuerdan en el navegador.
  const COUNT_INS = [
    { v: 0, n: 'no', title: 'Al darle a play, la canción entra sin cuenta' },
    {
      v: 1,
      n: '1 compás',
      title: 'Al darle a play en pausa, un compás de clic antes de que entre'
    },
    {
      v: 2,
      n: '2 compases',
      title: 'Al darle a play en pausa, dos compases de clic antes de que entre'
    }
  ]
  const CLICK_SOUNDS = [
    { v: 'clasico', n: 'clásico', title: 'Un pitido corto' },
    { v: 'madera', n: 'madera', title: 'Un bloque de madera: seco' },
    {
      v: 'baqueta',
      n: 'baqueta',
      title: 'Baqueta contra el aro: corto y brillante, se oye entre platos'
    },
    { v: 'cencerro', n: 'cencerro', title: 'Cencerro: se oye aunque la banda suene fuerte' },
    { v: 'claves', n: 'claves', title: 'Claves: agudo y con cuerpo, atraviesa cualquier mezcla' },
    { v: 'hihat', n: 'charles', title: 'Un charles cerrado: como el que marca un baterista' }
  ]
  const PREF_COUNT = 'danplay.click.countIn'
  const PREF_SOUND = 'danplay.click.sound'
  const PREF_VOLUME = 'danplay.click.volume'
  /** Hasta dónde sube el clic: el triple (en el panel, en su carril y al exportar). */
  const CLICK_MAX = 3
  function setCountIn(v) {
    savePref(PREF_COUNT, v)
    return player.setMetronome({ count_in: v })
  }
  function setClickSound(v) {
    savePref(PREF_SOUND, v)
    return player.setMetronome({ sound: v })
  }
  // al abrir el estudio, lo que quedo elegido
  {
    // (la cuenta solo existe en el celular: en el escritorio, play entra al momento)
    const count = isMobileApp
      ? Math.min(2, Math.max(0, Math.round(Number(readPref(PREF_COUNT, 0)) || 0)))
      : 0
    const sound = readPref(PREF_SOUND, 'clasico')
    // el volumen también: antes volvía al 80 % cada vez que se abría DanPlay
    const volume = Number(readPref(PREF_VOLUME, null))
    player.setMetronome({
      count_in: count,
      sound: CLICK_SOUNDS.some((c) => c.v === sound) ? sound : 'clasico',
      ...(Number.isFinite(volume) && volume > 0 ? { volume: Math.min(CLICK_MAX, volume) } : {})
    })
  }
  /** Más ajustes del clic (compás, cuenta, sonido): plegados, que son muchos. */
  const CLICK_MORE_KEY = 'danplay.study.clickMore'
  const clickMore = ref(readPref(CLICK_MORE_KEY, false) === true)
  watch(clickMore, (v) => savePref(CLICK_MORE_KEY, v))
  const setSongVolume = (v) => player.setVolume(v)

  /**
   * Deja `own` como el clic manual de la canción `id`. Si sigue abierta, por el
   * camino de siempre (el estudio de aquí); si ya se pasó a otra (la batería
   * tarda), se lee lo guardado de aquella y se le escribe solo esto.
   */
  async function saveOwnOf(id, own) {
    if (studyFor.value === id) {
      study.value.metronome = { ...(study.value.metronome || {}), own: markRaw(own) }
      scheduleSave()
      await flushSave()
      return
    }
    const stored = parse((await api.song(id))?.study)
    stored.metronome = { ...(stored.metronome || {}), own }
    warnIfOnlyHere(id, await api.setStudy(id, stored))
  }

  /** «3 pulsos» / «1 pulso» */
  const beatsWord = (n) => `${n} ${n === 1 ? 'pulso' : 'pulsos'}`

  /**
   * Lo que se cuenta al acabar un re-análisis. Siempre dice algo: qué cambió, o
   * por qué no cambió nada, y dónde está lo que no se ve en este modo.
   * @param {{fixed: number, weak: Array<[number, number]>, hadOwn: boolean, manual: boolean,
   *   answered: boolean, legacy: boolean, changed: number, windows: number}} r
   */
  function reanalysisNote(r) {
    const weak = r.weak.length
      ? `. Poco claro (aquí el clic es una suposición): ${windowsText(r.weak)}`
      : ''
    // afinar con el audio mueve unos milisegundos cada pulso: no es «cambió de
    // sitio» (un pulso de otro lugar del compás), es que cae sobre su golpe
    const tuned = r.snapped
      ? `${beatsWord(r.snapped)} ${r.snapped === 1 ? 'afinado' : 'afinados'} con el audio` +
        (r.shiftMs ? ` (${Math.round(Math.abs(r.shiftMs))} ms de media)` : '')
      : ''
    const auto =
      tuned ||
      (r.fixed
        ? `${beatsWord(r.fixed)} ${r.fixed === 1 ? 'cambió' : 'cambiaron'} de sitio`
        : 'ya estaba así, no cambió ningún pulso')
    if (!r.manual) {
      // el automático: lo nuevo se ve y suena ya; lo manual, si lo hay, sigue ahí
      const kept = r.hadOwn
        ? '. Tu clic manual de esta canción sigue como lo dejaste: pulsa «Manual» para volver a él'
        : ''
      return {
        text: `Automático analizado otra vez: ${auto}${kept}${weak}`,
        kind: r.fixed || r.snapped ? 'ok' : 'info',
        secs: r.hadOwn || weak ? 9 : 5
      }
    }
    const now = tuned
      ? `En «Automático» ${tuned}`
      : r.fixed
        ? `En «Automático» ${beatsWord(r.fixed)} ${r.fixed === 1 ? 'cambió' : 'cambiaron'}`
        : ''
    if (!r.answered) {
      return {
        text: `Tu clic manual se queda como estaba: este núcleo no lo re-analiza. ${now ? now + '. ' : ''}Lo nuevo está en «Automático»${weak}`,
        kind: 'info',
        secs: 9
      }
    }
    const mine = `${r.windows} ${r.windows === 1 ? 'zona tuya' : 'zonas tuyas'}`
    if (r.changed) {
      const respected = r.windows === 1 ? `se respetó ${mine}` : `se respetaron ${mine}`
      return {
        text: `Clic manual re-analizado: ${beatsWord(r.changed)} ${r.changed === 1 ? 'cambió' : 'cambiaron'} fuera de lo que moviste tú; ${respected}${weak}`,
        kind: 'ok',
        secs: 8
      }
    }
    return {
      text: r.windows
        ? `Tu clic manual se queda igual: fuera de lo que moviste tú (${mine}) el nuevo análisis no cambió ningún pulso${now ? '. ' + now : ''}${weak}`
        : `Tu clic manual se queda igual: el nuevo análisis no cambió ningún pulso${now ? '. ' + now : ''}${weak}`,
      kind: 'info',
      secs: 8
    }
  }

  /** Volver a analizar el clic: de toda la canción o del tramo elegido. */
  async function reanalyze(ev) {
    const id = loadedFor.value
    if (!id) return
    const seg = study.value.loops.length
      ? [study.value.loops[0][0], study.value.loops.at(-1)[1]]
      : null
    const run = async (mode, range) => {
      // En Manual, con su clic hecho, se re-analiza lo que NO movió él: el
      // núcleo recibe sus pulsos con las zonas que tocó y devuelve el clic
      // mezclado (lo suyo, y lo demás con el análisis nuevo).
      const own = study.value.metronome?.own
      const manual = clickMode.value === 'own' && !!own?.beats?.length
      const sent = manual
        ? {
            beats: [...own.beats],
            bars: [...(own.bars || [])],
            meter: own.meter ?? 4,
            // un clic de antes de que se guardaran las zonas va sin ellas: el
            // núcleo deduce cuáles movió de lo que se diferencia de la rejilla
            // de entonces, y las devuelve para guardarlas
            ...(ownTouched(own) ? { touched: ownTouched(own) } : {})
          }
        : null
      notify('Analizando el clic otra vez…', 'info', 4)
      try {
        const r = await click.reanalyze(id, {
          mode,
          ...(range ? { start: range[0], end: range[1] } : {}),
          ...(sent ? { own: sent } : {})
        })
        const weak = Array.isArray(r?.report?.weak) ? r.report.weak : []
        click.setWeak(id, weak)
        click.setFit(id, r?.report?.fit)
        // el clic manual mezclado se guarda (el núcleo no escribe el estudio) y suena ya
        const merged = sent && r?.own ? cleanOwn(r.own, sent.touched ?? []) : null
        let changed = 0
        if (merged) {
          changed = beatChanges(sent.beats, merged.beats).changed
          // (los «1» de lo guardado, como se guardarían: de cada compás)
          const mine = cleanOwn(own)
          // (un clic de antes, sin zonas, se guarda aunque no cambie un pulso: así
          // las deducidas ya son suyas y no hay que volver a deducirlas)
          const same =
            changed === 0 &&
            merged.bars.join() === mine?.bars.join() &&
            merged.meter === mine?.meter &&
            ownTouched(own) !== null
          if (!same) {
            // en esta canción, por el camino de siempre (se puede deshacer); si ya
            // se pasó a otra (la batería tarda), se le escribe solo esto
            if (loadedFor.value === id) await edit.commitOwn(merged, 'Re-analizar', { final: true })
            else await saveOwnOf(id, merged)
          }
        }
        // el automático nuevo, al motor y a la onda (si es el que suena)
        if (loadedFor.value === id) {
          edit.forgetAuto()
          await click.reload(r?.grid || null)
        }
        const note = reanalysisNote({
          fixed: r?.report?.fixed || 0,
          weak: loadedFor.value === id ? click.weak.value.regions : weak,
          hadOwn: !!own?.beats?.length,
          manual,
          answered: !!merged,
          snapped: Number(r?.report?.snapped) || 0,
          shiftMs: Number(r?.report?.shift_ms) || 0,
          changed,
          windows: merged?.touched?.length ?? 0
        })
        notify(note.text, note.kind, note.secs)
      } catch (e) {
        notify('No se pudo analizar: ' + errorMessage(e))
      }
    }
    const items = [
      {
        label: 'Arreglar huecos y saltos',
        icon: 'sparkles',
        note: 'toda la canción',
        action: () => run('repair', null)
      },
      {
        // (no es el modo «Tempo fijo» del clic, que es uno para todas las canciones)
        label: 'Tempo constante',
        icon: 'note',
        note: 'para las grabadas a clic',
        action: () => run('steady', null)
      },
      {
        label: 'Con la batería separada',
        icon: 'mixer',
        note: ctx.stems.hasStems.value ? 'la más precisa' : 'sepárala antes',
        disabled: !ctx.stems.hasStems.value,
        action: () => run('drums', null)
      },
      {
        label: 'Solo con la canción',
        icon: 'note',
        // al separar, DanPlay rellena solo los huecos sin clic con la batería
        // (si no cambia nada de lo que ya sonaba): esto lo deshace
        note: 'sin la batería separada',
        hint: 'Deshace lo que la batería separada añadió al clic, también lo que DanPlay añadió solo al separar',
        action: () => run('mix', null)
      },
      {
        label: 'Rellenar silencios',
        icon: 'plus',
        note: 'que no se calle en las partes suaves',
        action: () => run('fill', null)
      }
    ]
    if (seg) {
      items.push({ separator: true })
      items.push({
        label: 'Solo el tramo elegido',
        icon: 'repeat',
        note: `${formatTime(seg[0])} – ${formatTime(seg[1])}`,
        action: () => run('repair', seg)
      })
      items.push({
        label: 'El tramo, a tempo constante',
        icon: 'note',
        note: `${formatTime(seg[0])} – ${formatTime(seg[1])}`,
        action: () => run('steady', seg)
      })
    }
    const manual = clickMode.value === 'own' && hasOwn.value
    openMenu(
      ev,
      items,
      manual
        ? 'Re-analizar tu clic manual (se respeta lo que moviste tú)'
        : hasOwn.value
          ? 'Re-analizar el clic automático (tu clic manual no se toca)'
          : 'Volver a analizar el clic'
    )
  }
  /**
   * Los pulsos del clic, los mismos que se ven en la onda: los de la canción
   * (con el doble o la mitad, y sus cambios de tempo) o, con el tempo a mano,
   * uno cada tanto. A ellos se pegan las guías con el imán.
   */
  const clickBeats = computed(() => {
    if (soundingGrid.value?.beats?.length) return soundingGrid.value.beats
    const m = metronome.value
    return m.free && m.bpm ? clickTimes(null, m, duration.value || 0).beats : []
  })

  return {
    CLICK_MODES,
    modeTitle,
    chooseMode,
    shownGrid,
    soundingGrid,
    bpmDraft,
    bpmShown,
    clickPercent,
    clickLoading,
    hasOwn,
    clickLane,
    weakRegions,
    fitTitle,
    metroStatus,
    toggleMetronome,
    onClickLane,
    bumpBpm,
    onBpmFocus,
    onBpmInput,
    commitBpm,
    onBpmKey,
    followSong,
    setMeter,
    shiftOne,
    setMult,
    multTitle,
    setMetroVolume,
    editingBeats,
    edit,
    onOwnGrid,
    resetOwn,
    startEditing,
    COUNT_INS,
    CLICK_SOUNDS,
    CLICK_MAX,
    setCountIn,
    setClickSound,
    clickMore,
    setSongVolume,
    reanalyze,
    clickBeats
  }
}
