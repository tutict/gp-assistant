interface Props {
  action: string;
  loading: boolean;
  onRun: () => void;
  loadingLabel?: string;
  cacheRunLabel?: string;
  buttonClassName?: string;
  ariaLabel?: string;
}

export function LocalFirstRunControl({ action, loading, onRun, loadingLabel, cacheRunLabel, buttonClassName = "run-btn", ariaLabel }: Props) {
  const label = loading ? loadingLabel || `${action}中...` : cacheRunLabel || `运行${action}`;
  return <button type="button" className={buttonClassName} onClick={onRun} disabled={loading} aria-disabled={loading} aria-label={ariaLabel}>{label}</button>;
}
