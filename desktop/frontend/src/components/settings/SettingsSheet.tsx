import { X } from "lucide-react";
import { IconButton } from "../ui/IconButton";
import { Sheet } from "../ui/Sheet";

interface SettingsSheetProps {
  open: boolean;
  onClose: () => void;
  settings: readonly unknown[];
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
      <div className="settings-sheet-list" data-setting-count={settings.length} />
    </Sheet>
  );
}
