<script setup>
/**
 * Los ajustes del celular, con botones grandes en vez de desplegables: el
 * tema, el tamaño de la letra, cómo se pone una canción y la pantalla
 * encendida.
 *
 * Sin «Fundido entre canciones»: estaba aquí, de 0 a 8 segundos, pero en el
 * celular no se funden canciones (`set_crossfade` no hace nada) y elegir uno
 * no cambiaba nada.
 */
import { computed } from 'vue'
import { usePreferences } from '../composables/usePreferences.js'
import { SIZES, allThemes } from '../themes.js'
import { tapPlays, compactList, keepAwake } from './prefs.js'
import ToggleField from '../components/ui/ToggleField.vue'
import { version } from '../../package.json'

defineProps({ total: { type: Number, default: 0 } })

const { theme, appSize } = usePreferences()
const themes = computed(() => Object.entries(allThemes()))
</script>

<template>
  <div class="m-settings">
    <section>
      <h3>Tamaño de la letra</h3>
      <div class="m-chips m-sizes" role="group" aria-label="Tamaño de la letra">
        <button
          v-for="(s, k) in SIZES"
          :key="k"
          type="button"
          class="m-chip-btn"
          :class="{ on: appSize === k }"
          :aria-pressed="appSize === k"
          @click="appSize = k"
        >
          {{ s.name }}
        </button>
      </div>
    </section>

    <section>
      <h3>Tema</h3>
      <div class="m-themes" role="group" aria-label="Tema">
        <button
          v-for="[k, t] in themes"
          :key="k"
          type="button"
          class="m-theme"
          :class="{ on: theme === k }"
          :aria-pressed="theme === k"
          :style="{ '--sw-bg': t.v.bg, '--sw-panel': t.v.panel, '--sw-accent': t.v.accent }"
          @click="theme = k"
        >
          <span class="m-theme-sw" aria-hidden="true"><i></i></span>
          <span class="m-theme-name">{{ t.name }}</span>
        </button>
      </div>
    </section>

    <section>
      <h3>La lista</h3>
      <ToggleField
        v-model="tapPlays"
        title="Un toque pone la canción"
        hint="Apagado: un toque la elige y enseña sus botones; con doble toque suena"
      />
      <ToggleField
        v-model="compactList"
        title="Filas más bajas"
        hint="Caben más canciones en la pantalla"
      />
    </section>

    <section>
      <h3>Reproducción</h3>
      <ToggleField
        v-model="keepAwake"
        title="Pantalla encendida mientras suena"
        hint="Para tocar con el teléfono en el atril"
      />
    </section>

    <section class="m-about">
      <h3>DanPlay</h3>
      <p>Versión {{ version }} · {{ total }} {{ total === 1 ? 'pista' : 'pistas' }}</p>
      <p>
        Las pistas se separan en DanPlay de la PC y se traen aquí. Lo que cambies en el estudio (la
        mezcla, los tramos, el clic) se queda en este teléfono.
      </p>
    </section>
  </div>
</template>
