// El buscador de la barra superior.
//
// En «Todas las canciones» filtra la propia lista. En cualquier otro sitio esa
// lista es otra cosa (un repertorio, los favoritos) y filtrarla no vale: se
// busca en TODA la biblioteca y el resultado sale en un desplegable, sin
// sacarte de donde estabas.
//
// Estaba dentro de App.vue, con su retardo y sus siete refs.
import { ref, computed, watch, onUnmounted } from 'vue'
import { api } from '../api.js'

// Escribir NO dispara una búsqueda por tecla: cada una era una consulta al
// núcleo y repintar la lista entera, así que escribir «barak» eran cinco. Se
// espera a que pares un momento.
export const SEARCH_DELAY = 180

/**
 * @param {{
 *   view: import('vue').Ref<{kind: string}>,
 *   reload: () => any,        vuelve a pedir la lista de la página
 * }} context
 */
export function useSearch(context) {
  const query = ref('')
  const quick = ref([])
  const quickLoading = ref(false)
  const quickOpen = ref(false)
  const advanced = ref(false)
  const facets = ref({})
  const onlyFavorites = ref(false)
  const minStars = ref(0)

  /** En estas vistas el buscador filtra la lista que se está viendo. */
  const filtersTheList = computed(() => ['all', 'artists'].includes(context.view.value.kind))

  let timer = null
  watch(query, () => {
    clearTimeout(timer)
    timer = setTimeout(run, SEARCH_DELAY)
  })
  onUnmounted(() => clearTimeout(timer))

  async function run() {
    if (filtersTheList.value) {
      quickOpen.value = false
      return context.reload()
    }
    const q = query.value.trim()
    if (!q) {
      quickOpen.value = false
      quick.value = []
      return
    }
    quickOpen.value = true
    quickLoading.value = true
    try {
      quick.value = (await api.quickSearch(q, 60)).songs || []
    } catch {
      quick.value = []
    } finally {
      quickLoading.value = false
    }
  }

  async function loadFacets() {
    try {
      facets.value = await api.facets()
    } catch {
      /* el núcleo aún no responde; se vuelve a intentar al abrirlo */
    }
  }
  watch(advanced, (open) => {
    if (open && !facets.value.artists) loadFacets()
  })
  // al cambiar de sitio, el desplegable sobra
  watch(context.view, () => {
    quickOpen.value = false
  })
  watch([onlyFavorites, minStars], () => {
    if (filtersTheList.value) context.reload()
  })

  /** Cierra lo que esté abierto. Lo usan Escape y el clic fuera. */
  function close() {
    quickOpen.value = false
    advanced.value = false
  }

  /** Los filtros que hay que añadir a la consulta del núcleo. */
  function filters() {
    const p = {}
    if (onlyFavorites.value) p.only_favorites = true
    if (minStars.value) p.min_stars = minStars.value
    return p
  }

  return {
    query,
    quick,
    quickLoading,
    quickOpen,
    advanced,
    facets,
    onlyFavorites,
    minStars,
    filtersTheList,
    filters,
    run,
    close
  }
}
