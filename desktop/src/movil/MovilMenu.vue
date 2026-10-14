<script setup>
/**
 * El menú de lado del celular (se abre con ☰ arriba a la izquierda):
 * qué se ve en la lista —todas, las favoritas o una lista— y lo demás de la
 * app: traer pistas, los ajustes y la ayuda.
 */
import Icon from '../components/Icon.vue'
import { version } from '../../package.json'
import icono from './icono.png'

defineProps({
  /** lo que se ve: { kind: 'all' | 'favorites' | 'list', id? } */
  view: { type: Object, required: true },
  lists: { type: Array, default: () => [] },
  total: { type: Number, default: 0 },
  favorites: { type: Number, default: 0 }
})
const emit = defineEmits(['go', 'newList', 'import', 'settings', 'help'])
</script>

<template>
  <nav class="m-nav" aria-label="Menú">
    <div class="m-nav-brand">
      <img :src="icono" alt="" width="40" height="40" />
      <div>
        <strong>DanPlay</strong>
        <span>versión {{ version }}</span>
      </div>
    </div>

    <button
      type="button"
      class="m-nav-item"
      :class="{ on: view.kind === 'all' }"
      :aria-current="view.kind === 'all' ? 'page' : undefined"
      @click="emit('go', { kind: 'all' })"
    >
      <Icon n="music" :t="20" /><span>Mis pistas</span><em>{{ total }}</em>
    </button>
    <button
      type="button"
      class="m-nav-item"
      :class="{ on: view.kind === 'favorites' }"
      :aria-current="view.kind === 'favorites' ? 'page' : undefined"
      @click="emit('go', { kind: 'favorites' })"
    >
      <Icon n="heartFull" :t="20" /><span>Favoritas</span><em>{{ favorites }}</em>
    </button>

    <div class="m-nav-title">
      <span>Listas</span>
      <button type="button" class="m-nav-add" @click="emit('newList')">
        <Icon n="plus" :t="16" /> Nueva
      </button>
    </div>
    <button
      v-for="l in lists"
      :key="l.id"
      type="button"
      class="m-nav-item"
      :class="{ on: view.kind === 'list' && view.id === l.id }"
      :aria-current="view.kind === 'list' && view.id === l.id ? 'page' : undefined"
      @click="emit('go', { kind: 'list', id: l.id, name: l.name })"
    >
      <Icon n="list" :t="20" /><span>{{ l.name }}</span
      ><em>{{ l.n }}</em>
    </button>
    <p v-if="!lists.length" class="m-nav-empty">
      Arma la lista del domingo o la del ensayo y tócalas en orden. «Nueva» para empezar.
    </p>

    <div class="m-nav-sep"></div>
    <button type="button" class="m-nav-item" @click="emit('import')">
      <Icon n="download" :t="20" /><span>Importar pistas</span>
    </button>
    <button type="button" class="m-nav-item" @click="emit('settings')">
      <Icon n="settings" :t="20" /><span>Ajustes</span>
    </button>
    <button type="button" class="m-nav-item" @click="emit('help')">
      <Icon n="info" :t="20" /><span>Cómo traer pistas de la PC</span>
    </button>
  </nav>
</template>
