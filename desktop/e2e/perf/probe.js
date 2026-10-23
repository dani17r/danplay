// Lo que se mete en la página antes que nada (`addInitScript`) para medir
// desde dentro, sin las idas y vueltas de Playwright en medio:
//
// - Las tareas largas. WebKit no tiene `longtask` (PerformanceObserver), así
//   que un latido cada 10 ms dice cuánto se retrasó: si llega 60 ms tarde, el
//   hilo estuvo ocupado ese rato. Cuenta como larga a partir de 50 ms.
// - La primera fila de la biblioteca pintada: cuando aparece en el DOM, el
//   siguiente fotograma.
// - Lo que tarda en verse algo tras una acción: `watch(cond)` empieza a mirar
//   (con un MutationObserver, y cada 50 ms por si el cambio no es del DOM)
//   ANTES de hacerla y apunta cuándo fue el clic (o la última tecla); al
//   cumplirse, el fotograma siguiente.
//
// Es una función suelta porque Playwright la serializa: no puede usar nada de
// fuera.
export function probe() {
  const p = {
    gaps: /** @type {[number, number][]} */ ([]),
    firstRow: 0,
    startAt: 0,
    pending: /** @type {Promise<number> | null} */ (null)
  }
  window['__perf'] = p

  let last = performance.now()
  const beat = () => {
    const now = performance.now()
    const late = now - last - 10
    if (late > 50) p.gaps.push([Math.round(last), Math.round(late)])
    last = now
    setTimeout(beat, 10)
  }
  setTimeout(beat, 10)

  const rowsObserver = new MutationObserver(() => {
    if (p.firstRow || !document.querySelector('[data-song-row]')) return
    rowsObserver.disconnect()
    requestAnimationFrame(() => (p.firstRow = performance.now()))
  })
  rowsObserver.observe(document, { subtree: true, childList: true })

  /**
   * @param {() => boolean} cond
   * @param {string} [event] 'click' (el primero) o 'input'/'keydown' (el último)
   */
  p['watch'] = (cond, event = 'click') => {
    p.startAt = 0
    const onEvent = () => {
      if (event === 'click' && p.startAt) return
      p.startAt = performance.now()
    }
    window.addEventListener(event, onEvent, { capture: true })
    p.pending = new Promise((resolve, reject) => {
      let done = false
      const end = () => {
        done = true
        observer.disconnect()
        clearInterval(poll)
        clearTimeout(limit)
        window.removeEventListener(event, onEvent, { capture: true })
      }
      const check = () => {
        if (done || !p.startAt) return
        let ok = false
        try {
          ok = cond()
        } catch {
          /* todavía no está lo que se mira */
        }
        if (!ok) return
        end()
        requestAnimationFrame(() => resolve(performance.now() - p.startAt))
      }
      const observer = new MutationObserver(check)
      observer.observe(document, {
        subtree: true,
        childList: true,
        characterData: true,
        attributes: true
      })
      const poll = setInterval(check, 50)
      const limit = setTimeout(() => {
        end()
        reject(new Error('no se cumple: ' + cond.toString()))
      }, 20_000)
    })
  }
}
