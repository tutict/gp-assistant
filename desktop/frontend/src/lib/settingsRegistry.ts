import type { Density } from "../hooks/useDensity";
import type { FontScale } from "../hooks/useFontScale";
import type { Theme } from "../hooks/useTheme";

export type SettingValue = string | boolean;
export type SettingType = "segmented" | "toggle" | "select";

export interface SettingOption {
  value: string;
  label: string;
}

export interface SettingDescriptor {
  key: string;
  title: string;
  description?: string;
  type: SettingType;
  options?: readonly SettingOption[];
  get: () => SettingValue;
  set: (value: SettingValue) => void;
  disabled?: boolean;
  badge?: string;
}

interface SettingsRegistryState {
  density: Density;
  setDensity: (density: Density) => void;
  theme: Theme;
  setTheme: (theme: Theme) => void;
  fontScale: FontScale;
  setFontScale: (fontScale: FontScale) => void;
}

export function createSettingsRegistry({
  density,
  setDensity,
  theme,
  setTheme,
  fontScale,
  setFontScale,
}: SettingsRegistryState): SettingDescriptor[] {
  return [
    {
      key: "density",
      title: "信息密度",
      type: "segmented",
      options: [
        { value: "comfortable", label: "舒适" },
        { value: "compact", label: "紧凑" },
      ],
      get: () => density,
      set: (value) => {
        if (value === "comfortable" || value === "compact") setDensity(value);
      },
    },
    {
      key: "theme",
      title: "主题",
      type: "segmented",
      options: [
        { value: "dark", label: "暗色" },
        { value: "light", label: "亮色" },
      ],
      get: () => theme,
      set: (value) => {
        if (value === "dark" || value === "light") setTheme(value);
      },
    },
    {
      key: "fontScale",
      title: "字体大小",
      type: "segmented",
      options: [
        { value: "small", label: "偏小" },
        { value: "standard", label: "标准" },
        { value: "large", label: "偏大" },
      ],
      get: () => fontScale,
      set: (value) => {
        if (value === "small" || value === "standard" || value === "large") setFontScale(value);
      },
    },
  ];
}
