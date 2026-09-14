/** @type {import('tailwindcss').Config} */
// 設計 token 取自 docs/UI 設計/stitch_taiwan_manufacturing_erp_portal
// Material Design 3 配色。修改前請先確認設計稿，避免前端與設計不一致。
export default {
  darkMode: 'class',
  content: ['./index.html', './src/**/*.{vue,js,ts,jsx,tsx}'],
  theme: {
    extend: {
      colors: {
        "on-tertiary": "#ffffff",
        "surface-bright": "#f7f9fb",
        "surface-container-highest": "#e0e3e5",
        "on-tertiary-fixed-variant": "#38485d",
        "secondary": "#565e74",
        "on-primary-fixed-variant": "#003ea8",
        "inverse-surface": "#2d3133",
        "on-surface-variant": "#434655",
        "error": "#ba1a1a",
        "on-tertiary-container": "#e9f0ff",
        "surface-container-high": "#e6e8ea",
        "on-secondary-fixed-variant": "#3f465c",
        "primary-container": "#2563eb",
        "surface": "#f7f9fb",
        "outline": "#737686",
        "surface-container-lowest": "#ffffff",
        "background": "#f7f9fb",
        "primary": "#004ac6",
        "tertiary-container": "#5e6e85",
        "secondary-fixed-dim": "#bec6e0",
        "on-tertiary-fixed": "#0b1c30",
        "on-secondary-fixed": "#131b2e",
        "secondary-fixed": "#dae2fd",
        "outline-variant": "#c3c6d7",
        "on-primary-fixed": "#00174b",
        "primary-fixed": "#dbe1ff",
        "surface-tint": "#0053db",
        "surface-container-low": "#f2f4f6",
        "tertiary": "#46566c",
        "on-primary-container": "#eeefff",
        "inverse-on-surface": "#eff1f3",
        "on-surface": "#191c1e",
        "error-container": "#ffdad6",
        "tertiary-fixed": "#d3e4fe",
        "on-secondary-container": "#5c647a",
        "secondary-container": "#dae2fd",
        "on-error": "#ffffff",
        "on-background": "#191c1e",
        "primary-fixed-dim": "#b4c5ff",
        "tertiary-fixed-dim": "#b7c8e1",
        "on-secondary": "#ffffff",
        "surface-dim": "#d8dadc",
        "surface-container": "#eceef0",
        "inverse-primary": "#b4c5ff",
        "on-error-container": "#93000a",
        "surface-variant": "#e0e3e5",
        "on-primary": "#ffffff"
      },
      borderRadius: {
        "DEFAULT": "0.125rem",
        "lg": "0.25rem",
        "xl": "0.5rem",
        "full": "0.75rem"
      },
      spacing: {
        "gutter": "1rem",
        "space-xl": "1.5rem",
        "space-lg": "1rem",
        "space-xs": "0.25rem",
        "space-sm": "0.5rem",
        "margin": "1rem",
        "space-md": "0.75rem"
      },
      fontFamily: {
        "body-dense": [
          "Noto Sans"
        ],
        "headline-xl": [
          "Noto Sans"
        ],
        "label-caption": [
          "Noto Sans"
        ],
        "data-mono": [
          "JetBrains Mono"
        ],
        "headline-lg": [
          "Noto Sans"
        ],
        "title-md": [
          "Noto Sans"
        ],
        "body-default": [
          "Noto Sans"
        ],
        "label-header": [
          "Noto Sans"
        ]
      },
      fontSize: {
        "body-dense": [
          "13px",
          {
            "lineHeight": "18px",
            "fontWeight": "400"
          }
        ],
        "headline-xl": [
          "24px",
          {
            "lineHeight": "32px",
            "letterSpacing": "-0.01em",
            "fontWeight": "600"
          }
        ],
        "label-caption": [
          "11px",
          {
            "lineHeight": "14px",
            "fontWeight": "400"
          }
        ],
        "data-mono": [
          "13px",
          {
            "lineHeight": "18px",
            "fontWeight": "500"
          }
        ],
        "headline-lg": [
          "18px",
          {
            "lineHeight": "24px",
            "letterSpacing": "-0.005em",
            "fontWeight": "600"
          }
        ],
        "title-md": [
          "15px",
          {
            "lineHeight": "20px",
            "fontWeight": "600"
          }
        ],
        "body-default": [
          "14px",
          {
            "lineHeight": "20px",
            "fontWeight": "400"
          }
        ],
        "label-header": [
          "12px",
          {
            "lineHeight": "16px",
            "letterSpacing": "0.02em",
            "fontWeight": "600"
          }
        ]
      },
    },
  },
  plugins: [],
}
