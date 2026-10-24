<script setup>
/**
 * «Acerca de DanPlay»: quién lo hizo, las novedades de cada versión y los
 * términos y condiciones.
 *
 * DanPlay es obra de Daniel Rangel, y esta página lo dice primero: con su
 * contacto para quien quiera colaborar. Las novedades salen de
 * data/releases.js (cortas, para quien usa la app) y los términos de
 * data/terms.js, los mismos que la ventana de aceptarlos.
 */
import { computed } from 'vue'
import Icon from './Icon.vue'
import { app as tauriApp } from '../api.js'
import { useTerms } from '../composables/useTerms.js'
import { notify } from '../composables/useNotices.js'
import { AUTHOR, TERMS, TERMS_VERSION } from '../data/terms.js'
import { RELEASES, compareVersions } from '../data/releases.js'
import { version } from '../../package.json'
import logo from '../mobile/icon.png'

const props = defineProps({
  /** la pestaña: 'about' | 'news' | 'terms' */
  tab: { type: String, default: 'about' }
})
const emit = defineEmits(['tab'])

const TABS = [
  { v: 'about', label: 'Acerca de' },
  { v: 'news', label: 'Novedades' },
  { v: 'terms', label: 'Términos y condiciones' }
]
const current = computed(() => (TABS.some((t) => t.v === props.tab) ? props.tab : 'about'))

const terms = useTerms()
const acceptedOn = computed(() =>
  terms.state.at ? new Date(terms.state.at * 1000).toLocaleDateString('es') : ''
)

// lo que hace funcionar a DanPlay por dentro, con el nombre de quien lo hizo
const THANKS = [
  ['Demucs', 'Meta · separar en pistas'],
  ['Mega 53 Stems', 'MVSep y ZFTurbo · separar en máxima calidad'],
  ['Beat This!', 'CPJKU, Universidad Johannes Kepler · el pulso y el clic'],
  ['Supertonic', 'Supertone · las voces de las guías'],
  ['Piper', 'Rhasspy · las voces de España, México y Argentina'],
  ['yt-dlp', 'las descargas'],
  ['FFmpeg', 'convertir y leer audio'],
  ['Rust, Tauri y Vue', 'la aplicación']
]

/** @param {string} d AAAA-MM-DD */
function dateOf(d) {
  if (!d) return 'en preparación'
  const [y, m, day] = d.split('-').map(Number)
  return new Date(y, m - 1, day).toLocaleDateString('es', {
    day: 'numeric',
    month: 'long',
    year: 'numeric'
  })
}
const isMine = (r) => compareVersions(r.version, version) === 0

async function openTelegram() {
  try {
    await tauriApp.openInBrowser(AUTHOR.url)
  } catch {
    // fuera de la app de escritorio (o sin navegador): el enlace normal
    if (!window.open(AUTHOR.url, '_blank', 'noopener')) {
      notify(`Escríbele por Telegram a ${AUTHOR.telegram}: ${AUTHOR.url}`, 'info', 10)
    }
  }
}
</script>

<template>
  <div class="page about-page">
    <header class="about-head">
      <img :src="logo" alt="" width="56" height="56" />
      <div>
        <h2>DanPlay</h2>
        <p class="about-version">Versión {{ version }}</p>
        <p class="about-author">
          Creado y desarrollado por <strong>{{ AUTHOR.name }}</strong>
        </p>
      </div>
    </header>

    <div class="about-tabs" role="tablist" aria-label="Acerca de DanPlay">
      <button
        v-for="t in TABS"
        :key="t.v"
        type="button"
        role="tab"
        class="about-tab"
        :class="{ on: current === t.v }"
        :aria-selected="current === t.v"
        @click="emit('tab', t.v)"
      >
        {{ t.label }}
      </button>
    </div>

    <!-- acerca de: quién lo hizo -->
    <section v-if="current === 'about'" class="about-panel" role="tabpanel">
      <div class="about-credit">
        <Icon n="music" :t="22" />
        <div>
          <h3>DanPlay es obra de {{ AUTHOR.name }}</h3>
          <p>
            {{ AUTHOR.name }} creó y desarrolla DanPlay: un reproductor hecho por un músico para
            músicos, pensado para ensayar, estudiar y tocar en directo. El diseño, el código y cada
            función de la aplicación son obra suya.
          </p>
        </div>
      </div>

      <div class="about-card">
        <h3>¿Quieres colaborar?</h3>
        <p>
          Ideas, errores, traducciones, canciones para probar o ganas de ayudar a mejorar DanPlay:
          escríbele a {{ AUTHOR.name }} por Telegram. Toda colaboración se hace con él y con el
          crédito a su autor.
        </p>
        <button type="button" class="btn primary" @click="openTelegram">
          <Icon n="send" :t="14" /> Escribir a {{ AUTHOR.telegram }} en Telegram
        </button>
        <p class="about-link">{{ AUTHOR.url }}</p>
      </div>

      <div class="about-card">
        <h3>Gracias a</h3>
        <ul class="about-thanks">
          <li v-for="[name, what] in THANKS" :key="name">
            <strong>{{ name }}</strong> <span>{{ what }}</span>
          </li>
        </ul>
        <p class="about-small">
          Cada pieza conserva su propia licencia. DanPlay: © 2026 {{ AUTHOR.name }}, bajo la
          PolyForm Strict License 1.0.0 (uso personal y sin fines comerciales).
        </p>
      </div>
    </section>

    <!-- novedades: lo de cada versión, corto -->
    <section v-else-if="current === 'news'" class="about-panel" role="tabpanel">
      <article v-for="r in RELEASES" :key="r.version" class="release" :class="{ mine: isMine(r) }">
        <header>
          <span class="release-version">{{ r.version }}</span>
          <span class="release-date">{{ dateOf(r.date) }}</span>
          <span v-if="isMine(r)" class="release-mine">Tu versión</span>
        </header>
        <p class="release-summary">{{ r.summary }}</p>
        <ul>
          <li v-for="(it, i) in r.items" :key="i">{{ it }}</li>
        </ul>
      </article>
    </section>

    <!-- los términos -->
    <section v-else class="about-panel" role="tabpanel">
      <div class="about-card terms-state" :class="{ ok: terms.accepted.value }">
        <template v-if="terms.accepted.value">
          <Icon n="checkCircle" :t="18" />
          <span>Los aceptaste{{ acceptedOn ? ' el ' + acceptedOn : '' }}.</span>
        </template>
        <template v-else>
          <Icon n="lockClosed" :t="18" />
          <span
            >Aún no los aceptaste: sin ellos no se puede separar en pistas, usar el asistente ni
            «Transformar».</span
          >
          <button type="button" class="btn primary" @click="terms.show()">Leer y aceptar</button>
        </template>
      </div>
      <p class="about-small">Versión de los términos: {{ TERMS_VERSION }}</p>
      <section v-for="sec in TERMS" :key="sec.title" class="terms-section">
        <h4>{{ sec.title }}</h4>
        <p v-for="(p, i) in sec.body" :key="i">{{ p }}</p>
      </section>
    </section>
  </div>
</template>
