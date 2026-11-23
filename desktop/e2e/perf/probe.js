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
// - Lo que cuesta cada aviso de posición del reproductor (el `timeupdate` del
//   <audio>, cuatro por segundo): del aviso a que Vue acaba de repintar lo que
//   dependía de él (sus microtareas), en `ticks` ([cuándo, ms]).
//
// Es una función suelta porque Playwright la serializa: no puede usar nada de
// fuera.
export function probe() {
  const p = {
    gaps: /** @type {[number, number][]} */ ([]),
    // lo que el latido llega tarde, sumado (también lo de menos de 50 ms):
    // cuánto tiempo estuvo ocupado el hilo, para lo que no hace tareas
    // largas pero trabaja sin parar (el estudio sonando)
    busy: 0,
    ticks: /** @type {[number, number][]} */ ([]),
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
    if (late > 0) p.busy += late
    last = now
    setTimeout(beat, 10)
  }
  setTimeout(beat, 10)

  // Vue repinta en una microtarea que se pone en cola durante el aviso: dos
  // vueltas de microtareas después, ya acabó.
  const listen = HTMLMediaElement.prototype.addEventListener
  HTMLMediaElement.prototype.addEventListener = function (type, fn, options) {
    if (type !== 'timeupdate' || typeof fn !== 'function') {
      return listen.call(this, type, fn, options)
    }
    const timed = function (/** @type {Event} */ e) {
      const t0 = performance.now()
      try {
        return fn.call(this, e)
      } finally {
        Promise.resolve()
          .then(() => Promise.resolve())
          .then(() => p.ticks.push([t0, performance.now() - t0]))
      }
    }
    return listen.call(this, type, timed, options)
  }

  /**
   * Los fotogramas de un rato: lo que dura cada uno (media, p95, el peor y
   * cuántos pasan de 33 ms) y lo ocupado que estuvo el hilo, en ms por
   * segundo. Dura al menos `ms`, y más mientras `each(i)` (llamada en cada
   * fotograma, antes de pintarlo) devuelva `true`.
   * @param {number} ms
   * @param {(i: number) => boolean | void} [each]
   */
  p['frames'] = (ms, each) =>
    new Promise((done) => {
      const deltas = []
      const busy0 = p.busy
      let first = 0
      let prev = 0
      let i = 0
      const step = (/** @type {number} */ t) => {
        if (!first) first = t
        else deltas.push(t - prev)
        prev = t
        const more = each ? each(i++) === true : false
        if (more || t - first < ms) return requestAnimationFrame(step)
        const s = [...deltas].sort((a, b) => a - b)
        const secs = Math.max(0.001, (t - first) / 1000)
        done({
          frames: s.length,
          mean: s.reduce((a, b) => a + b, 0) / Math.max(1, s.length),
          p95: s[Math.floor(s.length * 0.95)] ?? NaN,
          max: s[s.length - 1] ?? NaN,
          slow: s.filter((d) => d > 33.4).length,
          busy: (p.busy - busy0) / secs
        })
      }
      requestAnimationFrame(step)
    })

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
