export type Theme = "system" | "light" | "dark";

const KEY = "acelus.theme";

export function currentTheme(): Theme {
  const forced = new URLSearchParams(location.search).get("theme");
  if (forced === "light" || forced === "dark") return forced;
  const stored = localStorage.getItem(KEY);
  return stored === "light" || stored === "dark" ? stored : "system";
}

export function applyTheme(theme: Theme): void {
  const root = document.documentElement;
  if (theme === "system") {
    root.removeAttribute("data-theme");
    localStorage.removeItem(KEY);
  } else {
    root.setAttribute("data-theme", theme);
    localStorage.setItem(KEY, theme);
  }
}

export function nextTheme(theme: Theme): Theme {
  if (theme === "system") return "dark";
  if (theme === "dark") return "light";
  return "system";
}

export function themeLabel(theme: Theme): string {
  return theme === "system" ? "system theme" : `${theme} theme`;
}
