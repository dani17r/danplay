<script setup>
/**
 * El reloj del modo estudio: por dónde va la canción y lo que dura. Aparte
 * para que la posición (cuatro avisos por segundo) solo repinte esto: leída
 * en StudyBar, cada aviso volvía a ejecutar su plantilla entera.
 */
import { usePlayback } from '../composables/usePlayback.js'
import { formatTime } from '../utils/format.js'

const { position, duration } = usePlayback()
</script>

<template>
  <span class="study-time mono" title="Dónde va la canción / lo que dura"
    >{{ formatTime(position)
    }}<span class="study-time-total"> / {{ formatTime(duration) }}</span></span
  >
</template>
