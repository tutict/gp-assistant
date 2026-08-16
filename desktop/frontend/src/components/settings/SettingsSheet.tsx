import { X } from "lucide-react";
import type { SettingDescriptor } from "../../lib/settingsRegistry";
import { IconButton } from "../ui/IconButton";
import { Sheet } from "../ui/Sheet";

interface SettingsSheetProps {
  open: boolean;
  onClose: () => void;
  settings: readonly SettingDescriptor[];
}

function SettingControl({ setting }: { setting: SettingDescriptor }) {
  const value = setting.get();
  if (setting.type === "segmented") {
    return (
      <div className="settings-segmented" role="group" aria-label={setting.title}>
        {setting.options?.map((option) => (
          <button
            key={option.value}
            type="button"
            className={value === option.value ? "active" : ""}
            aria-pressed={value === option.value}
            aria-label={`${setting.title}：${option.label}`}
            disabled={setting.disabled}
            onClick={() => setting.set(option.value)}
          >
            {option.label}
          </button>
        ))}
      </div>
    );
  }

  if (setting.type === "toggle") {
    return (
      <button
        type="button"
        className="settings-toggle"
        role="switch"
        aria-checked={Boolean(value)}
        aria-label={setting.title}
        disabled={setting.disabled}
        onClick={() => setting.set(!value)}
      >
        <span aria-hidden="true" />
      </button>
    );
  }

  return (
    <select
      className="settings-select"
      value={String(value)}
      aria-label={setting.title}
      disabled={setting.disabled}
      onChange={(event) => setting.set(event.currentTarget.value)}
    >
      {setting.options?.map((option) => (
        <option key={option.value} value={option.value}>{option.label}</option>
      ))}
    </select>
  );
}

export function SettingsSheet({ open, onClose, settings }: SettingsSheetProps) {
  return (
    <Sheet
      open={open}
      onClose={onClose}
      label="设置"
      className="settings-sheet"
      backdropClassName="settings-sheet-backdrop"
    >
      <header className="settings-sheet-header">
        <h2>设置</h2>
        <IconButton
          label="关闭设置"
          icon={<X size={18} aria-hidden="true" />}
          onClick={onClose}
        />
      </header>
      <div className="settings-sheet-list" data-setting-count={settings.length}>
        {settings.map((setting) => (
          <section
            key={setting.key}
            className={`settings-item ${setting.disabled ? "settings-item-disabled" : ""}`.trim()}
          >
            <div className="settings-item-copy">
              <strong className="settings-item-title">{setting.title}</strong>
              {setting.description ? <span>{setting.description}</span> : null}
            </div>
            {setting.badge ? <span className="settings-item-badge">{setting.badge}</span> : null}
            <SettingControl setting={setting} />
          </section>
        ))}
      </div>
    </Sheet>
  );
}
