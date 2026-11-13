import type { ReactNode } from "react";
import { AlertCircle, AlertTriangle, CheckCircle2, Inbox, LoaderCircle } from "lucide-react";

type PanelFeedbackKind = "empty" | "error" | "loading" | "warning" | "success";

interface PanelFeedbackProps {
  kind: PanelFeedbackKind;
  title?: string;
  description: string;
  action?: ReactNode;
  compact?: boolean;
}

const FEEDBACK_ICONS = {
  empty: Inbox,
  error: AlertCircle,
  warning: AlertTriangle,
  success: CheckCircle2,
  loading: LoaderCircle,
} as const;

export function PanelFeedback({ kind, title, description, action, compact = false }: PanelFeedbackProps) {
  if (kind === "loading" && !compact) {
    return (
      <div
        className="panel-feedback panel-feedback-loading panel-feedback-skeleton"
        role="status"
        aria-live="polite"
      >
        <div className="skeleton-layout" aria-hidden="true">
          <span className="skeleton skeleton-line skeleton-line-title" />
          <span className="skeleton skeleton-line" />
          <span className="skeleton skeleton-line skeleton-line-short" />
          <div className="skeleton-metrics">
            <span className="skeleton" />
            <span className="skeleton" />
            <span className="skeleton" />
          </div>
        </div>
        <p>{description}</p>
      </div>
    );
  }

  const Icon = FEEDBACK_ICONS[kind];
  return (
    <div
      className={`panel-feedback panel-feedback-${kind}${compact ? " panel-feedback-compact" : ""}`}
      role={kind === "error" ? "alert" : "status"}
      aria-live={kind === "error" ? "assertive" : "polite"}
    >
      <Icon size={20} aria-hidden="true" />
      <div>
        {title ? <strong>{title}</strong> : null}
        <p>{description}</p>
      </div>
      {action ? <div className="panel-feedback-action">{action}</div> : null}
    </div>
  );
}
