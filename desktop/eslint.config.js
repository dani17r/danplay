// Reglas de la interfaz. No había ninguna, y por eso convivían un `columns`
// que siempre valía null, una rama `already > 1 ? 'ias' : 'ias'` con las dos
// mitades iguales y una función llamada `cancelDownload` que cancelaba un tema.
import js from '@eslint/js'
import vue from 'eslint-plugin-vue'
import globals from 'globals'

export default [
  { ignores: ['dist/**', 'src-tauri/**', 'node_modules/**'] },
  js.configs.recommended,
  ...vue.configs['flat/recommended'],
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
      'no-unused-vars': ['error', { argsIgnorePattern: '^_', caughtErrors: 'none' }],
      'no-console': ['warn', { allow: ['warn', 'error'] }],
      eqeqeq: ['error', 'always', { null: 'ignore' }],
      // el código va en inglés, pero los textos que ve el usuario en
      // castellano: nada de reglas sobre las cadenas
      'vue/multi-word-component-names': 'off',
      'vue/no-unused-properties': ['error', { groups: ['props'] }],
      'vue/require-explicit-emits': 'error',
      'vue/no-v-html': 'off', // solo Icon.vue, con constantes propias
      'vue/max-attributes-per-line': 'off',
      'vue/singleline-html-element-content-newline': 'off',
      'vue/html-self-closing': 'off',
      'vue/html-indent': 'off',
      'vue/html-closing-bracket-newline': 'off',
      'vue/attributes-order': 'off',
      'vue/first-attribute-linebreak': 'off',
      'vue/one-component-per-file': 'off'
    }
  },
  {
    files: ['tests/**/*.js'],
    languageOptions: { globals: { ...globals.node } }
  },
  {
    files: ['scripts/**/*.mjs', '*.config.js'],
    languageOptions: { globals: { ...globals.node } }
  }
]
