// @ts-check
// Las pistas separadas del modo estudio (StudyBar.vue, StudyStems.vue):
// batería, voces, bajo… cada una con su volumen. Separarla, que suenen ellas
// con su mezcla, borrarlas, y exportar la canción o la mezcla a un archivo.
import { ref, computed, shallowRef, onUnmounted } from 'vue'
import { api, app as tauriApp, errorMessage, pickSavePath, JOBS } from '../api.js'
import { usePlayback } from './usePlayback.js'
import { useSeparation } from './useSeparation.js'
import { notify } from './useNotices.js'
import { ask } from './useDialog.js'
import { openMenu } from './useContextMenu.js'
import { takeToPhone } from './usePhone.js'
import { isMobileApp } from '../platform.js'
import { stemPlan, mixFileName } from '../utils/stems.js'
import { formatTime } from '../utils/format.js'
import { toneWords } from '../utils/theory.js'
import { clickTimes } from '../utils/beats.js'

/**
 * Se arma arriba del todo: `loadFor` quita las pistas de la canción que se
 * deja nada más empezar. Las guías (`ctx.cues`) y el clic (`ctx.metro`) se
 * arman después: se leen al usarlos. El `watch` del reparto que vuelve a
 * mandar la mezcla se queda en StudyBar, detrás del de las guías (los dos
 * miran lo mismo y avisan por orden de creación).
 * @param {import('./studyContext.js').StudyContext} ctx
 */
export function useStudyStems(ctx) {
  const player = usePlayback()
  const { track, duration, speed, pitch, metronome } = player
  const { study, loadedFor, scheduleSave, flushSave } = ctx.doc
  const { routing } = ctx

  const separation = useSeparation()
  /** Lo que dice el núcleo de las pistas de la canción, o null si no tiene. */
  const stems = shallowRef(null)
  /** El archivo de la canción, para el nombre de la mezcla que se guarda. */
  const songFile = ref('')
  /** Se pidió separar desde aquí esta canción: al acabar, suenan sus pistas. */
  let wantStems = null
  const hasStems = computed(() => !!stems.value?.complete)
  const mixerOn = computed(() => hasStems.value && !!study.value.mixer?.on)
  /** Cómo va cada pista (lo guardado, o como viene) y si suena. */
  const plan = computed(() => stemPlan(stems.value?.tracks || [], study.value.mixer?.tracks || {}))
  /** Los carriles de la onda: solo con las pistas sonando. */
  const lanes = computed(() => (mixerOn.value ? plan.value : []))
  const progress = computed(() => separation.progressOf(track.value?.id))
  const canSeparate = computed(() => separation.state.ok)
  /**
   * Se está separando OTRA canción: esta irá a la cola y empezará cuando
   * acabe. Antes el botón decía «Separar pistas» como si empezara ya.
   */
  const busyWith = computed(() => {
    const cur = separation.state.current
    if (!cur || cur.id === track.value?.id) return null
    const pct = cur.total ? Math.round(((cur.done || 0) / cur.total) * 100) : 0
    return { title: cur.title || 'otra canción', pct, waiting: separation.state.queue.length }
  })

  async function loadStems(id) {
    // sin pistas (404), o el núcleo no contesta: null
    const info = await api.stems(id).catch(() => null)
    if (loadedFor.value !== id) return
    stems.value = info
    if (wantStems === id && info?.complete) {
      wantStems = null
      study.value.mixer.on = true
      scheduleSave()
    }
    await routing.setStems(ctx.cues.routedParts.value)
    await sendMix()
  }
  // al acabar cada pasada de la canción que se estudia, sus pistas: las
  // rápidas en cuanto están, y luego las buenas (con los mismos nombres: el
  // motor ve que cambiaron y entran a la par, sin cortar)
  const stopListening = separation.onSeparated((ids) => {
    const id = loadedFor.value
    if (id != null && ids.includes(id)) loadStems(id)
  })
  onUnmounted(stopListening)

  /** Manda a Rust lo que tiene que sonar: las pistas con su mezcla, o la canción. */
  async function sendMix() {
    if (mixerOn.value) {
      await player.setStems(
        plan.value.map((t) => ({
          path: t.path,
          gain: t.gain,
          pan: t.pan,
          on: t.on,
          bus: routing.busOfStem(t.key)
        }))
      )
    } else if (player.stems.value) {
      await player.setStems(null)
    }
  }
  async function toggleStems() {
    if (!hasStems.value) return
    study.value.mixer.on = !study.value.mixer.on
    scheduleSave()
    await sendMix()
  }
  /** Un carril cambió: callar, dejar sola, volumen o panorama. */
  async function onLane(key, patch) {
    const all = study.value.mixer.tracks
    all[key] = { ...(all[key] || {}), ...patch }
    study.value.mixer.tracks = { ...all }
    scheduleSave()
    await sendMix()
  }
  /** Que vuelvan a sonar todas, como vienen. */
  async function resetMix() {
    study.value.mixer.tracks = {}
    scheduleSave()
    await sendMix()
  }
  /** Separa la canción que se estudia, con la mejor calidad (o a la cola). */
  /** La máxima calidad: más instrumentos, sobre las pistas buenas. */
  async function separateMore() {
    const t = track.value
    if (!t) return
    if (
      await separation.request([{ id: t.id, title: t.title, file: songFile.value }], { max: true })
    )
      wantStems = t.id
  }
  async function separateThis() {
    const t = track.value
    if (!t) return
    if (await separation.request([{ id: t.id, title: t.title, file: songFile.value }])) {
      wantStems = t.id
    }
  }
  const progressText = computed(() => {
    const p = progress.value
    if (!p) return ''
    const verb = p.stage === 'refine' ? 'mejorando' : 'separando'
    if (p.waiting) {
      return p.stage === 'refine'
        ? `en la cola para mejorar (${p.place}.º)`
        : `en la cola: empieza cuando acabe la que se está separando (${p.place}.º)`
    }
    if (p.step === 'download') return `bajando el separador · ${Math.round(p.fraction * 100)} %`
    if (!p.fraction) return p.stage === 'refine' ? 'mejorando…' : 'preparando…'
    const left = separation.remaining()
    const eta = left == null ? '' : ` · quedan ~${formatTime(left)}`
    return `${verb} · ${Math.round(p.fraction * 100)} %${eta}`
  })

  /**
   * Exporta a un archivo la canción (tal cual) o las pistas (como estén
   * mezcladas), y con las guías habladas si las tiene y se quieren: antes de
   * elegir dónde, una casilla lo pregunta. `asHeard`: además a la velocidad y
   * el tono de ahora, y con el clic si suena.
   * @param {{from?: 'song'|'stems', asHeard?: boolean}} [opts]
   */
  /** La ventana de exportar: abierta, y con qué se abre elegido ('song' o 'mix'). */
  const exportOpen = ref(false)
  const exportInitial = ref('song')
  function openExport(what = 'song') {
    exportInitial.value = what
    exportOpen.value = true
  }
  /**
   * Guarda en un archivo la canción (`from: 'song'`) o la mezcla de las pistas
   * (`'stems'`), con lo elegido en la ventana de exportar: las guías, el clic,
   * la velocidad y el tono (`heard`) y los canales (cada cosa por su lado, en
   * estéreo o en mono, como suena).
   */
  async function saveMix({
    from = 'stems',
    guides: withGuides = false,
    click: withClick = false,
    heard = false,
    channels = false
  } = {}) {
    exportOpen.value = false
    const { audibleCues, laneRoute, cueVolume } = ctx.cues
    const { shownGrid } = ctx.metro
    const info = stems.value
    const id = loadedFor.value
    if (!id || (from === 'stems' && !info)) return
    const on = from === 'stems' ? plan.value.filter((t) => t.on) : []
    if (from === 'stems' && !on.length) {
      return notify('Todas las pistas están calladas: no hay nada que guardar')
    }
    /** Por dónde va cada cosa en el archivo, si se pide con los canales. */
    const side = (kind, key = '', role = '') =>
      channels ? routing.exportRoute(kind, key, role) : {}
    const tracks = (
      from === 'stems'
        ? on.map((t) => {
            const r = side('stem', t.key, t.role || '')
            return r && { source: t.key, gain: t.gain, pan: t.pan, ...r }
          })
        : [((r) => r && { source: 'song', gain: 1, pan: 0, ...r })(side('song'))]
    ).filter(Boolean)
    if (!tracks.length) return notify('Con esos canales no suena nada: no hay nada que guardar')
    const guides = withGuides
      ? audibleCues()
          .map((c) => {
            const r = side('stem', laneRoute(c.lane), 'guide')
            return (
              r && {
                at: c.at,
                file: c.file,
                gain: Math.round(c.gain * cueVolume.value * 100) / 100,
                ...(r.side ? { side: r.side } : {})
              }
            )
          })
          .filter(Boolean)
      : []
    // el clic: los golpes que se oyen, con su sonido
    const m = metronome.value
    const clickSide = side('click')
    const clickTrack =
      withClick && m.on && clickSide
        ? {
            ...clickTimes(shownGrid.value, m, duration.value || 0),
            sound: m.sound || 'clasico',
            volume: m.volume,
            ...(clickSide.side ? { side: clickSide.side } : {})
          }
        : null
    const name = mixFileName(
      songFile.value || track.value?.title || 'Cancion',
      from === 'stems' ? plan.value : [],
      { click: !!clickTrack, guides: guides.length > 0 }
    )
    // junto a las pistas, o junto a la canción
    const songDir = (player.state.path || '').replace(/[\\/][^\\/]*$/, '')
    const folder = from === 'stems' ? info.folder : songDir
    const path = await pickSavePath({
      title: from === 'stems' ? 'Guardar la mezcla' : 'Guardar la canción',
      defaultPath: folder ? `${folder}/${name}.mp3` : `${name}.mp3`,
      filters: [
        { name: 'MP3', extensions: ['mp3'] },
        { name: 'FLAC', extensions: ['flac'] },
        { name: 'WAV', extensions: ['wav'] }
      ]
    })
    if (!path) return
    notify(from === 'stems' ? 'Guardando la mezcla…' : 'Guardando la canción…', 'info', 3)
    try {
      const r = await api.runJob(
        () =>
          api.exportMix(id, {
            tracks,
            path,
            ...(heard ? { speed: speed.value, pitch: pitch.value } : {}),
            ...(clickTrack ? { click: clickTrack } : {}),
            ...(guides.length ? { cues: guides } : {})
          }),
        JOBS.mix,
        { busy: 'aún se está guardando la anterior: espera a que termine' }
      )
      notify(`Guardada «${r.name}»${r.id ? ': ya está en tu biblioteca' : ''}`, 'ok', 6)
    } catch (e) {
      notify('No se pudo guardar la mezcla: ' + errorMessage(e))
    }
  }
  /** Al celular, con lo último que se cambió del estudio ya guardado. */
  async function toPhone() {
    const id = loadedFor.value
    if (!id) return
    await flushSave()
    await takeToPhone([{ id, title: track.value?.title }])
  }
  async function revealStems() {
    const first = stems.value?.tracks?.[0]?.path
    if (!first) return
    try {
      await tauriApp.revealInFolder(first)
    } catch (e) {
      notify(errorMessage(e))
    }
  }
  async function deleteStems() {
    const id = loadedFor.value
    if (!id || !stems.value) return
    const ok = await ask({
      kind: 'confirm',
      title: 'Borrar las pistas separadas',
      danger: true,
      message: 'Las pistas van a la papelera del sistema. La canción no se toca.',
      detail: stems.value.folder,
      okLabel: 'A la papelera'
    })
    if (!ok) return
    try {
      if (player.stems.value) await player.setStems(null)
      await api.deleteStems(id)
      stems.value = null
      study.value.mixer = { on: false, tracks: {} }
      scheduleSave()
      notify('Pistas en la papelera', 'ok')
    } catch (e) {
      notify('No se pudieron borrar: ' + errorMessage(e))
    }
  }
  /** La velocidad y el tono que se oyen, si no son los de la canción («80 % · medio tono arriba»). */
  const heardNote = computed(() => {
    const bits = []
    if (speed.value !== 1 || pitch.value !== 0) {
      bits.push(
        `${Math.round(speed.value * 100)} %${pitch.value ? ' · ' + toneWords(pitch.value) : ''}`
      )
    }
    return bits.join(' · ')
  })
  function stemsMenu(ev) {
    // en el celular las pistas vienen hechas: ni se guardan mezclas, ni se
    // separan otra vez, ni hay carpeta que abrir
    if (isMobileApp) {
      return openMenu(
        ev,
        [{ label: 'Que suenen todas, como vienen', icon: 'refresh', action: resetMix }],
        'Las pistas'
      )
    }
    const items = [
      // exportar no va aquí: un solo «Exportar…», el del ⋯ del estudio, que con
      // las pistas sonando ya abre con su mezcla elegida
      { label: 'Que suenen todas, como vienen', icon: 'refresh', action: resetMix },
      { label: 'Abrir la carpeta', icon: 'folderOpen', action: revealStems },
      { separator: true },
      ...(progress.value
        ? []
        : [
            ...(stems.value?.best && !stems.value?.max && separation.state.max
              ? [
                  {
                    label: 'Más instrumentos',
                    icon: 'sparkles',
                    hint: 'Máxima calidad: percusión aparte, guitarras, teclados, cuerdas… (tarda)',
                    action: separateMore
                  }
                ]
              : []),
            {
              label: 'Separar otra vez',
              icon: 'mixer',
              hint: stems.value?.best
                ? ''
                : stems.value?.quality === 'rapida'
                  ? 'Con la mejor calidad: las de ahora son las rápidas'
                  : 'Con la mejor calidad: se hicieron con una versión anterior',
              action: separateThis
            }
          ]),
      { separator: true },
      { label: 'Borrar las pistas…', icon: 'trash', danger: true, action: deleteStems }
    ]
    openMenu(ev, items, 'Las pistas')
  }

  return {
    separation,
    stems,
    songFile,
    hasStems,
    mixerOn,
    plan,
    lanes,
    progress,
    canSeparate,
    busyWith,
    loadStems,
    sendMix,
    toggleStems,
    onLane,
    resetMix,
    separateMore,
    separateThis,
    progressText,
    exportOpen,
    exportInitial,
    openExport,
    saveMix,
    toPhone,
    revealStems,
    deleteStems,
    heardNote,
    stemsMenu
  }
}
