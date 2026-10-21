import { describe, it, expect } from "vitest";
import { THEMES, DEFAULT_THEME, resolveTheme, themeLabel } from "../../src/shared/ui/themes";

describe("themes", () => {
  it("offers the themes the build carries", () => {
    expect(THEMES.map((t) => t.value)).toEqual([
      "Modern",
      "Indigo",
      "IndigoDark",
      "TangibleLight",
      "TangibleDark",
      "Simple",
      "Classic",
    ]);
  });

  it("starts on the default when nothing was remembered", () => {
    expect(resolveTheme(null)).toBe(DEFAULT_THEME);
    expect(resolveTheme(undefined)).toBe(DEFAULT_THEME);
  });

  it("starts on the remembered theme when it is one we have", () => {
    expect(resolveTheme("Indigo")).toBe("Indigo");
  });

  it("falls back to the default for a theme that is not in the build", () => {
    expect(resolveTheme("NoSuchTheme")).toBe(DEFAULT_THEME);
  });

  it("names each theme for the menu", () => {
    expect(themeLabel("IndigoDark")).toBe("Indigo dark");
    expect(themeLabel("Mystery")).toBe("Mystery");
  });
});
