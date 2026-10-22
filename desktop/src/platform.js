// @ts-check
// En qué app corre la interfaz: la de escritorio o la del celular
// (`mobile.html`, la app de `mobile/`). Es la misma interfaz; en el celular se
// esconde lo que solo se puede hacer en la PC: separar, guardar mezclas,
// llevar al celular, abrir carpetas, editar el archivo, buscar con la IA…
// Lo marca la propia página (`<html data-app="mobile">`).

/** La app del celular: solo importa pistas ya separadas y las estudia. */
export const isMobileApp =
  typeof document !== 'undefined' && document.documentElement.dataset.app === 'mobile'
