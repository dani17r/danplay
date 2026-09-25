<script setup>
/**
 * El asistente, a la vista.
 *
 * La conversación no vive aquí: vive en `useChat`, que sigue funcionando
 * aunque esta página no se vea (si sales mientras responde, lo que pidió se
 * hace igual). Aquí solo se pinta y se escribe.
 */
import { ref, watch, nextTick, onMounted, onActivated, onUnmounted, useTemplateRef } from 'vue'
import { api } from '../api.js'
import { useChat } from '../composables/useChat.js'
import { renderMarkdown } from '../utils/markdown.js'
import Icon from './Icon.vue'
import TextField from './ui/TextField.vue'

const chat = useChat()
const {
  messages,
  draft,
  thinking,
  live,
  info,
  chats,
  chatId,
  chatTitle,
  downloading,
  tryingFree,
  freeNote
} = chat

// las mismas que publica el nucleo en youtube.STATE
const PHASES = {
  starting: 'Preparando',
  downloading: 'Bajando',
  converting: 'Convirtiendo a mp3',
  filing: 'Identificando y archivando',
  done: 'Terminado',
  canceled: 'Cancelado'
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
  play: 'pongo música',
  play_song: 'pongo la canción',
  play_playlist: 'pongo la lista',
  playlist_songs: 'miro la lista',
  set_playlist_songs: 'dejo la lista como debe',
  rename_playlist: 'renombro la lista',
  player_control: 'controlo el reproductor',
  set_stars: 'pongo estrellas',
  set_favorite: 'marco favorito',
  edit_song: 'corrijo los datos',
  delete_song: 'mando a la papelera',
  related_keys: 'miro los tonos vecinos',
  setlist_sheet: 'escribo la hoja del repertorio'
}
const toolLabel = (n) => TOOL_LABELS[n] || n.replace(/_/g, ' ')

const SUGGESTIONS = [
  '¿Qué canciones tengo sin artista?',
  'Armame una lista para el domingo con temas lentos',
  'Busca en YouTube lo último de Barak',
  '¿En qué tono está Mi Gozo de Barak?',
  '¿De que año es este disco y quien lo produjo?',
  'Pasa esos acordes a Sol'
]

// El HTML de cada burbuja se calcula una vez por texto. Antes se volvia a
// pasar TODO el markdown de la conversacion en cada repintado, y mientras
// sonaba algo eso era cuatro veces por segundo.
const html = new Map()
const HTML_KEEP = 400
function md(text) {
  let out = html.get(text)
  if (out === undefined) {
    out = renderMarkdown(text)
    html.set(text, out)
    if (html.size > HTML_KEEP) html.delete(html.keys().next().value)
  }
  return out
}

/** Tokens y coste de una respuesta, en corto: «3,2k tokens · $0,002». */
function costLine(m) {
  const u = m.usage
  if (!u) return ''
  const n = (u.prompt || 0) + (u.completion || 0)
  const parts = [n >= 1000 ? (n / 1000).toFixed(1).replace('.', ',') + 'k tokens' : n + ' tokens']
  if (u.cost != null)
    parts.push(
      u.cost === 0
        ? 'gratis'
        : '$' + (u.cost < 0.01 ? u.cost.toFixed(4) : u.cost.toFixed(3)).replace('.', ',')
    )
  if (m.via?.fallback) parts.push(`respondió ${m.via.name} (respaldo)`)
  return parts.join(' · ')
}

// ------------------------------------------------ conversaciones guardadas
const showChats = ref(false)
const chatQuery = ref('')
const hits = ref([])
let searchTimer = null
watch(chatQuery, (q) => {
  clearTimeout(searchTimer)
  if (!q.trim()) {
    hits.value = []
    return
  }
  searchTimer = setTimeout(async () => {
    try {
      hits.value = (await api.chatSearch(q.trim())).hits || []
    } catch {
      hits.value = []
    }
  }, 250)
})
async function openChat(id) {
  await chat.openChat(id)
  showChats.value = false
}
function newChat() {
  chat.newChat()
  showChats.value = false
}

// ------------------------------------------------------------- el hilo
const thread = useTemplateRef('thread')
async function scrollToBottom() {
  await nextTick()
  if (thread.value) thread.value.scrollTop = thread.value.scrollHeight
}
// lo nuevo (un mensaje, lo que va llegando, la barra de la descarga) se ve
watch(
  [
    () => messages.value.length,
    () => live.value?.text,
    () => live.value?.tools.length,
    thinking,
    () => !!downloading.value
  ],
  scrollToBottom
)

async function appear() {
  await chat.show()
  scrollToBottom()
}
onMounted(appear)
// La app la guarda en <KeepAlive> al salir: al volver no se monta otra vez,
// se reactiva (y onActivated salta también la primera vez, justo después de
// montarse: esa ya la hizo onMounted).
let firstActivation = true
onActivated(() => {
  if (firstActivation) firstActivation = false
  else appear()
})
onUnmounted(() => clearTimeout(searchTimer))
</script>

<template>
  <div class="chat">
    <div class="chat-head">
      <Icon n="ai" :t="16" />
      <div style="flex: 1; min-width: 0">
        <span class="chat-title-row">
          <strong class="chat-title">{{ chatId ? chatTitle : 'Asistente' }}</strong>
          <button
            v-if="chatId"
            type="button"
            class="field-btn chat-rename"
            title="Renombrar la conversación"
            @click="chat.renameChat()"
          >
            <Icon n="pencil" :t="11" />
          </button>
        </span>
        <span class="chat-model mono"
          >{{ info?.provider ? info.provider + ' · ' : '' }}{{ info?.model || '—' }}</span
        >
      </div>
      <button
        class="btn mini"
        type="button"
        :class="{ on: showChats }"
        title="Conversaciones guardadas"
        :aria-expanded="showChats"
        @click="showChats = !showChats"
      >
        <Icon n="list" :t="13" /> {{ chats.length || '' }}
      </button>
      <button
        class="btn mini"
        type="button"
        title="Nueva conversación"
        :disabled="thinking || !messages.length"
        @click="newChat"
      >
        <Icon n="plus" :t="13" />
      </button>
      <button
        v-if="chatId && messages.length"
        class="btn mini"
        type="button"
        title="Copiar la conversación como texto"
        @click="chat.exportChat()"
      >
        <Icon n="copy" :t="13" />
      </button>
      <button
        v-if="messages.length"
        class="btn mini"
        type="button"
        title="Borrar esta conversación"
        @click="chat.clearChat()"
      >
        <Icon n="trash" :t="13" />
      </button>
    </div>

    <!-- las conversaciones guardadas: lista y busqueda en todas -->
    <div v-if="showChats" class="chat-list">
      <TextField
        v-model="chatQuery"
        width="100%"
        icon="search"
        compact
        placeholder="buscar en todas las conversaciones…"
        aria-label="Buscar en todas las conversaciones"
      />
      <div v-if="chatQuery.trim()" class="chat-list-items">
        <button
          v-for="h in hits"
          :key="h.id || h.chat_id + h.snippet"
          type="button"
          class="chat-item"
          @click="openChat(h.chat_id)"
        >
          <span class="chat-item-title">{{ h.title || 'sin título' }}</span>
          <span class="chat-item-sub"
            >{{ h.role === 'me' ? 'tú' : 'asistente' }}: {{ h.snippet }}</span
          >
        </button>
        <div v-if="!hits.length" class="chat-item-empty">nada con «{{ chatQuery }}»</div>
      </div>
      <div v-else class="chat-list-items">
        <!-- abrir y borrar son dos botones hermanos: un boton dentro de otro
             no se puede pulsar con el teclado -->
        <div v-for="c in chats" :key="c.id" class="chat-item-row">
          <button
            type="button"
            class="chat-item"
            :class="{ current: c.id === chatId }"
            :aria-current="c.id === chatId ? 'true' : undefined"
            @click="openChat(c.id)"
          >
            <span class="chat-item-title">{{ c.title || 'sin título' }}</span>
            <span class="chat-item-sub">{{ c.n }} mensajes</span>
          </button>
          <button
            type="button"
            class="chat-item-x field-btn"
            title="Borrar"
            :aria-label="'Borrar «' + (c.title || 'sin título') + '»'"
            @click="chat.deleteChat(c)"
          >
            <Icon n="close" :t="12" />
          </button>
        </div>
        <div v-if="!chats.length" class="chat-item-empty">
          todavía no hay conversaciones guardadas
        </div>
      </div>
    </div>

    <div ref="thread" class="chat-thread" aria-live="polite">
      <div v-if="!messages.length" class="chat-empty">
        <Icon n="ai" :t="34" />
        <p>
          Preguntame por tu musica. Busco en tu biblioteca, armo listas, traigo letras, saco acordes
          y los transpongo, busco en YouTube y en la web, y te bajo lo que me pidas: se convierte a
          mp3 y se archiva por artista.
        </p>
        <div class="chat-suggest">
          <button
            v-for="s in SUGGESTIONS"
            :key="s"
            type="button"
            class="chip"
            @click="chat.send(s)"
          >
            {{ s }}
          </button>
        </div>
        <p class="chat-note">
          Solo hablo de musica: canciones, artistas, generos, instrumentos, teoria e historia. Lo
          que se salga de ahi te lo dire.
        </p>
        <div v-if="info && !info.available" class="chat-noai">
          <p>
            <Icon n="warning" :t="14" /> La IA no está lista: {{ info.reason || 'sin proveedor' }}.
            Elige uno en Ajustes → Inteligencia artificial, o prueba uno gratuito sin clave.
          </p>
          <button type="button" class="btn primary" :disabled="tryingFree" @click="chat.tryFree()">
            {{ tryingFree ? 'Buscando uno que responda…' : 'Probar gratis, sin clave' }}
          </button>
          <p v-if="freeNote" class="chat-note" style="border: none; padding: 0">{{ freeNote }}</p>
        </div>
      </div>

      <div
        v-for="(m, i) in messages"
        v-show="!m.hidden"
        :key="i"
        class="chat-msg"
        :class="[m.role, { error: m.error, narrated: m.narrated }]"
        :title="
          m.narrated
            ? 'El asistente dice haber hecho algo, pero ninguna herramienta lo hizo'
            : undefined
        "
      >
        <div v-if="m.tools?.length" class="chat-tools">
          <span v-for="(h, j) in m.tools" :key="j" class="chip">
            <Icon n="check" :t="11" /> {{ toolLabel(h.name) }} · {{ h.summary }}
          </span>
        </div>
        <!-- Lo del asistente viene en markdown y se pinta como tal. Lo tuyo y
             los errores van tal cual: son texto plano. `renderMarkdown`
             escapa todo antes de marcar nada (ver utils/markdown.js). -->
        <!-- eslint-disable vue/no-v-html -->
        <div
          v-if="m.role === 'ai' && !m.error"
          class="chat-bubble chat-md"
          v-html="md(m.text)"
        ></div>
        <!-- eslint-enable vue/no-v-html -->
        <div v-else class="chat-bubble">{{ m.text }}</div>
        <div
          v-if="m.role === 'ai' && costLine(m)"
          class="chat-cost mono"
          :title="'tokens de esta respuesta (entrada + salida) y su coste según el catálogo'"
        >
          {{ costLine(m) }}
        </div>
      </div>

      <!-- la respuesta que esta llegando: herramientas segun terminan y el texto segun sale -->
      <div v-if="thinking && live" class="chat-msg ai chat-live">
        <div v-if="live.tools.length" class="chat-tools">
          <span v-for="(h, j) in live.tools" :key="j" class="chip">
            <Icon n="check" :t="11" /> {{ toolLabel(h.name) }} · {{ h.summary }}
          </span>
        </div>
        <!-- lo mismo: markdown ya escapado -->
        <!-- eslint-disable vue/no-v-html -->
        <div v-if="live.text" class="chat-bubble chat-md" v-html="renderMarkdown(live.text)"></div>
        <!-- eslint-enable vue/no-v-html -->
      </div>

      <div v-if="downloading" class="chat-msg ai">
        <div class="chat-bubble chat-downloading">
          <span class="spinner"></span>
          <span class="chat-downloading-txt">
            {{ PHASES[downloading.phase] || 'Bajando' }}
            <span v-if="downloading.total > 1">
              · {{ downloading.index || 1 }}/{{ downloading.total }}</span
            >
            <span v-if="downloading.name"> · {{ downloading.name }}</span>
          </span>
          <span class="mono sub">{{ (downloading.percent || 0).toFixed(0) }}%</span>
        </div>
      </div>

      <div
        v-if="thinking && !live?.text"
        class="chat-msg ai"
        aria-label="El asistente está pensando"
      >
        <div class="chat-bubble chat-thinking"><span></span><span></span><span></span></div>
      </div>
    </div>

    <div class="chat-foot">
      <TextField
        v-model="draft"
        width="100%"
        aria-label="Mensaje para el asistente"
        placeholder="Escribe lo que necesites…"
        @enter="chat.send()"
      />
      <!-- `next` es el icono de «cancion siguiente» del reproductor; aqui
           lo que se hace es enviar, no saltar de pista. -->
      <button
        class="btn primary"
        type="button"
        title="Enviar"
        :disabled="thinking || !draft.trim()"
        @click="chat.send()"
      >
        <Icon n="send" :t="15" />
      </button>
      <button
        v-if="thinking"
        class="btn chat-cancel"
        type="button"
        title="Parar la respuesta"
        @click="chat.cancel()"
      >
        <Icon n="close" :t="14" /> Parar
      </button>
    </div>
  </div>
</template>
