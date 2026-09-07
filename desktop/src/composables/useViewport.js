// Tamaño de ventana, en tres tramos.
//
// La app de escritorio y la de movil comparten esta base, asi que en vez de
// repartir media queries por todos lados hay un solo sitio que decide en que
// tramo estamos. El css sigue usando media queries para lo puramente visual;
// esto es para lo que cambia de COMPORTAMIENTO: en estrecho los paneles
// laterales dejan de ocupar sitio y pasan a abrirse encima.
import { ref, computed, onMounted, onUnmounted, readonly } from 'vue'

export const PHONE_MAX = 700
export const TABLET_MAX = 1000

const width = ref(typeof window !== 'undefined' ? window.innerWidth : 1400)
const height = ref(typeof window !== 'undefined' ? window.innerHeight : 900)
let listeners = 0

function measure () {
  width.value = window.innerWidth
  height.value = window.innerHeight
}

export function useViewport () {
  onMounted(() => {
    if (listeners++ === 0) window.addEventListener('resize', measure, { passive: true })
    measure()
  })
  onUnmounted(() => {
    if (--listeners === 0) window.removeEventListener('resize', measure)
  })

  const isPhone = computed(() => width.value <= PHONE_MAX)
  const isTablet = computed(() => width.value > PHONE_MAX && width.value <= TABLET_MAX)
  // «compacto» = los laterales estorban y pasan a abrirse encima
  const isCompact = computed(() => width.value <= TABLET_MAX)
  const isDesktop = computed(() => width.value > TABLET_MAX)

  return {
    width: readonly(width),
    height: readonly(height),
    isPhone, isTablet, isCompact, isDesktop
  }
}
