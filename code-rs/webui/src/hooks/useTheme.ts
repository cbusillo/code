import { ref, watch } from "vue";

const THEME_KEY = "codeTheme";

export type ThemeMode = "dark" | "light";

export const useTheme = () => {
  const stored = localStorage.getItem(THEME_KEY);
  const theme = ref<ThemeMode>(stored === "light" ? "light" : "dark");

  watch(
    theme,
    (value) => {
      document.documentElement.dataset.theme = value;
      localStorage.setItem(THEME_KEY, value);
    },
    { immediate: true },
  );

  const toggle = () => {
    theme.value = theme.value === "dark" ? "light" : "dark";
  };

  const setTheme = (value: ThemeMode) => {
    theme.value = value;
  };

  return { theme, toggle, setTheme };
};
