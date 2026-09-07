<script setup>
import { ref, nextTick, onMounted, onUnmounted } from 'vue'
import { api } from '../api.js'
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

// Mientras el asistente trabaja puede estar bajando musica. Se mira el mismo
// estado que enseña la pagina de Descargas, para no dejar al usuario a ciegas.
const downloading = ref(null)
let poll = null
function watchDownload () {
  if (poll) return
  poll = setInterval(async () => {
    try {
      const e = await api.youtube()
      downloading.value = e.active ? e : null
    } catch { downloading.value = null }
  }, 900)
}
function stopWatching () {
  if (poll) { clearInterval(poll); poll = null }
  downloading.value = null
}
onUnmounted(stopWatching)

onMounted(async () => {
  try { info.value = await api.chatTools() } catch {}
  const guardado = localStorage.getItem('danplay.chat')
  if (guardado) { try { messages.value = JSON.parse(guardado) } catch {} }
  scrollToBottom()
})

function save () {
  try { localStorage.setItem('danplay.chat', JSON.stringify(messages.value.slice(-60))) } catch {}
}
async function scrollToBottom () {
  await nextTick()
  if (thread.value) thread.value.scrollTop = thread.value.scrollHeight
}

async function send (text = null) {
  const t = (text ?? entrada.value).trim()
  if (!t || thinking.value) return
  entrada.value = ''
  messages.value.push({ role: 'me', text: t })
  thinking.value = true
  watchDownload()
  scrollToBottom()
  try {
    // copia: el historial sigue creciendo mientras esperamos la respuesta
    const r = await api.chat(messages.value.map(m => ({ role: m.role, text: m.text })))
    if (r.error) {
      messages.value.push({ role: 'ai', text: r.error, error: true })
    } else {
      messages.value.push({ role: 'ai', text: r.text, tools: r.tools || [] })
      // OJO: son los nombres de las herramientas tal y como estan hoy. Estaban
      // los viejos en castellano y por eso la lista nunca se refrescaba.
      const changesLibrary = ['create_playlist', 'add_to_playlist', 'download_music',
                              'edit_song', 'set_stars', 'set_favorite', 'delete_song',
                              'delete_playlist', 'remove_from_playlist',
                              'find_lyrics_and_cover']
      if ((r.tools || []).some(h => changesLibrary.includes(h.name))) emit('reload')
      // reproducir no se puede hacer desde Python: el nucleo devuelve la orden
      // y la ejecuta la app
      for (const a of r.actions || []) emit('action', a)
    }
  } catch (e) {
    messages.value.push({ role: 'ai', text: 'No pude responder: ' + e, error: true })
  } finally {
    thinking.value = false; stopWatching(); save(); scrollToBottom()
  }
}

function clearChat () {
  if (!messages.value.length || confirm('¿Borrar la conversacion?')) {
    messages.value = []; save()
  }
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

      <div v-for="(m,i) in messages" :key="i" class="chat-msg" :class="[m.role, {error: m.error}]">
        <div v-if="m.tools?.length" class="chat-tools">
          <span v-for="(h,j) in m.tools" :key="j" class="chip">
            <Icon n="check" :t="11" /> {{ toolLabel(h.name) }} · {{ h.summary }}
          </span>
        </div>
        <div class="chat-bubble">{{ m.text }}</div>
      </div>

      <div v-if="thinking" class="chat-msg ai">
        <div v-if="downloading" class="chat-bubble chat-downloading">
          <span class="spinner"></span>
          <span class="chat-downloading-txt">
            {{ PHASES[downloading.phase] || 'Bajando' }}
            <span v-if="downloading.name"> · {{ downloading.name }}</span>
          </span>
          <span class="mono sub">{{ (downloading.percent || 0).toFixed(0) }}%</span>
        </div>
        <div v-else class="chat-bubble chat-thinking">
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
