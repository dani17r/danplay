<script setup>
/**
 * Exportar, en una ventana: qué (la canción, la mezcla de las pistas o la
 * pista para el celular) y con qué (las guías, el clic, la velocidad y el
 * tono, los canales). Antes eran varias entradas largas en un menú; aquí
 * caben con lo que hace cada una. Lo marcado se recuerda para la próxima.
 */
import { computed, ref, watch } from 'vue'
import Icon from './Icon.vue'
import WideDialog from './ui/WideDialog.vue'

const props = defineProps({
  open: Boolean,
  /** con qué se abre elegido: 'song' o 'mix' */
  initial: { type: String, default: 'song' },
  /** la canción tiene sus pistas separadas */
  hasStems: Boolean,
  /** cuántas guías suenan (0: no se ofrece) */
  guides: { type: Number, default: 0 },
  /** el clic está puesto */
  clickOn: Boolean,
  /** la velocidad y el tono, si no son los de la canción («80 % · medio tono arriba») */
  heard: { type: String, default: '' },
  /** algo no suena por los dos lados en estéreo */
  routed: Boolean
})
const emit = defineEmits(['close', 'export', 'phone'])

const KEY = 'danplay.export'
/** Lo marcado la última vez (se recuerda en el navegador). */
function readSaved() {
  try {
    return JSON.parse(localStorage.getItem(KEY) || '{}') || {}
  } catch {
    return {}
  }
}
const saved = readSaved()
const what = ref('song')
const opts = ref({
  guides: saved.guides !== false,
  click: saved.click === true,
  heard: saved.heard === true,
  channels: saved.channels === true
})
watch(
  () => props.open,
  (v) => {
    if (v) what.value = props.initial === 'mix' && props.hasStems ? 'mix' : 'song'
  },
  { immediate: true }
)
watch(
  opts,
  (v) => {
    try {
      localStorage.setItem(KEY, JSON.stringify(v))
    } catch {
      /* modo privado */
    }
  },
  { deep: true }
)

const KINDS = computed(() => [
  {
    v: 'song',
    icon: 'music',
    n: 'La canción',
    text: 'El archivo de la canción, con lo que marques debajo.'
  },
  {
    v: 'mix',
    icon: 'mixer',
    n: 'La mezcla de las pistas',
    text: 'Las pistas como suenan ahora, cada una con su volumen; las calladas, fuera. Para practicar sin la batería, por ejemplo.',
    off: !props.hasStems && 'Separa antes la canción en pistas'
  },
  {
    v: 'phone',
    icon: 'send',
    n: 'Para el celular',
    text: 'Un .danpista con las pistas, el clic, los tramos, las guías, la letra y los acordes, para la app del celular.',
    off: !props.hasStems && 'Hace falta separarla en pistas'
  }
])
const EXTRAS = computed(() =>
  [
    props.guides > 0 && {
      k: 'guides',
      icon: 'waveform',
      n: 'Las guías',
      text: `${props.guides === 1 ? 'La guía' : 'Las ' + props.guides + ' guías'} en su sitio, con su volumen.`
    },
    props.clickOn && {
      k: 'click',
      icon: 'note',
      n: 'El clic',
      text: 'Los golpes del metrónomo, con su sonido y su volumen.'
    },
    props.heard && {
      k: 'heard',
      icon: 'refresh',
      n: 'La velocidad y el tono',
      text: `Como la estás oyendo: ${props.heard}.`
    },
    props.routed && {
      k: 'channels',
      icon: 'outputs',
      n: 'Los canales',
      text: 'Cada cosa por su lado (L o R), en estéreo o en mono, como suena: para usarla como una secuencia.'
    }
  ].filter(Boolean)
)
function pick(k) {
  const kind = KINDS.value.find((x) => x.v === k)
  if (kind && !kind.off) what.value = k
}
function go() {
  if (what.value === 'phone') return emit('phone')
  const o = opts.value
  const has = (k) => EXTRAS.value.some((e) => e.k === k)
  emit('export', {
    from: what.value === 'mix' ? 'stems' : 'song',
    guides: has('guides') && o.guides,
    click: has('click') && o.click,
    heard: has('heard') && o.heard,
    channels: has('channels') && o.channels
  })
}
</script>

<template>
  <WideDialog :open="open" title="Exportar" icon="download" width="620px" @close="emit('close')">
    <div class="exp-kinds" role="radiogroup" aria-label="Qué exportar">
      <button
        v-for="k in KINDS"
        :key="k.v"
        type="button"
        role="radio"
        class="exp-kind"
        :class="{ on: what === k.v, off: k.off }"
        :aria-checked="what === k.v"
        :disabled="!!k.off"
        :title="k.off || ''"
        @click="pick(k.v)"
      >
        <span class="exp-kind-icon"><Icon :n="k.icon" :t="20" /></span>
        <span class="exp-kind-name">{{ k.n }}</span>
        <span class="exp-kind-text">{{ k.off || k.text }}</span>
      </button>
    </div>
    <Transition name="fade" mode="out-in">
      <div v-if="what !== 'phone'" key="extras" class="exp-extras">
        <h4 class="exp-title">Con</h4>
        <p v-if="!EXTRAS.length" class="exp-none">
          Tal cual: no hay guías, clic ni cambios que añadir.
        </p>
        <button
          v-for="e in EXTRAS"
          :key="e.k"
          type="button"
          role="checkbox"
          class="exp-extra"
          :class="{ on: opts[e.k] }"
          :aria-checked="opts[e.k]"
          @click="opts[e.k] = !opts[e.k]"
        >
          <span class="exp-check"><Icon v-if="opts[e.k]" n="check" :t="13" /></span>
          <span class="exp-extra-icon"><Icon :n="e.icon" :t="15" /></span>
          <span class="exp-extra-body">
            <span class="exp-extra-name">{{ e.n }}</span>
            <span class="exp-extra-text">{{ e.text }}</span>
          </span>
        </button>
      </div>
      <p v-else key="phone" class="exp-none">
        Se guarda en la carpeta <b>Celular</b> de tu biblioteca: mándalo al teléfono por WhatsApp,
        Drive o un cable y ábrelo con DanPlay.
      </p>
    </Transition>
    <template #foot>
      <button type="button" class="btn" @click="emit('close')">Cancelar</button>
      <button type="button" class="btn primary exp-go" @click="go">
        <Icon :n="what === 'phone' ? 'send' : 'download'" :t="13" />
        {{ what === 'phone' ? 'Preparar para el celular' : 'Elegir dónde guardarla…' }}
      </button>
    </template>
  </WideDialog>
</template>
