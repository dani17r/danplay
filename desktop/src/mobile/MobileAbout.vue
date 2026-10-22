<script setup>
/**
 * «Acerca de DanPlay» en el celular: quién lo hizo (Daniel Rangel) y cómo
 * escribirle para colaborar, las novedades de cada versión y los términos.
 * Lo mismo que la página de la PC (AboutPage), a la medida del dedo.
 */
import CopyButton from '../components/ui/CopyButton.vue'
import { notifyCopied } from '../composables/useNotices.js'
import { AUTHOR, TERMS, TERMS_VERSION } from '../data/terms.js'
import { RELEASES, compareVersions } from '../data/releases.js'
import { version } from '../../package.json'
import appIcon from './icon.png'

const isMine = (r) => compareVersions(r.version, version) === 0
</script>

<template>
  <div class="m-about">
    <header class="m-about-head">
      <img :src="appIcon" alt="" width="56" height="56" />
      <div>
        <strong>DanPlay</strong>
        <span>versión {{ version }}</span>
      </div>
    </header>

    <section class="m-about-credit">
      <h3>DanPlay es obra de {{ AUTHOR.name }}</h3>
      <p>
        {{ AUTHOR.name }} creó y desarrolla DanPlay: un reproductor hecho por un músico para
        músicos, para ensayar, estudiar y tocar en directo.
      </p>
      <p>¿Quieres colaborar? Escríbele por Telegram:</p>
      <div class="m-about-contact">
        <a :href="AUTHOR.url" target="_blank" rel="noopener">{{ AUTHOR.telegram }}</a>
        <CopyButton
          :text="AUTHOR.url"
          what="el enlace"
          label="copiar"
          :size="14"
          @copied="(ok) => notifyCopied(ok, AUTHOR.url)"
        />
      </div>
      <p class="m-about-small">{{ AUTHOR.url }}</p>
    </section>

    <h3 class="m-about-title">Novedades</h3>
    <article v-for="r in RELEASES" :key="r.version" class="m-release" :class="{ mine: isMine(r) }">
      <header>
        <b>{{ r.version }}</b>
        <span v-if="isMine(r)">tu versión</span>
        <span v-else-if="!r.date">en preparación</span>
        <span v-else>{{ r.date }}</span>
      </header>
      <p>{{ r.summary }}</p>
      <ul>
        <li v-for="(it, i) in r.items" :key="i">{{ it }}</li>
      </ul>
    </article>

    <details class="m-about-terms">
      <summary>Términos y condiciones ({{ TERMS_VERSION }})</summary>
      <section v-for="sec in TERMS" :key="sec.title">
        <h4>{{ sec.title }}</h4>
        <p v-for="(p, i) in sec.body" :key="i">{{ p }}</p>
      </section>
    </details>

    <p class="m-about-small">© 2026 {{ AUTHOR.name }} · PolyForm Strict License 1.0.0</p>
  </div>
</template>
