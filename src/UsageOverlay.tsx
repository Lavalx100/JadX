import { Activity, Clock3, RefreshCw, X } from "lucide-react";
import { HKU_QUOTA_GROUPS, observedHkuRequests, type HkuRequestUsage } from "./hkuUsage";
import { estimateUsageCost, formatUsd, type PricingMap } from "./modelPricing";

export type UsageTotals = {
  requests: number;
  input_tokens: number | null;
  cached_input_tokens?: number | null;
  cache_miss_tokens?: number | null;
  cache_write_tokens?: number | null;
  output_tokens: number | null;
  total_tokens: number | null;
  input_metrics_reported_requests: number;
  output_metrics_reported_requests: number;
  total_metrics_reported_requests: number;
  cache_metrics_reported_requests: number;
};
export type UsageRecord = {
  id: string;
  provider_id: string;
  model: string;
  surface: string;
  usage: { input_tokens?: number | null; cached_input_tokens?: number | null; cache_miss_tokens?: number | null; cache_write_tokens?: number | null; output_tokens?: number | null; total_tokens?: number | null; ttft_ms?: number | null; total_ms?: number | null; tokens_per_second?: number | null };
  created_at: string;
};
export type UsageSummary = {
  totals: UsageTotals;
  by_model: { provider_id: string; model: string; surface: string; totals: UsageTotals }[];
  month_by_model?: { provider_id: string; model: string; surface: string; totals: UsageTotals }[];
  recent: UsageRecord[];
  hku_requests?: HkuRequestUsage[];
};

const number = (value?: number | null) => value == null ? "Not reported" : value.toLocaleString();

function surfaceName(surface: string) {
  return surface === "ide" ? "IDE" : surface === "assistant" ? "Chat assistant" : surface === "other" ? "Other" : "Unassigned";
}

function costText(providerId: string, model: string, totals: UsageTotals, pricing: PricingMap) {
  if (providerId === "local") return "Local";
  const estimate = estimateUsageCost(providerId, model, totals, pricing);
  if (!estimate) return "Rate not set";
  return `${estimate.complete ? "" : "~"}${formatUsd(estimate.amount)}`;
}

function spendInPeriod(
  items: NonNullable<UsageSummary["month_by_model"]>,
  surface?: string,
  pricing?: PricingMap,
) {
  let amount = 0;
  let unknownRates = 0;
  let partial = false;
  for (const item of items) {
    if (surface && item.surface !== surface) continue;
    if (item.provider_id === "local") continue;
    const estimate = estimateUsageCost(item.provider_id, item.model, item.totals, pricing ?? {});
    if (!estimate) unknownRates += item.totals.requests;
    else { amount += estimate.amount; partial ||= !estimate.complete; }
  }
  return { amount, unknownRates, partial };
}

function HkuQuotaMeter({ label, used, limit }: { label: string; used: number; limit: number }) {
  const percent = Math.min(100, Math.round((used / Math.max(1, limit)) * 100));
  return <div className="hku-quota-meter">
    <div><span>{label}</span><b>{used.toLocaleString()} / {limit.toLocaleString()}</b></div>
    <i><span className={percent >= 85 ? "near-limit" : ""} style={{ width: `${percent}%` }} /></i>
  </div>;
}

export default function UsageOverlay({ summary, pricing, onClose, onRefresh }: { summary: UsageSummary | null; pricing: PricingMap; onClose: () => void; onRefresh: () => void }) {
  const totals = summary?.totals;
  const month = summary?.month_by_model ?? [];
  const ideSpend = spendInPeriod(month, "ide", pricing);
  const assistantSpend = spendInPeriod(month, "assistant", pricing);
  const allSpend = spendInPeriod(month, undefined, pricing);
  const allTimeSpend = spendInPeriod(summary?.by_model ?? [], undefined, pricing);
  return <div className="modal-backdrop usage-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
    <section className="usage-modal material" role="dialog" aria-modal="true" aria-label="Model usage">
      <header className="usage-head"><div><p className="eyebrow">MODEL ACCOUNTING</p><h2>Usage & spend</h2><span>Token totals and cost estimates from successful provider responses. Prices are editable in Settings → Providers.</span></div><div className="usage-head-actions"><button className="icon-button quiet" onClick={onRefresh} aria-label="Refresh usage" title="Refresh usage"><RefreshCw size={14} /></button><button className="icon-button quiet" onClick={onClose} aria-label="Close usage"><X size={16} /></button></div></header>
      <div className="usage-spend-cards">
        <div><span>IDE · last 30 days</span><strong>{formatUsd(ideSpend.amount)}{ideSpend.partial ? " ~" : ""}</strong><small>{ideSpend.unknownRates ? `${ideSpend.unknownRates.toLocaleString()} requests lack a rate` : "Estimated provider spend"}</small></div>
        <div><span>Chat assistant · last 30 days</span><strong>{formatUsd(assistantSpend.amount)}{assistantSpend.partial ? " ~" : ""}</strong><small>{assistantSpend.unknownRates ? `${assistantSpend.unknownRates.toLocaleString()} requests lack a rate` : "Estimated provider spend"}</small></div>
        <div><span>All external models · last 30 days</span><strong>{formatUsd(allSpend.amount)}{allSpend.partial ? " ~" : ""}</strong><small>{allSpend.unknownRates ? `${allSpend.unknownRates.toLocaleString()} requests lack a rate` : "Includes other and unassigned requests"}</small></div>
        <div><span>All-time tracked</span><strong>{formatUsd(allTimeSpend.amount)}{allTimeSpend.partial ? " ~" : ""}</strong><small>{allTimeSpend.unknownRates ? `${allTimeSpend.unknownRates.toLocaleString()} requests lack a rate` : "Estimated from all recorded requests"}</small></div>
      </div>
      <p className="usage-estimate-note">Estimates use saved rates and provider-reported tokens. A “~” means input, output, or cache metrics were missing. The selected DeepSeek rate tier applies to all tracked requests; peak timing is not inferred per request. Provider discounts, taxes, and account credits can make invoices differ. Local model inference is not provider-billed.</p>
      <div className="usage-cards">
        <div><span>Input tokens</span><strong>{number(totals?.input_tokens)}</strong><small>Provider-reported input</small></div>
        <div><span>Cached input</span><strong>{number(totals?.cached_input_tokens)}</strong><small>Reported cache hits</small></div>
        <div><span>Cache misses</span><strong>{number(totals?.cache_miss_tokens)}</strong><small>{totals?.cache_metrics_reported_requests ? `Reported for ${totals.cache_metrics_reported_requests} of ${totals.requests} requests` : "Provider did not report cache metrics"}</small></div>
        <div><span>Cache writes</span><strong>{number(totals?.cache_write_tokens)}</strong><small>Input tokens written to provider cache</small></div>
        <div><span>Output tokens</span><strong>{number(totals?.output_tokens)}</strong><small>Generated output</small></div>
      </div>
      <div className="usage-section"><div className="usage-section-title"><h3>By model</h3><span>{totals?.requests ?? 0} requests · {number(totals?.total_tokens)} total tokens</span></div>
        {summary?.by_model.length ? <div className="usage-table">{summary.by_model.map((item) => <div className="usage-row" key={`${item.surface}:${item.provider_id}:${item.model}`}><div><strong>{item.model}</strong><small>{surfaceName(item.surface)} · {item.provider_id} · {item.totals.requests} requests</small></div><span><b>{costText(item.provider_id, item.model, item.totals, pricing)}</b><small>estimated cost</small></span><span><b>{number(item.totals.input_tokens)}</b><small>input</small></span><span><b>{number(item.totals.cached_input_tokens)}</b><small>cached</small></span><span><b>{number(item.totals.output_tokens)}</b><small>output</small></span></div>)}</div> : <p className="usage-empty"><Activity size={17} /> Usage appears here after a provider returns token counts.</p>}
      </div>
      <div className="usage-section hku-limits-section"><div className="usage-section-title"><div><h3>HKU request limits</h3><span>Published quotas with JadX-observed attempts, including failed calls</span></div><span>Not an account-wide meter</span></div>
        <p className="hku-quota-note">HKU’s published limits are shown alongside requests JadX has attempted in rolling windows. This local count starts when request tracking is enabled and does not include calls made in the HKU portal or other apps. HKU may enforce additional account-level limits.</p>
        <div className="hku-quota-groups">{HKU_QUOTA_GROUPS.map((group) => <details className="hku-quota-group" key={group.id} open={group.id === "claude"}>
          <summary><strong>{group.label}</strong><span>{group.rules.length} published limit{group.rules.length === 1 ? "" : "s"}</span></summary>
          <div className="hku-quota-list">{group.rules.map((rule) => {
            const usage = observedHkuRequests(rule, summary?.hku_requests);
            return <article className="hku-quota-rule" key={rule.id}>
              <strong>{rule.label}</strong>
              <div className="hku-quota-meters">
                {rule.per_minute != null && <HkuQuotaMeter label="Last 60 sec" used={usage.last_minute} limit={rule.per_minute} />}
                {rule.per_day != null && <HkuQuotaMeter label="Rolling 24 hours" used={usage.last_day} limit={rule.per_day} />}
                {rule.per_week != null && <HkuQuotaMeter label="Rolling 7 days" used={usage.last_week} limit={rule.per_week} />}
              </div>
              {usage.failed_last_day > 0 && <small className="hku-quota-failures">{usage.failed_last_day} failed attempt{usage.failed_last_day === 1 ? "" : "s"} in the last 24 hours are included.</small>}
            </article>;
          })}</div>
        </details>)}</div>
      </div>
      <div className="usage-section usage-recent"><div className="usage-section-title"><h3>Recent requests</h3><Clock3 size={14} /></div>
        {summary?.recent.length ? <div className="usage-recent-list">{summary.recent.map((entry) => <div className="usage-recent-row" key={entry.id}><div><strong>{entry.model}</strong><small>{surfaceName(entry.surface)} · {entry.provider_id} · {new Date(entry.created_at).toLocaleString()}</small></div><span>{costText(entry.provider_id, entry.model, { requests: 1, input_tokens: entry.usage.input_tokens ?? null, cached_input_tokens: entry.usage.cached_input_tokens ?? null, cache_miss_tokens: entry.usage.cache_miss_tokens ?? null, output_tokens: entry.usage.output_tokens ?? null, total_tokens: entry.usage.total_tokens ?? null, input_metrics_reported_requests: entry.usage.input_tokens == null ? 0 : 1, output_metrics_reported_requests: entry.usage.output_tokens == null ? 0 : 1, total_metrics_reported_requests: 0, cache_metrics_reported_requests: entry.usage.cached_input_tokens == null && entry.usage.cache_miss_tokens == null && entry.usage.cache_write_tokens == null ? 0 : 1 }, pricing)}</span><span>{number(entry.usage.input_tokens)} in</span><span>{number(entry.usage.output_tokens)} out</span><span>{entry.usage.total_ms == null ? "—" : `${(entry.usage.total_ms / 1000).toFixed(1)}s`}</span></div>)}</div> : null}
      </div>
      <footer className="usage-footer">Cache accounting is provider-dependent; unavailable values are intentionally shown as “Not reported.” Historical records created before surface tracking are marked unassigned. Estimates do not claim to reproduce the provider invoice.</footer>
    </section>
  </div>;
}
