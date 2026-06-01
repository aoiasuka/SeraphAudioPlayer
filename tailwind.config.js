/** @type {import('tailwindcss').Config} */
export default {
  darkMode: ["class"],
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    container: {
      center: true,
      padding: "2rem",
      screens: { "2xl": "1400px" },
    },
    extend: {
      colors: {
        seraph: {
          gold: "#d4af37",
          "gold-light": "#fffbf0",
          "gold-dark": "#b48a12",
        },
      },
      fontFamily: {
        sans: [
          "Inter",
          "Segoe UI",
          "-apple-system",
          "BlinkMacSystemFont",
          "sans-serif",
        ],
        mono: [
          "ui-monospace",
          "SFMono-Regular",
          "Menlo",
          "Monaco",
          "Consolas",
          "monospace",
        ],
      },
      keyframes: {
        "gentle-breath": {
          "0%, 100%": {
            transform: "scale(1)",
            boxShadow: "0 12px 35px rgba(6, 182, 212, 0.12)",
          },
          "50%": {
            transform: "scale(1.012)",
            boxShadow: "0 18px 45px rgba(139, 92, 246, 0.2)",
          },
        },
        "spin-slow": {
          "0%": { transform: "rotate(0deg)" },
          "100%": { transform: "rotate(360deg)" },
        },
        "slide-in-right": {
          "0%": { transform: "translateX(120%)", opacity: "0" },
          "100%": { transform: "translateX(0)", opacity: "1" },
        },
        "slide-out-right": {
          "0%": { transform: "translateX(0)", opacity: "1" },
          "100%": { transform: "translateX(120%)", opacity: "0" },
        },
      },
      animation: {
        "gentle-breath": "gentle-breath 12s ease-in-out infinite",
        "spin-slow": "spin-slow 8s linear infinite",
        "slide-in-right": "slide-in-right 500ms cubic-bezier(0.16, 1, 0.3, 1)",
        "slide-out-right": "slide-out-right 500ms cubic-bezier(0.16, 1, 0.3, 1)",
      },
    },
  },
  plugins: [require("tailwindcss-animate")],
};
