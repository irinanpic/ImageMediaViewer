/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      colors: {
        background: "#121212",
        surface: "#1e1e1e",
        surfaceLight: "#2a2a2a",
        border: "#333333",
        textPrimary: "#f3f4f6",
        textSecondary: "#9ca3af",
        accent: "#3b82f6",
        accentHover: "#2563eb",
      },
    },
  },
  plugins: [],
}
