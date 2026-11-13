import { Activity, ChevronDown, FileSearch, ShieldCheck, X } from "lucide-react";
import { lazy, Suspense, useId, useState } from "react";
import type { ReactNode, SyntheticEvent } from "react";
import type { LlmSettings } from "../../types";
import type { SettingDescriptor } from "../../lib/settingsRegistry";
import { LlmSettingsPanel } from "../panels/LlmSettingsPanel";
import { IconButton } from "../ui/IconButton";
import { Sheet } from "../ui/Sheet";
import { BackupPanel } from "./BackupPanel";
import { ReliabilityPanel } from "./ReliabilityPanel";
import { NetworkProxyPanel } from "./NetworkProxyPanel";
import "../../styles/stock-research.css";

const LazyStockResearchPanel = lazy(() => import("./StockResearchPanel").then((module) => ({ default: module.StockResearchPanel })));

function SettingsDisclosure({ title, description, icon, children, onToggle }: {
  title: string;
  description: string;
  icon: ReactNode;
  children: ReactNode;
  onToggle?: (event: SyntheticEvent<HTMLDetailsElement>) => void;
}) {
  return (
    <details className="settings-disclosure" onToggle={onToggle}>
      <summary>
        <span className="settings-disclosure-icon" aria-hidden="true">{icon}</span>
        <span className="settings-disclosure-copy"><strong>{title}</strong><span>{description}</span></span>
        <ChevronDown className="settings-disclosure-chevron" size={16} aria-hidden="true" />
      </summary>
      <div className="settings-disclosure-body">{children}</div>
    </details>
  );
}

function StockResearchSettings() {
  const [expanded, setExpanded] = useState(false);
  return <SettingsDisclosure title="选股研究与验证" description="查看数据覆盖与验证报告" icon={<FileSearch size={18} />}
    onToggle={(event) => setExpanded(event.currentTarget.open)}>
    {expanded ? <Suspense fallback={<p role="status">加载研究记录…</p>}><LazyStockResearchPanel /></Suspense> : null}
  </SettingsDisclosure>;
}

interface SettingsSheetProps {
  open: boolean;
  onClose: () => void;
  settings: readonly SettingDescriptor[];
  llmSettings?: LlmSettings | null;
  onLlmSettingsChange?: import("../../lib/llmCredentialStore").LlmSettingsSetter;
}

function SettingControl({ setting, descriptionId }: { setting: SettingDescriptor; descriptionId?: string }) {
  const value = setting.get();
  if (setting.type === "segmented") {
    return (
      <div className="settings-segmented" role="group" aria-label={setting.title} aria-describedby={descriptionId}>
        {setting.options?.map((option) => (
          <button
            key={option.value}
            type="button"
            className={value === option.value ? "active" : ""}
            aria-pressed={value === option.value}
            aria-label={`${setting.title}：${option.label}`}
            disabled={setting.disabled}
            onClick={setting.disabled ? undefined : () => setting.set(option.value)}
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
        aria-describedby={descriptionId}
        disabled={setting.disabled}
        onClick={setting.disabled ? undefined : () => setting.set(!value)}
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
      aria-describedby={descriptionId}
      disabled={setting.disabled}
      onChange={setting.disabled ? undefined : (event) => setting.set(event.currentTarget.value)}
    >
      {setting.options?.map((option) => (
        <option key={option.value} value={option.value}>{option.label}</option>
      ))}
    </select>
  );
}

function SettingItem({ setting }: { setting: SettingDescriptor }) {
  const descriptionId = useId();
  return (
    <section className={`settings-item ${setting.disabled ? "settings-item-disabled" : ""}`.trim()}>
      <div className="settings-item-copy">
        <strong className="settings-item-title">{setting.title}</strong>
        {setting.description ? <span id={descriptionId}>{setting.description}</span> : null}
      </div>
      {setting.badge ? <span className="settings-item-badge">{setting.badge}</span> : null}
      <SettingControl setting={setting} descriptionId={setting.description ? descriptionId : undefined} />
    </section>
  );
}

export function SettingsSheet({ open, onClose, settings, llmSettings, onLlmSettingsChange }: SettingsSheetProps) {
  const groupId = useId();
  const displaySettings = settings.filter((setting) => setting.key !== "market");
  const marketSettings = settings.filter((setting) => setting.key === "market");
  return (
    <Sheet
      open={open}
      onClose={onClose}
      label="设置"
      className="settings-sheet"
      backdropClassName="settings-sheet-backdrop"
    >
      <header className="settings-sheet-header">
        <div><h2>设置</h2><p>界面偏好、市场与本机工具</p></div>
        <IconButton
          label="关闭设置"
          icon={<X size={18} aria-hidden="true" />}
          onClick={onClose}
        />
      </header>
      <div className="settings-sheet-list" data-setting-count={settings.length}>
        {displaySettings.length > 0 ? (
          <section className="settings-group" aria-labelledby={`${groupId}-display`}>
            <h3 className="settings-group-heading" id={`${groupId}-display`}>显示偏好</h3>
            {displaySettings.map((setting) => <SettingItem key={setting.key} setting={setting} />)}
          </section>
        ) : null}
        {marketSettings.length > 0 || onLlmSettingsChange ? (
          <section className="settings-group" aria-labelledby={`${groupId}-workspace`}>
            <h3 className="settings-group-heading" id={`${groupId}-workspace`}>市场与模型</h3>
            {marketSettings.map((setting) => <SettingItem key={setting.key} setting={setting} />)}
            {onLlmSettingsChange ? (
              <div className="settings-model-entry">
                <LlmSettingsPanel settings={llmSettings ?? null} onChange={onLlmSettingsChange} presentation="dialog" />
              </div>
            ) : null}
          </section>
        ) : null}
        <section className="settings-group" aria-labelledby={`${groupId}-tools`}>
          <h3 className="settings-group-heading" id={`${groupId}-tools`}>本机工具</h3>
          <SettingsDisclosure title="网络代理" description="配置并测试港股免费数据源连接" icon={<Activity size={18} />}>
            <NetworkProxyPanel />
          </SettingsDisclosure>
          <SettingsDisclosure title="加密备份" description="备份与恢复本机研究数据" icon={<ShieldCheck size={18} />}>
            <BackupPanel />
          </SettingsDisclosure>
          <SettingsDisclosure title="本地诊断" description="运行记录与 GEPA 实验设置" icon={<Activity size={18} />}>
            <ReliabilityPanel onGepaPreferenceChange={() => window.dispatchEvent?.(new Event("gp:reliability-settings"))} />
          </SettingsDisclosure>
          <StockResearchSettings />
        </section>
      </div>
    </Sheet>
  );
}
