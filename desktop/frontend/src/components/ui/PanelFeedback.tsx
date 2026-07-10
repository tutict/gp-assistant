import type { ReactNode } from "react";
import { AlertCircle, Inbox, LoaderCircle } from "lucide-react";

type PanelFeedbackKind = "empty" | "error" | "loading";

interface PanelFeedbackProps {
  kind: PanelFeedbackKind;
  title?: string;
  description: string;
  action?: ReactNode;
}

const FEEDBACK_ICONS = {
  empty: Inbox,
  error: AlertCircle,
  loading: LoaderCircle,
} as const;

export function PanelFeedback({ kind, title, description, action }: PanelFeedbackProps) {
  const Icon = FEEDBACK_ICONS[kind];
  return (
    <div
      className={`panel-feedback panel-feedback-${kind}`}
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
