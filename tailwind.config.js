/** @type {import('tailwindcss').Config} */
export default {
    darkMode: ['class', '.theme-dark'],
    content: ['./src-ui/**/*.{html,js,svelte,ts}'],
    theme: {
        extend: {
            colors: {
                entropy: {
                    bg: 'rgb(var(--entropy-bg) / <alpha-value>)',
                    surface: 'rgb(var(--entropy-surface) / <alpha-value>)',
                    'surface-light': 'rgb(var(--entropy-surface-light) / <alpha-value>)',
                    primary: 'rgb(var(--entropy-primary) / <alpha-value>)',
                    'primary-dim': 'rgb(var(--entropy-primary-dim) / <alpha-value>)',
                    accent: 'rgb(var(--entropy-accent) / <alpha-value>)',
                    'text-primary': 'rgb(var(--entropy-text-primary) / <alpha-value>)',
                    'text-secondary': 'rgb(var(--entropy-text-secondary) / <alpha-value>)',
                    'text-dim': 'rgb(var(--entropy-text-dim) / <alpha-value>)',
                    border: 'rgb(var(--entropy-border) / <alpha-value>)',
                    'border-bright': 'rgb(var(--entropy-border-bright) / <alpha-value>)',
                }
            }
        },
    },
    plugins: [],
}
