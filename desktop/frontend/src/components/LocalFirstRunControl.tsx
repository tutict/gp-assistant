interface Props {
  action: string;
  loading: boolean;
  onRun: () => void;
  loadingLabel?: string;
  cacheRunLabel?: string;
  buttonClassName?: string;
  ariaLabel?: string;
  disabled?: boolean;
  ariaDescribedBy?: string;
}

export function LocalFirstRunControl({ action, loading, onRun, loadingLabel, cacheRunLabel, buttonClassName = "run-btn", ariaLabel, ariaDescribedBy, disabled = false }: Props) {
  const label = loading ? loadingLabel || `${action}中...` : cacheRunLabel || `运行${action}`;
  const unavailable = loading || disabled;
  return <button type="button" className={buttonClassName} onClick={onRun} disabled={unavailable} aria-disabled={unavailable} aria-label={ariaLabel} aria-describedby={ariaDescribedBy}>{label}</button>;
}
