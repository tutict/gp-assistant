import type { Density } from "../hooks/useDensity";
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
}

export function createSettingsRegistry({
  density,
  setDensity,
  theme,
  setTheme,
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
  ];
}
