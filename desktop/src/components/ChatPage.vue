<script setup>
import { ref, computed, nextTick, onMounted, onUnmounted } from 'vue'
import { api, errorMessage } from '../api.js'
import { ask } from '../composables/useDialog.js'
import { useDownloads } from '../composables/useDownloads.js'
import { renderMarkdown } from '../utils/markdown.js'
import Icon from './Icon.vue'
import TextField from './ui/TextField.vue'

const emit = defineEmits(['reload', 'action'])

// las mismas que publica el nucleo en youtube.STATE
const PHASES = {
  starting: 'Preparando', downloading: 'Bajando', converting: 'Convirtiendo a mp3',
  filing: 'Identificando y archivando', done: 'Terminado', canceled: 'Cancelado'
}

const TOOL_LABELS = {
  search_songs: 'busco en tu biblioteca',
  library_summary: 'miro tu biblioteca',
  create_playlist: 'creo una lista',
  list_playlists: 'veo tus listas',
  add_to_playlist: 'añado a una lista',
  remove_from_playlist: 'quito de la lista',
  delete_playlist: 'borro la lista',
  get_lyrics: 'busco la letra',
  lyrics_by_name: 'busco la letra',
  find_lyrics_and_cover: 'busco letra y portada',
  music_details: 'miro tono y acordes',
  transpose_chords: 'transpongo',
  search_youtube: 'busco en YouTube',
  download_music: 'descargo',
  download_status: 'miro la descarga',
  search_web: 'busco en la web',
  play_song: 'pongo la cancion',
  play_playlist: 'pongo la lista',
  playlist_songs: 'miro la lista',
  set_playlist_songs: 'dejo la lista como debe',
  rename_playlist: 'renombro la lista',
  player_control: 'controlo el reproductor',
  set_stars: 'pongo estrellas',
  set_favorite: 'marco favorito',
  edit_song: 'corrijo los datos',
  delete_song: 'mando a la papelera'
}
const toolLabel = (n) => TOOL_LABELS[n] || n.replace(/_/g, ' ')
const messages = ref([])
const entrada = ref('')
const thinking = ref(false)
const info = ref(null)
const thread = ref(null)

const SUGGESTIONS = [
  '¿Que canciones tengo sin artista?',
  'Armame una lista para el domingo con temas lentos',
  'Busca en YouTube lo ultimo de Barak',
  '¿En que tono esta Mi Gozo de Barak?',
  '¿De que año es este disco y quien lo produjo?',
  'Pasa esos acordes a Sol'
]

// La descarga que se aprobo desde aqui corre en el nucleo, en segundo plano.
// El chat la sigue —el mismo estado que enseñan Descargas y la barra
// lateral, via useDownloads— y, al terminar, cuenta que entro, que ya tenias
// y que fallo. Antes se decia «te lo cuento en Descargas» y aqui no volvia a
// saberse nada.
//
// Lo pedido se guarda en localStorage: una descarga tarda minutos y es normal
// cambiar de pagina mientras tanto; al volver, se retoma el seguimiento y el
// resultado se cuenta igual.
const FOLLOW_KEY = 'danplay.chat.download'
const downloads = useDownloads()
const following = ref(null)
// la burbuja de avance solo para lo que se pidio desde aqui
const downloading = computed(() => (following.value && downloads.state.active ? downloads.state : null))

function rememberFollowing () {
  try {
    if (following.value) localStorage.setItem(FOLLOW_KEY, JSON.stringify(following.value))
    else localStorage.removeItem(FOLLOW_KEY)
  } catch { /* sin almacenamiento (modo privado) */ }
}
const stopFollowing = downloads.onFinished((s) => {
  if (!following.value) return
  const asked = following.value
  following.value = null
  rememberFollowing()
  // Solo la descarga que se pidio desde aqui: si mientras tanto se lanzo
  // otra desde la pagina de Descargas, esa no es nuestra y no se cuenta.
  const mine = (s.results || []).filter(r => !r.requested || asked.items.includes(r.requested))
  if (!mine.length && (s.results || []).length) return
  reportDownload({ ...s, results: mine })
})
onUnmounted(stopFollowing)

/** Una fila del resultado, legible: con lo que quedo archivada. */
function describe (r) {
  const name = r.song || r.title || r.source || r.requested || 'un tema'
  const who = r.artist ? `${r.artist} - ` : ''
  return `${who}${name}`
}

// Como se identifico, en palabras. El nombre con el que entra una descarga lo
// decide esto, no el titulo de YouTube; si no se dice, la persona (y el
// modelo) creen que se bajo otra cancion.
const IDENTIFIED = {
  tags: 'por sus etiquetas', fingerprint: 'por huella acústica',
  heuristic: 'por el nombre', ai: 'por la IA', none: 'sin identificar'
}
function requestedAs (r) {
  const asked = (r.source || r.title || '').trim()
  const filed = describe(r)
  if (!asked || asked.toLowerCase() === filed.toLowerCase()) return ''
  const how = IDENTIFIED[r.identified_by] ? ` (${IDENTIFIED[r.identified_by]})` : ''
  return ` — pediste «${asked}»; se archivó con ese nombre${how}`
}

/**
 * Lo que se cuenta al acabar la descarga aprobada. Va en markdown: son
 * listas de temas y se leen mejor con sus negritas.
 */
function reportDownload (e) {
  const results = e.results || []
  const ok = results.filter(r => r.ok)
  const already = results.filter(r => r.already_there)
  const failed = results.filter(r => !r.ok && !r.already_there)
  const lines = []
  if (e.phase === 'canceled') lines.push('Descarga cancelada.')
  if (ok.length) {
    lines.push(ok.length > 1 ? `**Descargadas (${ok.length}):**` : '**Descargada:**')
    for (const r of ok) {
      let where = ''
      if (r.action === 'review') where = ' → en *Revisar/*, sin artista claro'
      else if (r.forced) where = ' (otra versión)'
      lines.push(`- **${describe(r)}**${where}${requestedAs(r)}`)
    }
  }
  if (already.length) {
    lines.push('', already.length > 1 ? '**Ya las tenías, no las he bajado:**' : '**Ya la tenías, no la he bajado:**')
    for (const r of already) {
      const m = r.matches?.[0]
      const as = m ? ` — en tu biblioteca como «${m.artist ? m.artist + ' - ' : ''}${m.title}»` : ''
      lines.push(`- ${r.title || r.source || 'un tema'}${as}`)
    }
    lines.push('', already.length > 1
      ? 'Si las quieres igualmente como otra versión, dímelo y las bajo.'
      : 'Si la quieres igualmente como otra versión, dímelo y la bajo.')
  }
  if (failed.length) {
    lines.push('', '**No se pudo:**')
    for (const r of failed) lines.push(`- ${describe(r)}: ${r.reason || 'sin motivo conocido'}`)
  }
  if (!results.length && e.phase !== 'canceled') lines.push('No se ha bajado nada.' + (e.error ? ` ${e.error}` : ''))
  const parts = []
  parts.push(`${ok.length} descargada${ok.length === 1 ? '' : 's'}`)
  if (already.length) parts.push(`${already.length} ya la${already.length > 1 ? 's' : ''} tenías`)
  if (failed.length) parts.push(`${failed.length} con fallo`)
  messages.value.push({
    role: 'ai', text: lines.join('\n').trim(), app: true,
    tools: [{ name: 'download_music', summary: parts.join(', ') }]
  })
  if (ok.length) emit('reload')
  save(); scrollToBottom()
  // Si entro algo, se le pasa el turno al asistente: «descargame estas y
  // armame una lista» se quedaba a medias, porque el no se entera solo de
  // que la descarga acabo y la persona tenia que volver a pedirselo.
  //
  // Con los ids EXACTOS de lo que entro. Sin ellos, el modelo se los
  // inventaba y la lista salia con otras canciones.
  if (ok.length) {
    const ids = ok.filter(r => r.id).map(r => {
      const asked = (r.source || r.title || '').trim()
      return `pediste «${asked || r.requested || '?'}» → entro como «${describe(r)}» (id ${r.id})`
    }).join('; ')
    const notice = '[aviso de la app] La descarga ha terminado y ES la que pediste; el nombre con ' +
         'el que entra lo decide la identificacion, no YouTube. ' +
         (ids || 'No entro ninguna con id.') + '. Usa EXACTAMENTE esos ids y NO la vuelvas a descargar. ' +
         'Si en lo que te pedi quedaba algo por hacer con esas canciones (añadirla a una lista, ' +
         'ponerla a sonar…), hazlo AHORA con las herramientas —añadir a una lista no necesita ' +
         'confirmacion— y cuentamelo en una linea; no toques nada mas. ' +
         'Si no quedaba nada, responde solo: Terminado.'
    if (thinking.value) queued = { text: notice, event: 'download_done' }
    else send(notice, true, 'download_done')
  }
}

onMounted(async () => {
  try { info.value = await api.chatTools() } catch { /* sin almacenamiento (modo privado) */ }
  const guardado = localStorage.getItem('danplay.chat')
  if (guardado) { try { messages.value = JSON.parse(guardado) } catch { /* sin almacenamiento (modo privado) */ } }
  // una descarga pedida desde aqui que seguia en marcha al cambiar de pagina
  try {
    const pending = localStorage.getItem(FOLLOW_KEY)
    if (pending) { following.value = JSON.parse(pending); downloads.wake() }
  } catch { /* sin almacenamiento (modo privado) */ }
  scrollToBottom()
})

function save () {
  try { localStorage.setItem('danplay.chat', JSON.stringify(messages.value.slice(-60))) } catch { /* sin almacenamiento (modo privado) */ }
}
async function scrollToBottom () {
  await nextTick()
  if (thread.value) thread.value.scrollTop = thread.value.scrollHeight
}

/**
 * Lo que se manda al nucleo de cada mensaje. Las herramientas que uso el
 * asistente van tambien: con eso el nucleo marca en el historial que hizo de
 * verdad cada mensaje, y el modelo no se cree sus propias frases («ya la
 * cree») cuando no llamo a nada.
 */
function forCore (m) {
  const out = { role: m.role, text: m.text }
  if (m.tools?.length) out.tools = m.tools.map(h => ({ name: h.name, summary: h.summary }))
  // lo que escribio la app (un cancelado, un fallo, el arranque de una
  // descarga) no es una frase del modelo: el nucleo lo marca como tal
  if (m.app) out.app = true
  // y un aviso estructural (la descarga termino) se reconoce por su tipo,
  // no por su texto
  if (m.event) out.event = m.event
  return out
}

// El aviso de fin de descarga que no se pudo mandar porque el chat estaba
// ocupado: se manda en cuanto termine ese turno, no se pierde.
let queued = null

/**
 * `hidden`: un mensaje que manda la propia app en nombre del usuario (al
 * terminar una descarga, para que el asistente remate lo que quedaba). Va al
 * nucleo como cualquier otro, pero no se pinta como si lo hubieras escrito.
 * `event` lo etiqueta para el nucleo («download_done»).
 */
async function send (text = null, hidden = false, event = null) {
  const t = (text ?? entrada.value).trim()
  if (!t || thinking.value) return
  if (!hidden) entrada.value = ''
  const mine = { role: 'me', text: t }
  if (hidden) mine.hidden = true
  if (event) mine.event = event
  messages.value.push(mine)
  thinking.value = true
  scrollToBottom()
  try {
    // copia: el historial sigue creciendo mientras esperamos la respuesta
    const r = await api.chat(messages.value.map(forCore))
    if (r.error) {
      messages.value.push({ role: 'ai', text: r.error, error: true, app: true })
    } else {
      messages.value.push({ role: 'ai', text: r.text, tools: r.tools || [], narrated: !!r.narrated })
      // OJO: son los nombres de las herramientas tal y como estan hoy. Estaban
      // los viejos en castellano y por eso la lista nunca se refrescaba.
      // `download_music` no esta: en la conversacion solo se PIDE; la
      // biblioteca cambia cuando termina la descarga, y eso lo avisa
      // `reportDownload`.
      const changesLibrary = ['create_playlist', 'add_to_playlist', 'set_playlist_songs',
                              'rename_playlist', 'edit_song', 'set_stars', 'set_favorite',
                              'delete_song', 'delete_playlist', 'remove_from_playlist',
                              'find_lyrics_and_cover']
      if ((r.tools || []).some(h => changesLibrary.includes(h.name))) emit('reload')
      // reproducir no se puede hacer desde Python: el nucleo devuelve la orden
      // y la ejecuta la app
      for (const a of r.actions || []) emit('action', a)
      if (r.confirm) await confirmPending(r.confirm)
    }
  } catch (e) {
    messages.value.push({ role: 'ai', text: 'No pude responder: ' + errorMessage(e), error: true, app: true })
  } finally {
    thinking.value = false; save(); scrollToBottom()
    if (queued) { const q = queued; queued = null; send(q.text, true, q.event) }
  }
}

/**
 * Lo que no tiene vuelta atras no lo hace el modelo por su cuenta.
 *
 * El nucleo devuelve `confirm` con lo que iba a hacer en vez de hacerlo
 * (docs/CONTRATO-INTERNO.md §3): borrar una cancion, borrar un repertorio o
 * descargar. Aqui se pregunta, y solo si dices que si se ejecuta. Un texto
 * copiado de una pagina web o de un titulo de YouTube no puede borrarte nada.
 */
async function confirmPending (pending) {
  const ok = await ask({
    kind: 'confirm', title: 'El asistente quiere hacer esto', danger: true,
    message: pending.summary, okLabel: 'Adelante'
  })
  if (!ok) {
    messages.value.push({ role: 'ai', text: 'Cancelado, no he tocado nada.', app: true })
    save(); scrollToBottom()
    return
  }
  try {
    const r = await api.chatConfirm(pending.tool, pending.args)
    // con la herramienta apuntada: asi el nucleo le cuenta al modelo que la
    // descarga (o el borrado) se pidio y se acepto de verdad
    messages.value.push({ role: 'ai', text: r.text || 'Hecho.', app: true,
      tools: [{ name: pending.tool, summary: pending.tool === 'download_music' ? 'aceptada, en marcha' : 'hecho' }] })
    if (pending.tool === 'download_music' && r.result?.active) {
      // arranco en segundo plano: se sigue desde aqui y se cuenta al acabar
      following.value = { items: r.result.items || [], force: !!r.result.force }
      rememberFollowing()
      downloads.wake()
    } else {
      emit('reload')
    }
  } catch (e) {
    messages.value.push({ role: 'ai', text: 'No se pudo: ' + errorMessage(e), error: true, app: true })
  } finally {
    save(); scrollToBottom()
  }
}

async function clearChat () {
  if (!messages.value.length) return
  const ok = await ask({
    kind: 'confirm', title: 'Borrar la conversación',
    message: 'Se borra el historial del chat. Tu biblioteca no se toca.',
    okLabel: 'Borrar'
  })
  if (ok) { messages.value = []; save() }
}
</script>

<template>
  <div class="chat">
    <div class="chat-head">
      <Icon n="ai" :t="16" />
      <div style="flex:1;min-width:0">
        <strong>Asistente</strong>
        <span class="chat-model mono">{{ info?.model || '—' }}</span>
      </div>
      <button class="btn mini" @click="clearChat" v-if="messages.length">
        <Icon n="trash" :t="13" /></button>
    </div>

    <div class="chat-thread" ref="thread">
      <div v-if="!messages.length" class="chat-empty">
        <Icon n="ai" :t="34" />
        <p>Preguntame por tu musica. Busco en tu biblioteca, armo listas, traigo
           letras, saco acordes y los transpongo, busco en YouTube y en la web, y
           te bajo lo que me pidas: se convierte a mp3 y se archiva por artista.</p>
        <div class="chat-suggest">
          <button v-for="s in SUGGESTIONS" :key="s" class="chip" @click="send(s)">{{ s }}</button>
        </div>
        <p class="chat-note">Solo hablo de musica: canciones, artistas, generos,
           instrumentos, teoria e historia. Lo que se salga de ahi te lo dire.</p>
      </div>

      <div v-for="(m,i) in messages" :key="i" v-show="!m.hidden" class="chat-msg"
           :class="[m.role, {error: m.error, narrated: m.narrated}]"
           :title="m.narrated ? 'El asistente dice haber hecho algo, pero ninguna herramienta lo hizo' : undefined">
        <div v-if="m.tools?.length" class="chat-tools">
          <span v-for="(h,j) in m.tools" :key="j" class="chip">
            <Icon n="check" :t="11" /> {{ toolLabel(h.name) }} · {{ h.summary }}
          </span>
        </div>
        <!-- Lo del asistente viene en markdown y se pinta como tal. Lo tuyo y
             los errores van tal cual: son texto plano. `renderMarkdown`
             escapa todo antes de marcar nada (ver utils/markdown.js). -->
        <div v-if="m.role === 'ai' && !m.error" class="chat-bubble chat-md"
             v-html="renderMarkdown(m.text)"></div>
        <div v-else class="chat-bubble">{{ m.text }}</div>
      </div>

      <div v-if="downloading" class="chat-msg ai">
        <div class="chat-bubble chat-downloading">
          <span class="spinner"></span>
          <span class="chat-downloading-txt">
            {{ PHASES[downloading.phase] || 'Bajando' }}
            <span v-if="downloading.total > 1"> · {{ downloading.index || 1 }}/{{ downloading.total }}</span>
            <span v-if="downloading.name"> · {{ downloading.name }}</span>
          </span>
          <span class="mono sub">{{ (downloading.percent || 0).toFixed(0) }}%</span>
        </div>
      </div>

      <div v-if="thinking" class="chat-msg ai">
        <div class="chat-bubble chat-thinking">
          <span></span><span></span><span></span>
        </div>
      </div>
    </div>

    <div class="chat-foot">
      <TextField v-model="entrada" width="100%"
             placeholder="Escribe lo que necesites…" @enter="send()" />
      <!-- `next` es el icono de «cancion siguiente» del reproductor; aqui
           lo que se hace es enviar, no saltar de pista. -->
      <button class="btn primary" title="Enviar"
              :disabled="thinking || !entrada.trim()" @click="send()">
        <Icon n="send" :t="15" />
      </button>
    </div>
  </div>
</template>
