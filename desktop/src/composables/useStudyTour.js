// @ts-check
// El recorrido del modo estudio (driver.js): cada cosa resaltada con una
// frase corta y, donde ayuda, una animación pequeña que se mueve sola (un
// cursor que arrastra sobre la onda, un doble clic que pone una guía, los
// botones L y R mandando el sonido a cada lado). Para quien no sabe por
// dónde empezar: se entiende mirando, sin leer.
//
// Sale solo la primera vez que se abre el estudio en el escritorio (se
// recuerda) y siempre con el botón «?» de la barra de la onda. Los pasos
// cuyo sitio no está a la vista (sin guías, sin pistas…) se saltan.
import { driver } from 'driver.js'
import 'driver.js/dist/driver.css'

export const TOUR_KEY = 'danplay.study.recorridoVisto'

/** Una onda de mentira (barras), para las animaciones. */
function wave(n = 28) {
  let out = ''
  for (let i = 0; i < n; i++) {
    const h = 18 + Math.round(Math.abs(Math.sin(i * 1.7) * 22 + Math.sin(i * 0.6) * 12))
    out += `<i style="height:${h}px"></i>`
  }
  return out
}

/** Las animaciones de cada paso: HTML con sus clases (src/styles/tour.css). */
const DEMOS = {
  play: `<div class="tour-demo tour-play" aria-hidden="true">
      <span class="tour-btn"><b class="tour-ico-play"></b><b class="tour-ico-pause"></b></span>
      <span class="tour-mini-wave">${wave(18)}<em class="tour-head"></em></span>
    </div>`,
  drag: `<div class="tour-demo tour-drag" aria-hidden="true">
      <span class="tour-wave">${wave()}<em class="tour-sel"></em><em class="tour-cursor"></em></span>
    </div>`,
  lock: `<div class="tour-demo tour-lock" aria-hidden="true">
      <span class="tour-wave">${wave()}<em class="tour-head moving"></em><em class="tour-tap"></em></span>
      <span class="tour-chip"><svg viewBox="0 0 24 24"><path d="M8 11V8a4 4 0 0 1 8 0v3M6 11h12v9H6z"/></svg></span>
    </div>`,
  guide: `<div class="tour-demo tour-guide" aria-hidden="true">
      <span class="tour-lane"><em class="tour-block">Coro</em><em class="tour-cursor dbl"></em></span>
    </div>`,
  channels: `<div class="tour-demo tour-chans" aria-hidden="true">
      <span class="tour-row"><b>Canción</b><span class="tour-lr"><i>L</i><i class="on">R</i></span><span class="tour-out right"><i></i><i></i><i></i></span></span>
      <span class="tour-row"><b>Guías</b><span class="tour-lr"><i class="on">L</i><i>R</i></span><span class="tour-out left"><i></i><i></i><i></i></span></span>
    </div>`,
  magnet: `<div class="tour-demo tour-magnet" aria-hidden="true">
      <span class="tour-lane ticks">${'<i></i>'.repeat(9)}<em class="tour-block snap">Coro</em></span>
    </div>`
}

/**
 * Los pasos, con lo que hay a la vista: el que no tiene dónde ponerse, fuera.
 * @param {HTMLElement} root  el modo estudio
 */
function stepsFor(root) {
  const at = (/** @type {string} */ sel) => {
    const el = root.querySelector(sel)
    return el && /** @type {HTMLElement} */ (el).offsetParent !== null ? el : null
  }
  /** @type {Array<{el?: string, title: string, text: string, demo?: string, side?: any}>} */
  const all = [
    {
      title: 'El modo estudio',
      text: 'Todo para ensayar una canción. Te enseño lo principal: mira las animaciones.',
      demo: 'play'
    },
    {
      el: '.study-transport',
      title: 'La canción',
      text: 'Sonar, pausar, detener y saltar 5 s. También con la barra espaciadora.',
      demo: 'play',
      side: 'bottom'
    },
    {
      el: '.tl-box',
      title: 'Elige un tramo',
      text: 'Arrastra sobre la onda: ese trozo se repite. Ctrl + rueda acerca.',
      demo: 'drag',
      side: 'bottom'
    },
    {
      el: '.tl-lock',
      title: 'El candado',
      text: 'Con él, sonando, un clic no mueve la canción. A su lado, «ir con un clic» para saltar igual.',
      demo: 'lock',
      side: 'left'
    },
    {
      el: '.tl-guide-tracks',
      title: 'Guías',
      text: 'Doble clic en un carril pone una guía: escrita (la dice una voz), grabada o importada.',
      demo: 'guide',
      side: 'top'
    },
    {
      el: '.study-magnet',
      title: 'El imán',
      text: 'Pega las guías a los pulsos del clic, aunque la canción cambie de tempo.',
      demo: 'magnet',
      side: 'bottom'
    },
    {
      el: '.tl-lane-chans',
      title: 'Por dónde suena',
      text: 'L y R en cada carril: la canción a un lado y las guías al otro, como una secuencia. Est o Mono.',
      demo: 'channels',
      side: 'right'
    },
    {
      el: '.study-show-mix',
      title: 'Más sitio',
      text: 'Oculta la mezcla o los canales de los carriles para ver más onda.',
      side: 'bottom'
    },
    {
      el: '.study-col:nth-child(2)',
      title: 'El clic',
      text: 'Un metrónomo pegado a la canción. Ajústalo si se adelanta o se atrasa.',
      side: 'top'
    },
    {
      el: '.study-more',
      title: 'Más',
      text: 'Exportar, las salidas de audio, el estilo de la onda… Este recorrido vuelve con «?».',
      side: 'left'
    }
  ]
  return all
    .filter((s) => !s.el || at(s.el))
    .map((s, i) => ({
      ...(s.el ? { element: /** @type {Element} */ (at(s.el)) } : {}),
      popover: {
        title: s.title,
        description: `<p class="tour-text">${s.text}</p>${s.demo ? DEMOS[s.demo] : ''}`,
        ...(s.side ? { side: s.side, align: /** @type {const} */ ('center') } : {}),
        // el primero, sin «Atrás»
        ...(i === 0
          ? { showButtons: /** @type {Array<'next'|'close'>} */ (['next', 'close']) }
          : {})
      }
    }))
}

/** @type {import('driver.js').Driver | null} */
let current = null

/**
 * Arranca el recorrido por el modo estudio.
 * @param {HTMLElement} root  el modo estudio
 * @returns {import('driver.js').Driver | null}
 */
export function startStudyTour(root) {
  if (!root) return null
  current?.destroy()
  const steps = stepsFor(root)
  current = driver({
    steps,
    animate: true,
    smoothScroll: true,
    allowClose: true,
    stagePadding: 6,
    stageRadius: 10,
    overlayOpacity: 0.62,
    popoverClass: 'dp-tour',
    showProgress: true,
    progressText: '{{current}} de {{total}}',
    nextBtnText: 'Siguiente',
    prevBtnText: 'Atrás',
    doneBtnText: '¡Listo!',
    onDestroyed: () => {
      try {
        localStorage.setItem(TOUR_KEY, 'true')
      } catch {
        /* modo privado */
      }
      current = null
    }
  })
  // visto en cuanto sale: ya no vuelve a salir solo aunque se cierre la app a medias
  try {
    localStorage.setItem(TOUR_KEY, 'true')
  } catch {
    /* modo privado */
  }
  current.drive()
  return current
}

/** Si ya se vio (o se cerró) alguna vez. */
export function tourSeen() {
  try {
    return localStorage.getItem(TOUR_KEY) === 'true'
  } catch {
    return true
  }
}

/** Cierra el recorrido si está abierto (al ocultar el estudio). */
export function stopStudyTour() {
  current?.destroy()
  current = null
}
