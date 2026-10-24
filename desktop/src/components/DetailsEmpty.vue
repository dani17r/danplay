<script setup>
/**
 * La ficha sin canción elegida. Aparte del panel para que la app la pinte
 * mientras llega el trozo de DetailsPanel (es una isla, ver islands.js): es
 * lo mismo que pinta él sin canción, y el cambio de uno a otro no se ve.
 */
import EmptyState from './ui/EmptyState.vue'
</script>

<template>
  <aside class="details">
    <EmptyState
      icon="note"
      title="Selecciona una canción"
      hint="Aquí verás su ficha, la letra y los acordes"
      style="padding-top: 70px"
    />
  </aside>
</template>
