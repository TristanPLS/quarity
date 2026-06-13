import js from '@eslint/js'
import globals from 'globals'
import reactHooks from 'eslint-plugin-react-hooks'
import reactRefresh from 'eslint-plugin-react-refresh'
import tseslint from 'typescript-eslint'

// Config plate (ESLint 9) — base create-vite react-ts. Lint scopé à `src/` (code
// navigateur) via le script `npm run lint` ; les fichiers de config (vite/eslint)
// ne sont pas linté (pas de risque, et ils utilisent des globals Node).
export default tseslint.config(
  { ignores: ['dist'] },
  {
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    files: ['**/*.{ts,tsx}'],
    languageOptions: {
      ecmaVersion: 2021,
      globals: globals.browser,
    },
    plugins: {
      'react-hooks': reactHooks,
      'react-refresh': reactRefresh,
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      // On garde les règles CLASSIQUES à valeur sûre du set recommended :
      //   react-hooks/rules-of-hooks (error) + react-hooks/exhaustive-deps (warn).
      // En revanche les règles « React Compiler » introduites par react-hooks v7 dans
      // « recommended » flaguent des patterns idiomatiques et corrects (réinitialiser
      // un filtre via setState dans un effet, mémoïsation manuelle) — désactivées :
      'react-hooks/set-state-in-effect': 'off',
      'react-hooks/preserve-manual-memoization': 'off',
      'react-refresh/only-export-components': [
        'warn',
        { allowConstantExport: true },
      ],
    },
  },
)
