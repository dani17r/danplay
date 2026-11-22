// @ts-check
// Las guías habladas del modo estudio (StudyBar.vue, StudyCueList.vue): una
// frase que suena en un punto de la canción (escrita, y dicha con una voz, o
// grabada con el micrófono). Las hace el núcleo (`/api/cues`) y suenan en el
// motor en su sitio, también a otra velocidad y en la vuelta de un tramo, por
// la salida de las guías. Con sus carriles, el imán, la guía automática y la
// voz.
import { ref, computed, watch, shallowRef } from 'vue'
import { api, errorMessage, pickAudio, JOBS } from '../api.js'
import { usePlayback } from './usePlayback.js'
import { notify } from './useNotices.js'
import { ask } from './useDialog.js'
import { openMenu } from './useContextMenu.js'
import { isMobileApp } from '../platform.js'
import { formatTime } from '../utils/format.js'
import { nearestBeat } from '../utils/beats.js'
import { clamp, round2, round3 } from '../utils/num.js'
import { readPref, savePref } from '../utils/prefs.js'
import { MAX_CUE_LANES } from '../utils/cues.js'

/**
 * Se arma después del clic (sus pulsos son los del imán) y antes de las
 * pistas separadas, que reparten «Salidas» con los carriles de las guías.
 * @param {import('./studyContext.js').StudyContext} ctx
 */
export function useStudyCues(ctx) {
  const player = usePlayback()
  const { track, position, duration, metronome, playing } = player
  const { study, loadedFor, scheduleSave } = ctx.doc
  const { click, routing, timeline } = ctx
  const clickAuto = click.auto
  const clickMode = click.mode
  const grid = click.grid
  const { stems } = ctx.stems
  const { soundingGrid, hasOwn, clickBeats } = ctx.metro
  const { jump } = ctx.markers

  const CUE_VOLUME_KEY = 'danplay.cues.volume'
  const cueVolume = ref(clamp(Number(readPref(CUE_VOLUME_KEY, 1)) || 1, 0, 3))
  watch(cueVolume, (v) => {
    savePref(CUE_VOLUME_KEY, v)
    sendCues()
  })
  /**
   * Los carriles de las guías: los guardados, o uno («Guías») si no hay.
   * Como los de las pistas separadas: cada uno con su volumen, callado o solo,
   * y su salida (en «Salidas», como una pista con papel de guía).
   */
  const guideLanes = computed(() =>
    study.value.cueLanes?.length
      ? study.value.cueLanes
      : [{ key: 'g1', name: 'Guías', gain: 1, mute: false, solo: false }]
  )
  /** El carril de una guía (sin carril, o uno que ya no está: el primero). */
  const laneOfCue = (c) =>
    guideLanes.value.some((l) => l.key === c.lane) ? c.lane : guideLanes.value[0].key
  /** Cómo se llama un carril de guías en el reparto (como una pista). */
  const laneRoute = (key) => 'cues_' + key
  /** Lo que reparte «Salidas»: las pistas separadas y los carriles de guías. */
  const routedParts = computed(() => [
    ...(stems.value?.tracks || []).map((t) => ({ key: t.source, role: t.role || '' })),
    ...guideLanes.value.map((l) => ({ key: laneRoute(l.key), role: 'guide' }))
  ])
  watch(
    () => JSON.stringify(routedParts.value),
    () => routing.setStems(routedParts.value)
  )
  /**
   * Las guías que suenan: las de los carriles que no están callados (o los
   * que están en solo), con el volumen de su carril.
   */
  function audibleCues() {
    const lanes = guideLanes.value
    const anySolo = lanes.some((l) => l.solo)
    const live = new Map(
      lanes.filter((l) => !l.mute && (!anySolo || l.solo)).map((l) => [l.key, l])
    )
    return (study.value.cues || [])
      .filter((c) => c.file && live.has(laneOfCue(c)))
      .map((c) => {
        const lane = live.get(laneOfCue(c))
        return { at: c.t, file: c.file, gain: (c.gain ?? 1) * (lane.gain ?? 1), lane: lane.key }
      })
  }
  function sendCues() {
    const path = player.state.path
    if (!path || !loadedFor.value) return Promise.resolve()
    const buses = routing.routing.value.stems
    const cues = audibleCues().map((c) => {
      const bus = buses[laneRoute(c.lane)]
      return { at: c.at, path: c.file, gain: c.gain, ...(bus != null ? { bus } : {}) }
    })
    return player.setCues(cues, cueVolume.value, path)
  }
  // otro reparto (un carril de guías a otra salida): las guías, con su bus nuevo
  watch(
    () => JSON.stringify(routing.routing.value.stems),
    () => sendCues()
  )
  /**
   * Las guías se ponen, se mueven y se cambian con la canción parada: sonando,
   * un doble clic o un arrastre sin querer movería lo que está sonando.
   */
  const cueEdit = computed(() => !!track.value && !playing.value)
  const STOP_FIRST = 'Para la canción para poner, mover o cambiar guías'
  /**
   * El imán: las guías se pegan a los pulsos del clic (los de la canción, que
   * siguen sus cambios de tempo) al ponerlas o moverlas. De quien toca: se
   * recuerda.
   */
  const SNAP_KEY = 'danplay.cues.magnet'
  const snapCues = ref(readPref(SNAP_KEY, false) === true)
  watch(snapCues, (v) => savePref(SNAP_KEY, v))
  /** Los pulsos del imán (vacío: sin imán, o la canción aún sin pulsos). */
  const snapBeats = computed(() => (snapCues.value ? clickBeats.value : []))
  const magnetTitle = computed(() =>
    !snapCues.value
      ? 'Imán: que las guías se peguen a los pulsos del clic al ponerlas o moverlas (siguen los cambios de tempo de la canción)'
      : clickBeats.value.length
        ? 'Imán puesto: las guías se pegan a los pulsos del clic (pulsa para moverlas libres)'
        : 'Imán puesto, pero esta canción aún no tiene los pulsos del clic: pon el clic o espera a que cargue'
  )
  /**
   * La onda y lo que dura cada guía (por su archivo), para verlas en sus
   * bloques. Superficial: se sustituye entera y son miles de picos que solo se
   * leen para pintar.
   */
  const cueWaves = shallowRef({})
  const askedWaves = new Set()
  watch(
    () => (isMobileApp ? [] : (study.value.cues || []).map((c) => c.file).filter(Boolean)),
    async (files) => {
      for (const file of files) {
        if (askedWaves.has(file)) continue
        askedWaves.add(file)
        try {
          const w = await api.cueWave(file)
          if (w?.peaks) cueWaves.value = { ...cueWaves.value, [file]: w }
        } catch {
          // sin su onda, el bloque se ve igual, con el largo a ojo
        }
      }
    },
    { immediate: true }
  )
  /** El diálogo de una guía: el instante, el texto y la voz (o grabar). */
  const cueDraft = ref(null)
  function addCue(t = position.value, lane = guideLanes.value[0].key) {
    if (!track.value) return
    if (playing.value) return notify(STOP_FIRST, 'info')
    cueDraft.value = {
      t: round2(nearestBeat(snapBeats.value, t)),
      text: '',
      voice: readPref('danplay.cues.voice', ''),
      cue: null,
      lane
    }
  }
  function editCue(c) {
    if (playing.value) return notify(STOP_FIRST, 'info')
    cueDraft.value = {
      t: c.t,
      text: c.text || '',
      voice: c.voice || '',
      cue: c,
      lane: laneOfCue(c)
    }
  }
  /** Arrastrada en su carril: a su sitio nuevo. */
  function moveCue(c, t) {
    if (playing.value) return
    c.t = round2(t)
    study.value.cues = [...(study.value.cues || [])].sort((a, b) => a.t - b.t)
    scheduleSave()
    sendCues()
  }
  /** Los carriles se guardan en cuanto se toca uno (el de fábrica pasa a ser de verdad). */
  function ownLanes() {
    if (!study.value.cueLanes?.length)
      study.value.cueLanes = guideLanes.value.map((l) => ({ ...l }))
    return study.value.cueLanes
  }
  function setCueLane(key, patch) {
    const lane = ownLanes().find((l) => l.key === key)
    if (!lane) return
    Object.assign(lane, patch)
    scheduleSave()
    sendCues()
  }
  function addCueLane() {
    const lanes = ownLanes()
    if (lanes.length >= MAX_CUE_LANES) return
    let n = 1
    while (lanes.some((l) => l.key === 'g' + n)) n++
    lanes.push({
      key: 'g' + n,
      name: 'Guías ' + (lanes.length + 1),
      gain: 1,
      mute: false,
      solo: false
    })
    scheduleSave()
  }
  async function renameCueLane(key) {
    const lane = ownLanes().find((l) => l.key === key)
    if (!lane) return
    const name = await ask({
      kind: 'prompt',
      title: 'Nombre del carril',
      message: 'Por ejemplo «Baterista», «Banda» o «Coros».',
      value: lane.name || 'Guías',
      okLabel: 'Guardar'
    })
    if (typeof name !== 'string') return
    lane.name = name.trim().slice(0, 40)
    scheduleSave()
  }
  async function removeCueLane(key) {
    const lanes = ownLanes()
    if (lanes.length < 2) return
    const lane = lanes.find((l) => l.key === key)
    if (!lane) return
    const mine = (study.value.cues || []).filter((c) => laneOfCue(c) === key)
    if (mine.length) {
      const ok = await ask({
        kind: 'confirm',
        title: 'Quitar el carril',
        danger: true,
        message: `«${lane.name || 'Guías'}» tiene ${mine.length === 1 ? 'una guía' : mine.length + ' guías'}: se quitan con él.`,
        okLabel: 'Quitar'
      })
      if (!ok) return
    }
    study.value.cueLanes = lanes.filter((l) => l.key !== key)
    study.value.cues = (study.value.cues || []).filter((c) => !mine.includes(c))
    scheduleSave()
    sendCues()
  }
  /**
   * Un audio corto del equipo como guía en ese punto (no más largo que la
   * canción): se trae y se abre en su editor, para recortarlo antes de ponerlo.
   */
  async function importCueAt(t = position.value, lane = guideLanes.value[0].key) {
    if (!track.value) return
    if (playing.value) return notify(STOP_FIRST, 'info')
    const path = await pickAudio()
    if (!path) return
    try {
      const r = await api.importCue(loadedFor.value, path, duration.value || undefined)
      cueDraft.value = {
        t: round2(nearestBeat(snapBeats.value, t)),
        text: '',
        voice: '',
        cue: null,
        lane,
        file: r.file,
        seconds: r.seconds
      }
    } catch (e) {
      notify('No se pudo importar: ' + errorMessage(e))
    }
  }
  /** Las opciones de un carril de guías (su ⋯ o un clic derecho en él, en `t`). */
  function cueLaneMenu(key, ev, t) {
    const lane = guideLanes.value.find((l) => l.key === key)
    if (!lane) return
    const at = t ?? position.value
    const stopped = cueEdit.value
    const items = [
      {
        label: 'Guía aquí…',
        icon: 'plus',
        note: stopped ? formatTime(at) : 'para la canción',
        disabled: !stopped,
        action: () => addCue(at, key)
      },
      {
        label: 'Importar un audio…',
        icon: 'import',
        note: stopped ? 'hasta ' + formatTime(duration.value || 0) : 'para la canción',
        disabled: !stopped,
        action: () => importCueAt(at, key)
      },
      ...((study.value.cues || []).some((c) => c.text && laneOfCue(c) === key)
        ? [
            {
              label: 'Cambiar la voz…',
              icon: 'speech',
              disabled: !stopped,
              action: () => changeVoice(key)
            }
          ]
        : []),
      ...(lane.auto
        ? [
            {
              label: 'Rehacer la guía automática',
              icon: 'sparkles',
              disabled: !stopped,
              action: autoGuide
            }
          ]
        : []),
      { separator: true },
      { label: 'Renombrar…', icon: 'pencil', action: () => renameCueLane(key) },
      {
        label: lane.mute ? 'Que suene' : 'Callarlo',
        icon: lane.mute ? 'volume' : 'mute',
        action: () => setCueLane(key, { mute: !lane.mute })
      },
      {
        label: 'Otro carril de guías',
        icon: 'plus',
        disabled: guideLanes.value.length >= MAX_CUE_LANES,
        note: guideLanes.value.length >= MAX_CUE_LANES ? `como mucho ${MAX_CUE_LANES}` : '',
        action: addCueLane
      },
      ...(guideLanes.value.length > 1
        ? [
            {
              label: 'Quitar el carril',
              icon: 'trash',
              danger: true,
              action: () => removeCueLane(key)
            }
          ]
        : [])
    ]
    openMenu(ev, items, lane.name || 'Guías')
  }
  /** Las opciones de una guía (clic derecho en su bloque). */
  function cueMenu(c, ev) {
    const stopped = cueEdit.value
    const others = guideLanes.value.filter((l) => l.key !== laneOfCue(c))
    const items = [
      { label: 'Escuchar', icon: 'play', action: () => previewCue(c) },
      {
        label: 'Editar…',
        icon: c.text ? 'pencil' : 'cut',
        note: stopped ? (c.text ? '' : 'recortar, fundidos, limpiar') : 'para la canción',
        disabled: !stopped,
        action: () => editCue(c)
      },
      { label: 'Ir aquí', icon: 'right', note: formatTime(c.t), action: () => jump(c.t) },
      ...(others.length
        ? [
            {
              label: 'Pasar a otro carril',
              icon: 'list',
              disabled: !stopped,
              children: others.map((l) => ({
                label: l.name || 'Guías',
                action: () => {
                  c.lane = l.key
                  study.value.cues = [...study.value.cues]
                  scheduleSave()
                  sendCues()
                }
              }))
            }
          ]
        : []),
      { separator: true },
      {
        label: 'Quitar',
        icon: 'trash',
        danger: true,
        disabled: !stopped,
        action: () => removeCue(c)
      }
    ]
    openMenu(ev, items, c.text ? '«' + c.text + '»' : 'Guía')
  }
  // ---- la guía automática y la voz de las guías
  /** El selector de voz abierto: {title, okLabel, current, resolve}, o null. */
  const voicePick = ref(null)
  /** Abre el selector de voz y espera: la voz elegida, o null si se cerró. */
  function chooseVoice({
    title = 'La voz de las guías',
    okLabel = 'Usar esta voz',
    current = ''
  } = {}) {
    return new Promise((resolve) => {
      voicePick.value?.resolve(null)
      voicePick.value = { title, okLabel, current, resolve }
    })
  }
  function onVoicePicked(id) {
    const p = voicePick.value
    voicePick.value = null
    savePref('danplay.cues.voice', id)
    p?.resolve(id)
  }
  function onVoiceClosed() {
    const p = voicePick.value
    voicePick.value = null
    p?.resolve(null)
  }
  /** La voz de siempre si ya está bajada; si no, se pregunta (y se recuerda). */
  async function readyVoice(okLabel) {
    const wanted = readPref('danplay.cues.voice', '')
    if (wanted) {
      try {
        const r = await api.cueVoices()
        if (r?.voices?.some((v) => v.id === wanted && v.installed)) return wanted
      } catch {
        /* se pregunta */
      }
    }
    return chooseVoice({ title: 'La voz de la guía', okLabel, current: wanted })
  }
  /**
   * Los compases del clic ajustado a mano (el «propio» de esta canción) y lo
   * que dura un pulso, para la guía automática. Solo esos: los del clic
   * automático los saca el núcleo de su análisis. Antes se mandaban siempre
   * «los "1" que se ven», y con «sin acento» puesto no se ve ninguno: salía
   * uno de cada cuatro pulsos, empezara donde empezara, y las partes caían
   * hasta tres pulsos fuera de su sitio.
   */
  function barsAndBeat() {
    // con el tempo a mano, el clic que suena es el regular: sus compases (los
    // de la detectada son justo los que se corrigió)
    const tempo = clickAuto.value && metronome.value.free && metronome.value.has_grid
    const g = tempo
      ? soundingGrid.value
      : clickMode.value === 'own' && hasOwn.value
        ? grid.value
        : null
    const beats = g?.beats || []
    const bars = (g?.bars || []).map((i) => beats[i]).filter((t) => Number.isFinite(t))
    const gaps = beats
      .slice(1)
      .map((t, i) => t - beats[i])
      .sort((a, b) => a - b)
    const beat = gaps.length ? gaps[Math.floor(gaps.length / 2)] : 0
    return { bars: bars.map(round3), beat: beat > 0.2 && beat < 2 ? round3(beat) : 0 }
  }
  /** Haciendo la guía automática. */
  const autoBusy = ref(false)
  /** Por dónde va (lo que dice su trabajo): «Esperando al clic… 40 %». */
  const autoStage = ref('')
  /**
   * La guía automática: DanPlay busca las partes de la canción (intro,
   * estrofas, coros, puente, solos…) y las dice con la voz justo antes de
   * cada una, en su propio carril («Partes»). Otra vez: la rehace.
   */
  async function autoGuide() {
    if (!track.value || !loadedFor.value || autoBusy.value) return
    if (playing.value) return notify(STOP_FIRST, 'info')
    const voice = await readyVoice('Hacer la guía')
    if (!voice) return
    const id = loadedFor.value
    autoBusy.value = true
    autoStage.value = ''
    notify('Buscando las partes de la canción…', 'info', 4)
    try {
      const { bars, beat } = barsAndBeat()
      let waiting = false
      const r = await api.runJob(
        () =>
          api.autoCues(id, {
            voice,
            ...(bars.length >= 8 ? { bars } : {}),
            ...(beat ? { beat } : {})
          }),
        // un trabajo por canción: el de otra ni se espera ni se confunde
        `${JOBS.autoGuide}-${id}`,
        {
          onProgress: (job) => {
            autoStage.value = job?.message || ''
            // pulsada antes de que cargue el clic: el núcleo lo espera (las
            // partes se ajustan a sus compases), y aquí se dice
            if (!waiting && autoStage.value.startsWith('Esperando al clic')) {
              waiting = true
              notify(
                'Esperando al clic de la canción para ajustar las partes a sus compases…',
                'info',
                6
              )
            }
          }
        }
      )
      // lo que llega dice de qué canción es: las partes de otra no se colocan
      if (r?.song != null && r.song !== id) {
        throw new Error('llegó la guía de otra canción: pídela otra vez')
      }
      if (loadedFor.value === id) placeAutoGuide(r, voice)
    } catch (e) {
      notify('No se pudo hacer la guía automática: ' + errorMessage(e))
    } finally {
      autoBusy.value = false
      autoStage.value = ''
    }
  }
  /** Las guías de las partes, a su carril (el de la guía automática, o uno nuevo). */
  function placeAutoGuide(r, voice) {
    const cues = r?.cues || []
    if (!cues.length) return notify('No encontré partes que decir en esta canción', 'info')
    const lanes = ownLanes()
    let lane = lanes.find((l) => l.auto)
    const empty = (l) => !(study.value.cues || []).some((c) => laneOfCue(c) === l.key)
    if (!lane && lanes.length === 1 && empty(lanes[0])) {
      lane = lanes[0]
      Object.assign(lane, { name: 'Partes', auto: true })
    }
    if (!lane) {
      if (lanes.length >= MAX_CUE_LANES) {
        return notify(
          `Como mucho ${MAX_CUE_LANES} carriles de guías: quita uno para la guía automática`
        )
      }
      let n = 1
      while (lanes.some((l) => l.key === 'g' + n)) n++
      lane = { key: 'g' + n, name: 'Partes', gain: 1, mute: false, solo: false, auto: true }
      lanes.push(lane)
    }
    const key = lane.key
    study.value.cues = [
      ...(study.value.cues || []).filter((c) => laneOfCue(c) !== key),
      ...cues.map((c) => ({
        t: round2(c.t),
        text: c.text,
        voice: c.voice || voice,
        file: c.file,
        lane: key
      }))
    ].sort((a, b) => a.t - b.t)
    scheduleSave()
    sendCues()
    const names = (r.sections || []).map((s) => s.label)
    // de dónde salieron las partes: de lo más fiable a lo menos
    const from =
      {
        sheet: 'con los nombres del cifrado, casado con la letra',
        lyrics: 'por la letra',
        sequence: 'las de la secuencia',
        sound: 'por cómo suena (sin letra con tiempos de esta grabación: es aproximada)'
      }[r.source] || ''
    // sin los compases del clic (no llegó a tiempo, o aquí no lo hay) las
    // partes no se ajustan al «1»: se dice, que salen distintas
    const loose =
      r.click === false
        ? `. Sin el clic de la canción${r.click_why ? ' (' + r.click_why + ')' : ''}: las partes no van ajustadas al compás`
        : ''
    notify(
      `Guía lista${from ? ', ' + from : ''}: ${names.slice(0, 5).join(', ')}${names.length > 5 ? '…' : ''}${loose}`,
      'ok',
      r.source === 'sound' || loose ? 9 : 6
    )
  }
  /** Decir otra vez con otra voz las guías escritas (de un carril, o todas). */
  async function changeVoice(laneKey = null) {
    const cues = (study.value.cues || []).filter(
      (c) => c.text && (laneKey == null || laneOfCue(c) === laneKey)
    )
    if (!cues.length) return notify('Aquí no hay guías escritas a las que cambiar la voz', 'info')
    if (playing.value) return notify(STOP_FIRST, 'info')
    const voice = await chooseVoice({
      title: 'Cambiar la voz',
      okLabel: 'Cambiar la voz',
      current: cues[0].voice || ''
    })
    if (!voice) return
    const id = loadedFor.value
    notify(`Diciéndolas con la voz nueva (${cues.length})…`, 'info', 3)
    let done = 0
    for (const c of cues) {
      try {
        const r = await api.makeCue(id, { text: c.text, voice })
        c.file = r.file
        c.voice = voice
        done++
      } catch (e) {
        notify('No se pudo cambiar la voz: ' + errorMessage(e))
        break
      }
    }
    if (!done) return
    study.value.cues = [...study.value.cues]
    scheduleSave()
    sendCues()
    notify(`${done === 1 ? 'Una guía' : done + ' guías'} con la voz nueva`, 'ok', 4)
  }
  /** Las guías en la lista, por orden (como suenan). */
  const cueList = computed(() => [...(study.value.cues || [])].sort((a, b) => a.t - b.t))
  /** Desde la lista: la canción va ahí y la guía se ve (y destella) en su carril. */
  function goToCue(c) {
    jump(c.t)
    timeline.value?.focusCue?.(c)
  }
  /** La guía ya hecha (por el diálogo): a la canción, y a sonar. */
  async function onCueMade({ t, text, voice, file, cue }) {
    const lane = cueDraft.value?.lane || (cue ? laneOfCue(cue) : guideLanes.value[0].key)
    const list = (study.value.cues || []).filter((c) => c !== cue)
    list.push({
      t: round2(t),
      text,
      voice,
      file,
      lane,
      ...(cue?.gain != null ? { gain: cue.gain } : {})
    })
    study.value.cues = list.sort((a, b) => a.t - b.t)
    cueDraft.value = null
    if (voice) savePref('danplay.cues.voice', voice)
    scheduleSave()
    await sendCues()
  }
  function removeCue(c) {
    study.value.cues = (study.value.cues || []).filter((x) => x !== c)
    scheduleSave()
    sendCues()
  }
  async function previewCue(c) {
    try {
      await api.previewCue(cuePath(c))
    } catch (e) {
      notify(errorMessage(e))
    }
  }
  /**
   * El archivo de una guía. Las que vinieron de la PC en un .danpista dicen
   * dónde van dentro de la carpeta de la pista (`guias/…`).
   */
  function cuePath(c) {
    const f = String(c.file || '')
    const base = player.state.path || ''
    return f && !f.startsWith('/') && !/^[A-Za-z]:[\\/]/.test(f) && base ? base + '/' + f : f
  }

  return {
    cueVolume,
    guideLanes,
    laneOfCue,
    laneRoute,
    routedParts,
    audibleCues,
    sendCues,
    cueEdit,
    STOP_FIRST,
    snapCues,
    snapBeats,
    magnetTitle,
    cueWaves,
    cueDraft,
    addCue,
    editCue,
    moveCue,
    setCueLane,
    addCueLane,
    renameCueLane,
    removeCueLane,
    importCueAt,
    cueLaneMenu,
    cueMenu,
    voicePick,
    onVoicePicked,
    onVoiceClosed,
    autoBusy,
    autoStage,
    autoGuide,
    changeVoice,
    cueList,
    goToCue,
    onCueMade,
    removeCue,
    previewCue
  }
}
