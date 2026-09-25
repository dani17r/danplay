// Las páginas de la app (Ajustes, el asistente, Descargas, Entrada y
// Duplicados) se cargan aparte al abrirlas (ver src/router.js). Después de
// pulsar su enlace hay que esperar a que llegue su código, no solo a las
// promesas de siempre.
import { vi } from 'vitest'
import { flushPromises } from '@vue/test-utils'

/** Espera a que la página que se acaba de abrir esté pintada. */
export async function settle() {
  for (let i = 0; i < 3; i++) {
    await vi.dynamicImportSettled()
    await flushPromises()
  }
}
