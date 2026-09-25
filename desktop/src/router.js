// @ts-check
// Las páginas de la ventana principal: Ajustes, el asistente, Descargas,
// Entrada y Duplicados. Van por el enrutador para cargarse al abrirlas (cada
// una es un trozo aparte del paquete: el arranque no carga Ajustes ni el
// chat) y para que el asistente se quede vivo en <KeepAlive> al salir de él.
//
// La historia es de memoria: no hay barra de direcciones que enseñar, y la
// misma página sirve también la ventanita (?mini=1) y la proyección
// (?projection=1), que no llevan enrutador (ver main.js).
//
// La lista de canciones (Todas, Favoritos, Artistas, un repertorio…) no es
// una ruta: es la vista de siempre, y cuál se ve lo dice `view` en App.vue.
import { createRouter, createMemoryHistory } from 'vue-router'

/** Cada página, por su `view.kind`. */
export const PAGE_ROUTES = [
  {
    path: '/ajustes',
    name: 'settings',
    component: () => import('./components/SettingsPage.vue')
  },
  {
    path: '/asistente',
    name: 'chat',
    component: () => import('./components/ChatPage.vue')
  },
  {
    path: '/descargas',
    name: 'downloads',
    component: () => import('./components/DownloadsPage.vue')
  },
  {
    path: '/entrada',
    name: 'inbox',
    component: () => import('./components/InboxPage.vue')
  },
  {
    path: '/duplicados',
    name: 'duplicates',
    component: () => import('./components/DuplicatesPage.vue')
  }
]

/** La ruta de una vista: la de su página, o la de la lista. @param {{kind: string}} view */
export function routeFor(view) {
  return PAGE_ROUTES.some((r) => r.name === view.kind) ? { name: view.kind } : { name: 'library' }
}

export function createAppRouter() {
  return createRouter({
    history: createMemoryHistory(),
    routes: [
      // la lista: la pinta App.vue; aquí no hay nada que cargar
      { path: '/', name: 'library', component: { render: () => null } },
      ...PAGE_ROUTES
    ]
  })
}
