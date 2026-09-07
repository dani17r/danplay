<script setup>
/**
 * Tarjeta de ajustes o de pagina: titulo, nota y contenido.
 * Se usaba veinte veces con el mismo marcado escrito a mano.
 */
defineProps({ title: String, note: String, tone: String })
</script>

<template>
  <section class="card" :class="tone ? 'card-' + tone : null">
    <!-- el hueco trae su propio <h3>: hay titulos con un icono, una cuenta o
         su propia colocacion, y envolverlos aqui los anidaria dos veces -->
    <slot name="title"><h3 v-if="title">{{ title }}</h3></slot>
    <div v-if="note || $slots.note" class="note">
      <slot name="note">{{ note }}</slot>
    </div>
    <slot />
  </section>
</template>
