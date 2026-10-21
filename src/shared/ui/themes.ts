/**
 * The themes the build carries, so the user can switch at runtime instead of
 * asking for a rebuild.
 */
export interface ThemeOption {
  label: string;
  /** The class name under `qx.theme`. */
  value: string;
}

export const THEMES: ThemeOption[] = [
  { label: "Modern", value: "Modern" },
  { label: "Indigo", value: "Indigo" },
  { label: "Indigo dark", value: "IndigoDark" },
  { label: "Tangible light", value: "TangibleLight" },
  { label: "Tangible dark", value: "TangibleDark" },
  { label: "Simple", value: "Simple" },
  { label: "Classic", value: "Classic" },
];

export const DEFAULT_THEME = "Modern";
export const THEME_STORAGE_KEY = "smppclient.theme";

/** The theme to start with: the remembered one, or the default. */
export function resolveTheme(remembered: string | null | undefined): string {
  const found = THEMES.find((t) => t.value === remembered);
  return found ? found.value : DEFAULT_THEME;
}

export function themeLabel(value: string): string {
  const found = THEMES.find((t) => t.value === value);
  return found ? found.label : value;
}
