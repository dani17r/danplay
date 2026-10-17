<script setup>
/**
 * Las salidas: por dónde suena cada cosa. Para usar DanPlay como una
 * secuencia en directo: el clic y las guías a los cascos y la música a la
 * mesa, por los dos lados de una salida normal (izquierda y derecha, en
 * mono) o por las salidas de una interfaz de varios canales.
 *
 * Arriba la salida de audio (la del sistema o una concreta, que se recuerda
 * aunque se desenchufe: si no está, suena por la del sistema y se avisa);
 * debajo, la manera de repartirlo. Cada cosa por su sitio se elige en la
 * onda, en el carril de cada una (sus botones L y R, estéreo o mono); en el
 * celular, que no tiene esas cabeceras, aquí debajo, cada cosa en su tarjeta.
 */
import { computed, onMounted, ref } from 'vue'
import { isMobileApp } from '../platform.js'
import {
  useRouting,
  destinations,
  destOf,
  monoOf,
  isSingle,
  PRESETS
} from '../composables/useRouting.js'
import Icon from './Icon.vue'
import Drawer from './ui/Drawer.vue'
import SelectField from './ui/SelectField.vue'

const props = defineProps({
  open: Boolean,
  /** las pistas de la canción que suena: [{key, name, role}] */
  stems: { type: Array, default: () => [] }
})
const emit = defineEmits(['close'])

const r = useRouting()
const { prefs, devices, output } = r
const loading = ref(false)

async function refresh() {
  loading.value = true
  try {
    await r.loadDevices()
  } finally {
    loading.value = false
  }
}
onMounted(refresh)

const deviceOptions = computed(() => [
  { v: '', n: 'La del sistema' },
  ...devices.value.map((d) => ({
    v: d.id,
    n: d.name + (d.default ? ' (la del sistema)' : ''),
    // la que usa el sistema ahora no se puede abrir aparte: suena por la del sistema
    note: d.busy
      ? 'ocupada: la usa el sistema'
      : `${d.channels} ${d.channels === 1 ? 'canal' : 'canales'}`
  }))
])
/** Los canales de la salida elegida (o de la abierta). */
const channels = computed(() => {
  const chosen = devices.value.find((d) => d.id === prefs.value.device)
  return chosen?.channels || output.value?.channels || 2
})
const dests = computed(() => destinations(channels.value))
const rows = computed(() => [
  { kind: 'song', name: 'La canción', key: '' },
  ...props.stems.map((s) => ({
    kind: 'stem',
    // un carril de guías ya se llama «Guías: …»; una pista de una secuencia
    // dice su papel
    name:
      s.name + (s.lane ? '' : s.role === 'click' ? ' (clic)' : s.role === 'guide' ? ' (guía)' : ''),
    key: s.key,
    role: s.role || ''
  })),
  { kind: 'click', name: 'El clic', key: '' },
  { kind: 'cues', name: 'Las guías', key: '' }
])
/** Lo que va ahora por cada fila (también con un preset puesto). */
function destFor(row) {
  return destOf(prefs.value, row.kind, row.key, row.role)
}
/** Si va por un solo canal (o no suena): entonces el modo no se elige. */
const fixedMode = (row) => {
  const d = destFor(row)
  return d === 'off' || isSingle(d)
}
/** Si suena en mono: por un solo canal, o un par con lo mismo por los dos. */
const monoFor = (row) => isSingle(destFor(row)) || monoOf(prefs.value, row.kind, row.key, row.role)

/** Pasar a «a medida» desde un preset deja cada cosa donde iba (`useRouting`). */
function choose(row, dest) {
  return r.setRoute(row.kind, row.key, dest)
}
const needs = computed(() => PRESETS.find((p) => p.v === prefs.value.preset)?.channels || 2)
const short = computed(() => channels.value < Math.max(needs.value, r.routing.value.channels))
</script>

<template>
  <Drawer :open="open" side="right" title="Salidas" width="min(440px, 94vw)" @close="emit('close')">
    <div class="outs">
      <p class="outs-lead">
        Como una secuencia en directo: el clic y las guías a tus cascos, la música a la mesa.
      </p>

      <section class="outs-block">
        <h4 class="outs-title"><Icon n="volume" :t="13" /> Salida de audio</h4>
        <div class="outs-device">
          <SelectField
            :model-value="prefs.device || ''"
            :options="deviceOptions"
            label="Salida de audio"
            width="100%"
            @update:model-value="(v) => r.update({ device: v || null })"
          />
          <button
            type="button"
            class="btn mini"
            :disabled="loading"
            title="Volver a buscar las salidas (si acabas de enchufar una interfaz)"
            @click="refresh"
          >
            <Icon n="refresh" :t="12" />
          </button>
        </div>
        <p v-if="output" class="outs-now" :class="{ warn: output.fallback || output.error }">
          <template v-if="output.error">La salida falla: {{ output.error }}</template>
          <template v-else-if="output.fallback">
            La elegida no está o la usa otro programa: suena por {{ output.name }}
          </template>
          <template v-else>
            Suena por {{ output.name }} · {{ output.channels }}
            {{ output.channels === 1 ? 'canal' : 'canales' }} ·
            {{ Math.round(output.rate / 100) / 10 }}
            kHz
          </template>
          <template v-if="output.xruns">
            · {{ output.xruns }} {{ output.xruns === 1 ? 'tropiezo' : 'tropiezos' }}</template
          >
        </p>
        <p v-else class="outs-now">Se abre al darle a play.</p>
      </section>

      <section class="outs-block">
        <h4 class="outs-title"><Icon n="mixer" :t="13" /> Cómo repartirlo</h4>
        <div class="outs-presets" role="radiogroup" aria-label="Cómo repartir el sonido">
          <button
            v-for="p in PRESETS"
            :key="p.v"
            type="button"
            role="radio"
            class="outs-preset"
            :class="{ on: prefs.preset === p.v }"
            :aria-checked="prefs.preset === p.v"
            @click="r.update({ preset: p.v })"
          >
            <span class="outs-preset-name">{{ p.n }}</span>
            <span class="outs-preset-note">{{ p.note }}</span>
          </button>
        </div>
        <p v-if="short" class="outs-now warn">
          Esta salida tiene {{ channels }} canales: lo que vaya por más arriba no se oirá. Elige una
          interfaz con más salidas o «Clic y guías a la izquierda».
        </p>
      </section>

      <p v-if="!isMobileApp" class="outs-hint outs-where">
        <Icon n="outputs" :t="13" /> Cada cosa por su lado: en la onda, con los botones <b>L</b> y
        <b>R</b> de su carril (y <b>Est</b> o <b>Mono</b>).
      </p>
      <section v-if="isMobileApp" class="outs-block">
        <h4 class="outs-title"><Icon n="list" :t="13" /> Cada cosa</h4>
        <div v-for="row in rows" :key="row.kind + row.key" class="outs-row">
          <span class="outs-row-name">{{ row.name }}</span>
          <div class="outs-row-pick">
            <SelectField
              :model-value="destFor(row)"
              :options="dests"
              :label="'Por dónde suena ' + row.name"
              width="170px"
              @update:model-value="(v) => choose(row, v)"
            />
            <div
              class="outs-mode"
              role="group"
              :aria-label="'Estéreo o mono: ' + row.name"
              :title="fixedMode(row) ? 'Por un solo canal: va en mono' : ''"
            >
              <button
                type="button"
                class="outs-mode-btn"
                :class="{ on: !monoFor(row) }"
                :aria-pressed="!monoFor(row)"
                :disabled="fixedMode(row)"
                @click="r.setMono(row.kind, false, row.key)"
              >
                Estéreo
              </button>
              <button
                type="button"
                class="outs-mode-btn"
                :class="{ on: monoFor(row) }"
                :aria-pressed="monoFor(row)"
                :disabled="fixedMode(row)"
                @click="r.setMono(row.kind, true, row.key)"
              >
                Mono
              </button>
            </div>
          </div>
        </div>
        <p class="outs-hint">
          «Mono» junta los dos lados de lo que suena y lo manda igual por los dos canales (para una
          mesa o una sala en mono). Por un solo canal («solo izquierda», «salida 3») siempre va en
          mono. Con las pistas separadas sonando, cada una puede ir por su sitio.
        </p>
      </section>
    </div>
  </Drawer>
</template>
