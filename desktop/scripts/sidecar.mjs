// Tauri exige que exista el binario del núcleo (`bundle.externalBin`) para
// compilar, también en desarrollo y en las pruebas de Rust. En un clon recién
// hecho no lo hay: el de verdad lo genera `scripts/build.sh` con PyInstaller.
//
// Esto deja uno de relleno si falta. La app lo reconoce por su marca (ver
// `is_placeholder` en src-tauri/src/core.rs) y arranca el núcleo desde el
// código del proyecto. Si ya hay uno de verdad, no toca nada.
//
//   node scripts/sidecar.mjs [triple]    (sin triple: el del propio equipo)
import { execFileSync } from 'node:child_process'
import { chmodSync, existsSync, mkdirSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const folder = join(here, '..', 'src-tauri', 'binaries')
const triple =
  process.argv[2] ||
  execFileSync('rustc', ['-vV'], { encoding: 'utf8' })
    .match(/^host: (.+)$/m)[1]
    .trim()
const target = join(folder, `danplay-core-${triple}${triple.includes('windows') ? '.exe' : ''}`)

if (!existsSync(target)) {
  mkdirSync(folder, { recursive: true })
  writeFileSync(
    target,
    '#!/bin/sh\n' +
      '# danplay-core de relleno: el nucleo de verdad lo genera scripts/build.sh\n' +
      'echo "DanPlay: este nucleo es de relleno; genera el de verdad con scripts/build.sh" >&2\n' +
      'exit 1\n'
  )
  try {
    chmodSync(target, 0o755)
  } catch {
    /* en Windows no hay permisos de ejecución que poner */
  }
  console.log(`núcleo de relleno en ${target}`)
}
