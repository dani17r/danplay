// Las islas (src/islands.js): las partes de la ventana que llegan cuando
// hacen falta, o un poco antes. Aquí, con islas de prueba: las de la app ya
// las carga tests/setup.js para que el resto de pruebas no tenga que esperar.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { defineComponent, h, withDirectives, ref, nextTick } from 'vue'
import { island, preloadWhenIdle, vPreload, warm } from '../src/islands.js'

const Part = defineComponent({ name: 'Part', render: () => h('p', 'una parte') })

/** Una isla con su cargador espiado, que devuelve `Part` cuando se le diga. */
function fake() {
  let release
  const gate = new Promise((r) => (release = r))
  const loader = vi.fn(() => gate.then(() => ({ default: Part })))
  return { one: island(loader), loader, release: () => release() }
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('an island', () => {
  it('is null until it arrives, and is loaded once however many times it is asked for', async () => {
    const { one, loader, release } = fake()
    expect(one.component.value).toBeNull()
    const a = one.load()
    const b = one.load()
    expect(loader).toHaveBeenCalledTimes(1)
    release()
    expect(await a).toBe(Part)
    expect(await b).toBe(Part)
    expect(one.component.value).toBe(Part)
    await one.load()
    expect(loader).toHaveBeenCalledTimes(1)
  })

  it('if its chunk does not arrive, it says so, and the next time it tries again', async () => {
    const loader = vi
      .fn()
      .mockRejectedValueOnce(new Error('sin red'))
      .mockResolvedValue({ default: Part })
    const one = island(loader)
    await expect(one.load()).rejects.toThrow('sin red')
    expect(one.component.value).toBeNull()
    await expect(one.load()).resolves.toBe(Part)
    expect(loader).toHaveBeenCalledTimes(2)
  })

  it('warming it up never throws: it will be retried when opened', async () => {
    const one = island(() => Promise.reject(new Error('sin red')))
    expect(() => warm(one)).not.toThrow()
    await flushPromises()
  })
})

describe('v-preload', () => {
  /** Un botón con la directiva; `which` decide qué isla lleva. */
  function button(which) {
    return mount(
      defineComponent({
        setup: () => () =>
          withDirectives(h('button', { type: 'button' }, 'Abrir'), [[vPreload, which.value]])
      })
    )
  }

  it('the mouse over the button brings its island, before the click', async () => {
    const { one, loader } = fake()
    const w = button(ref(one))
    expect(loader).not.toHaveBeenCalled()
    await w.find('button').trigger('pointerenter')
    expect(loader).toHaveBeenCalledTimes(1)
  })

  it('so does the keyboard focus, and it can be a list or nothing at all', async () => {
    const a = fake()
    const b = fake()
    const which = ref([a.one, b.one])
    const w = button(which)
    await w.find('button').trigger('focusin')
    expect(a.loader).toHaveBeenCalledTimes(1)
    expect(b.loader).toHaveBeenCalledTimes(1)
    // la tabla no es una isla: sin nada, no hace nada
    which.value = null
    await nextTick()
    await w.find('button').trigger('pointerenter')
    expect(a.loader).toHaveBeenCalledTimes(1)
  })

  it('it follows the island it is given now, not the first one', async () => {
    const a = fake()
    const b = fake()
    const which = ref(a.one)
    const w = button(which)
    which.value = b.one
    await nextTick()
    await w.find('button').trigger('pointerenter')
    expect(a.loader).not.toHaveBeenCalled()
    expect(b.loader).toHaveBeenCalledTimes(1)
  })
})

describe('preloading in idle time', () => {
  it('loads them one by one, each in its own idle slot, in the order given', async () => {
    const idle = []
    vi.stubGlobal('requestIdleCallback', (fn) => idle.push(fn))
    const a = fake()
    const b = fake()
    preloadWhenIdle([a.one, b.one])
    expect(a.loader).not.toHaveBeenCalled()
    idle.shift()()
    expect(a.loader).toHaveBeenCalledTimes(1)
    expect(b.loader).not.toHaveBeenCalled()
    // la segunda espera a que llegue la primera y a otro rato libre
    a.release()
    await flushPromises()
    expect(b.loader).not.toHaveBeenCalled()
    idle.shift()()
    expect(b.loader).toHaveBeenCalledTimes(1)
  })

  it('one that fails does not stop the rest', async () => {
    const idle = []
    vi.stubGlobal('requestIdleCallback', (fn) => idle.push(fn))
    const broken = island(() => Promise.reject(new Error('sin red')))
    const b = fake()
    preloadWhenIdle([broken, b.one])
    idle.shift()()
    await flushPromises()
    idle.shift()()
    expect(b.loader).toHaveBeenCalledTimes(1)
  })
})
