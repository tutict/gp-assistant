import { Component, type ErrorInfo, type ReactNode } from "react";
import { useWorkspaceStore } from "../../hooks/useWorkspace";
interface Props { children: ReactNode; label?: string; resetKey?: string; beforeReload?: () => Promise<boolean>; reload?: () => void; }
interface State { failed: boolean; reloadBlocked: boolean; }
/** Render/lifecycle isolation only; async storage failures are handled by the workspace store. */
export class ErrorBoundary extends Component<Props, State> {
  state: State = { failed: false, reloadBlocked: false };
  static getDerivedStateFromError(): State { return { failed: true, reloadBlocked: false }; }
  componentDidCatch(_error: Error, _info: ErrorInfo) { /* Never log draft/credential-bearing component state. */ }
  componentDidUpdate(previous: Props) { if (previous.resetKey !== this.props.resetKey && this.state.failed) this.setState({ failed: false, reloadBlocked: false }); }
  private reload = async () => {
    try {
      if (this.props.beforeReload && !await this.props.beforeReload()) { this.setState({ reloadBlocked: true }); return; }
      (this.props.reload ?? (() => window.location.reload()))();
    } catch { this.setState({ reloadBlocked: true }); }
  };
  render() {
    if (!this.state.failed) return this.props.children;
    return <section role="alert" className="panel-feedback">
      <h2>{this.props.label ?? "工作区"}暂时无法显示</h2>
      <p>本地数据不会被清除。可以重试当前区域，或保存草稿后重新加载。</p>
      <button type="button" onClick={() => this.setState({ failed: false, reloadBlocked: false })}>重试显示</button>
      <button type="button" onClick={() => void this.reload()}>保存并重新加载</button>
      {this.state.reloadBlocked && <p>草稿尚未保存，已阻止重新加载。请先重试保存，避免丢失当前编辑。</p>}
    </section>;
  }
}
export function WorkspaceErrorBoundary(props: Omit<Props, "beforeReload">) {
  const store = useWorkspaceStore();
  return <ErrorBoundary {...props} beforeReload={async () => { await store.flush(); return store.getStatus().state === "saved"; }} />;
}
