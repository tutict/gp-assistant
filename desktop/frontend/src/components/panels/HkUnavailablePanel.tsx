export function HkUnavailablePanel({ feature }: { feature: string }) {
  return (
    <section className="panel-container hk-unavailable-panel" aria-label={`${feature}暂不可用`}>
      <div className="panel-feedback panel-feedback-empty">
        <div>
          <strong>{feature}暂未开放</strong>
          <p>港股 MVP 当前提供证券搜索、报价、日 K 和自选。请切换到 A 股使用这项功能。</p>
        </div>
      </div>
    </section>
  );
}
