export type Theme = "auto" | "light" | "dark";

const KEY = "ystreamer.theme";

function load(): Theme {
  try {
    const saved = localStorage.getItem(KEY);
    // Dark unless chosen otherwise; "auto" follows the system
    return saved === "light" || saved === "auto" ? saved : "dark";
  } catch {
    return "dark";
  }
}

class ThemeStore {
  value = $state<Theme>(load());

  set(theme: Theme) {
    this.value = theme;
    try {
      if (theme === "dark") localStorage.removeItem(KEY);
      else localStorage.setItem(KEY, theme);
    } catch {}
    apply(theme);
  }
}

function apply(theme: Theme) {
  const root = document.documentElement;
  root.classList.toggle("light", theme === "light");
  root.classList.toggle("dark", theme === "dark");
  root.style.colorScheme = theme === "auto" ? "light dark" : theme;
}

export const theme = new ThemeStore();
