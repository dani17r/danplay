// Reglas de la interfaz. No había ninguna, y por eso convivían un `columns`
// que siempre valía null, una rama `already > 1 ? 'ias' : 'ias'` con las dos
// mitades iguales y una función llamada `cancelDownload` que cancelaba un tema.
//
// El formato (sangrías, saltos de línea, comillas) no es cosa de ESLint: lo
// deja Oxfmt (`vp fmt`, que formatea como Prettier), y `eslint-config-prettier`
// apaga aquí lo que chocaría con él. La accesibilidad va en aviso: que se vea
// sin tumbar nada, y el objetivo es cero avisos.
//
// ¿Por qué no Oxlint, que viene con Vite+? No lee las plantillas de Vue: de
// las 190 reglas de aquí cubre 99 (las de JavaScript y las de Vue que miran
// el <script>), y las 91 que faltan son justo las de las plantillas y las de
// accesibilidad. Repartirlas entre los dos (Oxlint para lo suyo y ESLint, con
// eslint-plugin-oxlint, para el resto) ahorraba 0,3 s, y con
// `--concurrency auto` ni eso. Así que todo sigue aquí, en varios hilos y con
// caché (en node_modules/.cache/eslint): la segunda vez solo mira lo que cambió.
import js from '@eslint/js'
import vue from 'eslint-plugin-vue'
import a11y from 'eslint-plugin-vuejs-accessibility'
import prettier from 'eslint-config-prettier/flat'
import globals from 'globals'

/** Las reglas de accesibilidad recomendadas, todas como aviso. */
const a11yAsWarnings = a11y.configs['flat/recommended'].map((c) =>
  c.rules ? { ...c, rules: Object.fromEntries(Object.keys(c.rules).map((r) => [r, 'warn'])) } : c
)

export default [
  {
    ignores: [
      'dist/**',
      'src-tauri/**',
      'node_modules/**',
      'coverage/**',
      'playwright-report/**',
      'test-results/**'
    ]
  },
  js.configs.recommended,
  ...vue.configs['flat/recommended'],
  ...a11yAsWarnings,
  {
    files: ['**/*.{js,mjs,vue}'],
    languageOptions: {
      ecmaVersion: 2024,
      sourceType: 'module',
      globals: { ...globals.browser, ...globals.es2024 }
    },
    rules: {
      // los `catch {}` a propósito llevan comentario dentro; lo que no vale
      // es dejarse una variable o un import por el camino
      'no-unused-vars': [
        'error',
        { argsIgnorePattern: '^_', caughtErrors: 'none', ignoreRestSiblings: true }
      ],
      'no-console': ['warn', { allow: ['warn', 'error'] }],
      eqeqeq: ['error', 'always', { null: 'ignore' }],
      // el código va en inglés, pero los textos que ve el usuario en
      // castellano: nada de reglas sobre las cadenas
      'vue/multi-word-component-names': 'off',
      'vue/no-unused-properties': ['error', { groups: ['props'] }],
      'vue/require-explicit-emits': 'error',
      // v-html solo donde se pinta marcado propio o ya escapado (los iconos,
      // el markdown del asistente); cada uso lleva su desactivación y el motivo
      'vue/no-v-html': 'error',
      'vue/one-component-per-file': 'off',
      // una etiqueta que envuelve su campo ya lo nombra: no hace falta
      // además un `for`/`id` (la regla pedía las dos cosas a la vez)
      'vuejs-accessibility/label-has-for': ['warn', { required: { some: ['nesting', 'id'] } }]
    }
  },
  {
    files: ['tests/**/*.js', 'e2e/**/*.js'],
    languageOptions: { globals: { ...globals.node } }
  },
  {
    // herramientas de línea de órdenes (y el servidor de las e2e): escribir
    // en la consola es su trabajo
    files: ['scripts/**/*.mjs', 'e2e/**/*.mjs', '*.config.js'],
    languageOptions: { globals: { ...globals.node } },
    rules: { 'no-console': 'off' }
  },
  // lo último: apaga las reglas de formato que chocarían con Oxfmt
  prettier
]
