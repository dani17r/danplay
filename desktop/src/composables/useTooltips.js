// @ts-check
// Los avisos de los botones: qué hace cada icono, en un globo que sale al
// dejar el ratón encima un momento (o al llegar con el teclado) y se va al
// salir, al pulsar o al desplazarse.
//
// Se pone en un contenedor (`v-tips`) y vale para todo lo de dentro que
// lleve `title` (o `data-tip`): el `title` se guarda mientras el ratón está
// encima, para que no salga a la vez el aviso del sistema (feo, sin tema y
// a destiempo). Así cada botón explica lo suyo sin un texto largo a la vista.

/** Lo que se espera con el ratón quieto antes de enseñarlo (ms). */
export const TIP_DELAY = 450

/** @type {HTMLDivElement | null} */
let bubble = null
function ensureBubble() {
  if (bubble && document.body.contains(bubble)) return bubble
  bubble = document.createElement('div')
  bubble.className = 'tip'
  bubble.id = 'danplay-tip'
  bubble.setAttribute('role', 'tooltip')
  document.body.appendChild(bubble)
  return bubble
}

/** Lo que dice un elemento (su `title` guardado o su `data-tip`). @param {HTMLElement} el */
const textOf = (el) => (el.dataset.tip || el.getAttribute('title') || '').trim()

/**
 * Pone el globo junto a `el`: encima si cabe, si no debajo; centrado y sin
 * salirse de la ventana.
 * @param {HTMLElement} el @param {string} text
 */
function place(el, text) {
  const tip = ensureBubble()
  tip.textContent = text
  tip.classList.remove('on', 'below')
  tip.style.left = '0px'
  tip.style.top = '0px'
  const r = el.getBoundingClientRect()
  const t = tip.getBoundingClientRect()
  let top = r.top - t.height - 8
  const below = top < 6
  if (below) top = r.bottom + 8
  const left = Math.min(
    Math.max(6, r.left + r.width / 2 - t.width / 2),
    window.innerWidth - t.width - 6
  )
  tip.style.left = `${Math.round(left)}px`
  tip.style.top = `${Math.round(top)}px`
  tip.classList.toggle('below', below)
  requestAnimationFrame(() => tip.classList.add('on'))
}

/** @param {HTMLElement} root */
function install(root) {
  /** @type {HTMLElement | null} */
  let current = null
  /** @type {ReturnType<typeof setTimeout> | null} */
  let timer = null
  /**
   * Lo que se acaba de pulsar: su aviso no vuelve hasta salir de él (al
   * pulsar, su icono cambia y el ratón «entra» otra vez en el botón).
   * @type {HTMLElement | null}
   */
  let pressed = null

  const hide = () => {
    if (timer) clearTimeout(timer)
    timer = null
    if (current) {
      // el title vuelve a su sitio (por si se mira sin el ratón, o lo lee
      // quien no ve la pantalla)
      if (current.dataset.tipFrom === 'title' && current.dataset.tip) {
        current.setAttribute('title', current.dataset.tip)
      }
      current.removeAttribute('aria-describedby')
      current = null
    }
    bubble?.classList.remove('on')
  }
  /** @param {Event} e @param {number} delay */
  const start = (e, delay) => {
    const target = /** @type {HTMLElement | null} */ (e.target)
    const el = /** @type {HTMLElement | null} */ (target?.closest?.('[title], [data-tip]'))
    if (!el || !root.contains(el) || el === current || el === pressed) return
    hide()
    if (el.hasAttribute('title')) {
      el.dataset.tip = el.getAttribute('title') || ''
      el.dataset.tipFrom = 'title'
      el.removeAttribute('title')
    }
    if (!textOf(el)) return
    current = el
    timer = setTimeout(() => {
      if (current !== el || !document.body.contains(el)) return
      place(el, textOf(el))
      el.setAttribute('aria-describedby', 'danplay-tip')
    }, delay)
  }
  /** @param {PointerEvent} e */
  const over = (e) => start(e, TIP_DELAY)
  /** @param {PointerEvent} e */
  const out = (e) => {
    const to = /** @type {Node | null} */ (e.relatedTarget)
    if (pressed && !(to && pressed.contains(to))) pressed = null
    if (!current) return
    if (to && current.contains(to)) return
    hide()
  }
  /** @param {PointerEvent} e */
  const down = (e) => {
    const target = /** @type {HTMLElement | null} */ (e.target)
    pressed = /** @type {HTMLElement | null} */ (target?.closest?.('[title], [data-tip]')) || null
    hide()
  }
  /** @param {FocusEvent} e */
  const focus = (e) => {
    // solo con el teclado: con el ratón ya lo lleva `over`
    const el = /** @type {HTMLElement | null} */ (e.target)
    let byKeyboard = false
    try {
      byKeyboard = !!el?.matches?.(':focus-visible')
    } catch {
      // un navegador que no sabe de :focus-visible: sin aviso al enfocar
    }
    if (byKeyboard) start(e, TIP_DELAY)
  }
  /** @param {KeyboardEvent} e */
  const key = (e) => {
    if (e.key === 'Escape') hide()
  }
  root.addEventListener('pointerover', over)
  root.addEventListener('pointerout', out)
  root.addEventListener('pointerdown', down, true)
  root.addEventListener('focusin', focus)
  root.addEventListener('focusout', hide)
  root.addEventListener('keydown', key)
  window.addEventListener('scroll', hide, true)
  window.addEventListener('blur', hide)
  return () => {
    hide()
    root.removeEventListener('pointerover', over)
    root.removeEventListener('pointerout', out)
    root.removeEventListener('pointerdown', down, true)
    root.removeEventListener('focusin', focus)
    root.removeEventListener('focusout', hide)
    root.removeEventListener('keydown', key)
    window.removeEventListener('scroll', hide, true)
    window.removeEventListener('blur', hide)
  }
}

const uninstall = new WeakMap()

/** `v-tips` en un contenedor: avisos propios para todo lo de dentro con `title`. */
export const vTips = {
  /** @param {HTMLElement} el */
  mounted(el) {
    uninstall.set(el, install(el))
  },
  /** @param {HTMLElement} el */
  unmounted(el) {
    uninstall.get(el)?.()
    uninstall.delete(el)
  }
}
