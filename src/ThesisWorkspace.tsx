import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { assessThesis, executionEconomics, INVESTOR_FRAMEWORK, INVESTOR_RULES, quoteUsable, safeSourceUrl, scenarioValue, thesisAction, thesisFor, thesisReviewContext, validateEvidence } from "./investmentThesis";
import type { ThesisEvidence, ThesisPosition, ThesisQuote } from "./investmentThesis";

type Research = {
  symbol: string; name: string; financials: { label: string; value: string; period?: string | null; source: string }[];
  news: { title: string; detail: string; url?: string | null; published_at?: string | null; source: string }[];
  filings: { title: string; url?: string | null; source: string }[];
  provenance: { freshness: string; retrieved_at: string; provider: string };
};
type Props = { initialSymbol: string; evidence: ThesisEvidence[]; onEvidenceChange: (entries: ThesisEvidence[]) => boolean; ledger: ThesisPosition[]; ledgerReady: boolean; onOpenCompany: (symbol: string) => void };
const money = (value: number | null) => value === null ? "—" : new Intl.NumberFormat("en-HK", { style: "currency", currency: "HKD" }).format(value);
const hkDate = () => new Date().toLocaleDateString("en-CA", { timeZone: "Asia/Hong_Kong" });
const initialForm = () => ({ criterionId: "", direction: "supports" as ThesisEvidence["direction"], kind: "filing" as ThesisEvidence["kind"], stage: "realized" as ThesisEvidence["stage"], date: hkDate(), source: "", url: "", detail: "" });

export function ThesisSummary({ symbol, evidence, quote, onOpen }: { symbol: string; evidence: ThesisEvidence[]; quote?: ThesisQuote | null; onOpen: () => void }) {
  const assessment = assessThesis(symbol, evidence, quote);
  if (!assessment.profile) return null;
  const action = thesisAction(assessment);
  return <section className="market-panel thesis-summary">
    <div className="market-panel-heading"><div><p className="eyebrow">YOUR INVESTMENT THESIS</p><h3>{assessment.profile.name} · {assessment.status}</h3></div><button className="secondary-button" onClick={onOpen}>Check my thesis</button></div>
    <p>{assessment.profile.thesis}</p><p className="thesis-muted">{assessment.supported}/{assessment.checks.length} tests supported by recorded primary evidence · {action.action}: {action.reason}</p>
    <p className="thesis-muted">{assessment.profile.obsoleteAnchor}</p>
  </section>;
}

export default function InvestmentThesis({ initialSymbol, evidence, onEvidenceChange, ledger, ledgerReady, onOpenCompany }: Props) {
  const [symbol, setSymbol] = useState(initialSymbol || "EXAMPLE");
  const [symbolInput, setSymbolInput] = useState(initialSymbol || "EXAMPLE");
  const [quote, setQuote] = useState<ThesisQuote | null>(null);
  const [research, setResearch] = useState<Research | null>(null);
  const [refreshId, setRefreshId] = useState(0);
  const [loading, setLoading] = useState(false);
  const [fetchError, setFetchError] = useState("");
  const [form, setForm] = useState(initialForm);
  const [formError, setFormError] = useState("");
  const [editingId, setEditingId] = useState<string | null>(null);
  const [answer, setAnswer] = useState("");
  const [reviewing, setReviewing] = useState(false);
  const [reviewError, setReviewError] = useState("");
  const [model, setModel] = useState("");
  const [scenarios, setScenarios] = useState({ bear: { eps: "", multiple: "" }, base: { eps: "", multiple: "" }, bull: { eps: "", multiple: "" }, source: "" });
  const [execution, setExecution] = useState({ entry: "", quantity: "1", buyFees: "", sellFees: "", capital: "", source: "" });
  const [entryWasEdited, setEntryWasEdited] = useState(false);
  const [reviewRevision, setReviewRevision] = useState("");
  const reviewVersion = useRef(0);
  const profile = thesisFor(symbol)!;
  const currentQuote = quote?.symbol === symbol ? quote : null;
  const currentResearch = research?.symbol === symbol ? research : null;
  const assessment = assessThesis(symbol, evidence, currentQuote);
  const action = thesisAction(assessment);
  const entries = evidence.filter((entry) => entry.symbol === symbol).slice().sort((a, b) => b.date.localeCompare(a.date));
  const revision = JSON.stringify({ symbol, evidence: entries, quote: currentQuote, research: currentResearch, scenarios, execution, ledger });
  const staleReview = Boolean(answer && reviewRevision !== revision);
  const scenarioTargets = Object.fromEntries((["bear", "base", "bull"] as const).map((name) => [name, scenarioValue({ eps: Number(scenarios[name].eps), multiple: Number(scenarios[name].multiple) })]));
  const scenarioOrdered = scenarioTargets.bear !== null && scenarioTargets.base !== null && scenarioTargets.bull !== null && scenarioTargets.bear <= scenarioTargets.base && scenarioTargets.base <= scenarioTargets.bull;
  const economics = (Object.keys(scenarioTargets) as ("bear" | "base" | "bull")[]).map((name) => ({ name, value: scenarioTargets[name], result: execution.entry && execution.buyFees && execution.sellFees && execution.source.trim() && scenarios.source.trim() && scenarioTargets[name] !== null ? executionEconomics({ entry: Number(execution.entry), exit: scenarioTargets[name]!, quantity: Number(execution.quantity), buyFees: Number(execution.buyFees), sellFees: Number(execution.sellFees), capital: Number(execution.capital) }) : null }));
  const currentLedger = useMemo(() => {
    const rows = ledger.filter((row) => row.symbol === profile.symbol);
    return [{ profile, ledgerShares: rows.reduce((sum, row) => sum + row.shares, 0), accounts: rows.map((row) => row.account + " (" + row.origin + ")").join(", "), conflicts: rows.flatMap((row) => row.mismatch ? [row.mismatch] : []) }];
  }, [ledger, profile]);

  useEffect(() => {
    let cancelled = false;
    setLoading(true); setFetchError(""); setQuote(null); setResearch(null);
    void Promise.allSettled([
      invoke<ThesisQuote>("markets_quote", { symbol }),
      invoke<Research>("markets_company_research", { symbol }),
    ]).then(([quoteResult, researchResult]) => {
      if (cancelled) return;
      if (quoteResult.status === "fulfilled") setQuote(quoteResult.value);
      if (researchResult.status === "fulfilled") setResearch(researchResult.value);
      setFetchError([quoteResult, researchResult].filter((result) => result.status === "rejected").map((result) => String((result as PromiseRejectedResult).reason)).join(" · "));
    }).finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [symbol, refreshId]);

  useEffect(() => () => { reviewVersion.current += 1; }, []);

  function chooseCompany(value: string) {
    reviewVersion.current += 1; setReviewing(false);
    setSymbol(value); setSymbolInput(value); setForm(initialForm()); setEditingId(null); setFormError(""); setAnswer(""); setReviewError("");
    setScenarios({ bear: { eps: "", multiple: "" }, base: { eps: "", multiple: "" }, bull: { eps: "", multiple: "" }, source: "" });
    setExecution({ entry: "", quantity: "1", buyFees: "", sellFees: "", capital: "", source: "" }); setEntryWasEdited(false);
  }

  function saveEvidence() {
    const entry: ThesisEvidence = { ...form, criterionId: form.criterionId || profile.criteria[0].id, id: editingId ?? crypto.randomUUID(), symbol };
    const error = validateEvidence(entry);
    if (error) { setFormError(error); return; }
    if (!editingId && evidence.length >= 500) { setFormError("The journal has 500 findings. Remove or consolidate older findings before adding another."); return; }
    if (!onEvidenceChange([...evidence.filter((item) => item.id !== entry.id), entry])) return;
    setForm(initialForm()); setEditingId(null); setFormError("");
  }

  async function review() {
    const version = ++reviewVersion.current;
    setReviewing(true); setReviewError(""); setAnswer("");
    const context = JSON.stringify({ investment: thesisReviewContext(symbol, evidence, currentQuote), quote: currentQuote, research: currentResearch, valuationAssumptions: scenarios, scenarioTargets, executionInputs: execution, economics, ledgerComparison: currentLedger, instructions: "Assess each thesis test using cited supplied evidence. News is only a lead unless underlying facts are supplied. Explicitly challenge the original thesis, identify missing data and explain hold/add/trim/exit/re-entry conditions. User-entered filing links have not been independently read. Historical user assertions remain unverified. Do not call any scenario target fair value without defensible inputs." });
    try {
      const response = await invoke<{ answer: string; model: string }>("markets_qwen_review", { prompt: `Check my ${profile.name} investment thesis as of today. Give: thesis status; strongest support; strongest counterargument; valuation sensitivity; costs and portfolio fit; next evidence and action conditions. Cite source and date for each factual assertion, or label it unknown.`, context });
      if (version !== reviewVersion.current) return;
      setAnswer(response.answer); setModel(response.model); setReviewRevision(revision);
    } catch (cause) { if (version === reviewVersion.current) setReviewError(String(cause)); }
    finally { if (version === reviewVersion.current) setReviewing(false); }
  }

  return <div className="markets-page thesis-workspace">
    <div className="markets-page-heading"><div><p className="eyebrow">INVESTMENT RESEARCH</p><h2>Check my thesis</h2><p className="muted-copy">Test what must be true, what would break the thesis, and what the price already assumes.</p></div><button className="secondary-button" onClick={() => setRefreshId((value) => value + 1)} disabled={loading}>{loading ? "Refreshing sources…" : "Refresh quote & research"}</button></div>
    <section className="market-panel"><p className="eyebrow">YOUR FRAMEWORK</p><h3>{INVESTOR_FRAMEWORK}</h3><p className="thesis-muted">No portfolio snapshot or company-specific research is bundled. Add your own records locally.</p><details><summary>Investment preferences and source discipline</summary><ul>{INVESTOR_RULES.map((rule) => <li key={rule}>{rule}</li>)}</ul></details></section>
    <form className="thesis-company-picker" aria-label="Choose a thesis" onSubmit={(event) => { event.preventDefault(); const next = symbolInput.trim().toUpperCase(); if (next) chooseCompany(next); }}>
      <label htmlFor="thesis-symbol">Ticker or symbol</label>
      <input id="thesis-symbol" value={symbolInput} onChange={(event) => setSymbolInput(event.target.value)} placeholder="Enter a symbol" />
      <button className="secondary-button" type="submit" disabled={!symbolInput.trim()}>Open</button>
    </form>
    <section className="market-panel thesis-company">
      <div className="market-panel-heading"><div><p className="eyebrow">{symbol} · THESIS STATUS</p><h3>{profile.name} · {assessment.status}</h3></div><button className="text-button" onClick={() => onOpenCompany(symbol)}>Open company analysis →</button></div>
      <p>{profile.thesis}</p><p className="thesis-muted">{profile.position}</p>
      <div className="thesis-quote"><strong>{money(currentQuote?.last ?? null)}</strong><span>{currentQuote?.provenance.freshness ?? "UNAVAILABLE"} · {currentQuote?.provenance.provider ?? "No quote"}</span><span>Observed: {currentQuote?.provenance.observed_at ? new Date(currentQuote.provenance.observed_at).toLocaleString("en-HK", { timeZone: "Asia/Hong_Kong" }) : "Unknown"} HKT</span></div>
      {!assessment.currentQuote && <p className="thesis-attention">A current timestamped quote is still needed. No historical snapshot is included.</p>}
      {fetchError && <p className="thesis-attention">Source refresh: {fetchError}</p>}
      <p className="thesis-attention">{profile.obsoleteAnchor}</p>
      <details><summary>Historical context</summary><p className="thesis-muted">{profile.historicalContext}</p></details>
    </section>
    <section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">FALSIFIABLE TESTS</p><h3>{assessment.supported}/{assessment.checks.length} supported by recorded primary evidence</h3></div><b>{action.action}</b></div><p className="thesis-muted">{action.reason} Filing/regulatory findings you record count as primary evidence; the app has not independently verified their contents. Samples, design wins, expert estimates, headlines and proposals remain leads. Evidence older than 120 days needs review.</p><div className="thesis-checks">{assessment.checks.map((check) => <article key={check.id}><div><strong>{check.label}{check.critical ? " · critical" : ""}</strong><b className={check.status === "SUPPORTED" ? "market-positive" : check.status === "CHALLENGED" || check.status === "MIXED" ? "market-negative" : ""}>{check.status}</b></div><p>{check.test}</p><small>{check.entries.length} recorded finding(s) · {check.currentCount} recent primary finding(s)</small></article>)}</div></section>
    <section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">EVIDENCE JOURNAL</p><h3>{editingId ? "Edit evidence" : "Record a finding"}</h3></div></div><div className="thesis-form">
      <label>Thesis test<select value={form.criterionId || profile.criteria[0].id} onChange={(event) => setForm({ ...form, criterionId: event.target.value })}>{profile.criteria.map((item) => <option key={item.id} value={item.id}>{item.label}</option>)}</select></label>
      <label>Effect<select value={form.direction} onChange={(event) => setForm({ ...form, direction: event.target.value as ThesisEvidence["direction"] })}><option value="supports">Supports the thesis</option><option value="challenges">Challenges the thesis</option></select></label>
      <label>Source type<select value={form.kind} onChange={(event) => setForm({ ...form, kind: event.target.value as ThesisEvidence["kind"] })}><option value="filing">Company filing / results</option><option value="regulatory">Official regulatory document</option><option value="expert">Expert estimate</option><option value="news">News / headline</option><option value="observation">Personal observation</option></select></label>
      <label>Evidence stage<select value={form.stage} onChange={(event) => setForm({ ...form, stage: event.target.value as ThesisEvidence["stage"] })}><option value="realized">Reported actual result / effective rule</option><option value="production">Production / shipments</option><option value="design-win">Design win / future award</option><option value="sample">Sample / evaluation / demo</option><option value="proposal">Proposed rule / plan</option><option value="commentary">Commentary / forecast</option></select></label>
      <label>Publication date<input type="date" value={form.date} max={hkDate()} onChange={(event) => setForm({ ...form, date: event.target.value })} /></label>
      <label>Source name<input value={form.source} onChange={(event) => setForm({ ...form, source: event.target.value })} placeholder="Issuer interim report, page 12" /></label>
      <label className="thesis-wide">Source link<input value={form.url} onChange={(event) => setForm({ ...form, url: event.target.value })} placeholder="https://… exact document, not a search page" /></label>
      <label className="thesis-wide">Specific finding and limitations<textarea rows={3} value={form.detail} onChange={(event) => setForm({ ...form, detail: event.target.value })} placeholder="What changed? Include the reported period, units, comparison and what remains unknown." /></label>
    </div>{formError && <p role="alert" className="thesis-attention">{formError}</p>}<div className="thesis-actions"><button className="primary-button" onClick={saveEvidence}>{editingId ? "Save evidence changes" : "Save finding locally"}</button>{editingId && <button className="secondary-button" onClick={() => { setEditingId(null); setForm(initialForm()); }}>Cancel edit</button>}</div>
      <div className="thesis-evidence">{entries.length ? entries.map((entry) => <article key={entry.id}><div><strong>{entry.direction === "supports" ? "Support" : "Counterevidence"} · {profile.criteria.find((item) => item.id === entry.criterionId)?.label}</strong><span>{entry.date} · {entry.source} · {entry.stage}</span></div><p>{entry.detail}</p><div className="thesis-actions">{safeSourceUrl(entry.url) && <a href={safeSourceUrl(entry.url)!} target="_blank" rel="noreferrer">Open source</a>}<button className="text-button" onClick={() => { const { id, symbol: _symbol, ...fields } = entry; setForm(fields); setEditingId(id); setFormError(""); }}>Edit</button><button className="text-button" onClick={() => onEvidenceChange(evidence.filter((item) => item.id !== entry.id))}>Remove finding</button></div></article>) : <p className="thesis-muted">No current evidence recorded. Historical context and price momentum do not automatically pass these tests.</p>}</div>
    </section>
    <section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">NORMALIZED VALUATION</p><h3>What would earnings need to justify?</h3></div></div><p className="thesis-muted">Enter normalized earnings per H-share in HKD and a defensible P/E for each scenario. Target = EPS × multiple. Convert reporting currency and use the correct share count first. For a loss-making business or ETF, use a separate revenue/cash-flow or NAV model; do not manufacture positive EPS.</p>
      <label className="thesis-field">Valuation source, horizon and assumptions<input value={scenarios.source} onChange={(event) => setScenarios({ ...scenarios, source: event.target.value })} placeholder="2027 normalized margins, FX, share count, peer multiple and source/date" /></label>
      <div className="thesis-scenarios">{(["bear", "base", "bull"] as const).map((name) => <article key={name}><strong>{name.toUpperCase()}</strong><label>Normalized EPS · HKD<input aria-label={`${name} normalized EPS`} type="number" step="any" value={scenarios[name].eps} onChange={(event) => setScenarios({ ...scenarios, [name]: { ...scenarios[name], eps: event.target.value } })} /></label><label>P/E multiple<input aria-label={`${name} multiple`} type="number" step="any" value={scenarios[name].multiple} onChange={(event) => setScenarios({ ...scenarios, [name]: { ...scenarios[name], multiple: event.target.value } })} /></label><b>{money(scenarioTargets[name])}</b></article>)}</div>
      {Object.values(scenarioTargets).every((value) => value !== null) && !scenarioOrdered && <p className="thesis-attention">Bear, base and bull targets are not ordered; review your assumptions before interpreting the range.</p>}
      <h3 className="thesis-section-title">Execution and position size</h3><p className="thesis-muted">Enter the actual odd-lot offer and all buy/sell costs. Leaving a fee blank leaves the net calculation incomplete; enter 0 only when confirmed free. Scenario proceeds assume sale at that scenario price; they are not current executable bids.</p>
      <div className="thesis-form"><label>Executable entry · HKD<input type="number" step="any" value={execution.entry} onChange={(event) => { setExecution({ ...execution, entry: event.target.value }); setEntryWasEdited(true); }} /></label><label>Shares<input type="number" min="1" step="1" value={execution.quantity} onChange={(event) => setExecution({ ...execution, quantity: event.target.value })} /></label><label>All buy costs · HKD<input type="number" min="0" step="any" value={execution.buyFees} onChange={(event) => setExecution({ ...execution, buyFees: event.target.value })} /></label><label>All sell costs · HKD<input type="number" min="0" step="any" value={execution.sellFees} onChange={(event) => setExecution({ ...execution, sellFees: event.target.value })} /></label><label>Verified available cash · HKD<input type="number" min="0" step="any" value={execution.capital} onChange={(event) => setExecution({ ...execution, capital: event.target.value })} placeholder="Refresh account balance first" /></label><label>Offer source and time<input value={execution.source} onChange={(event) => setExecution({ ...execution, source: event.target.value })} placeholder="Broker odd-lot offer · HKT date/time" /></label></div>
      <p className="thesis-muted">Provider bid {money(currentQuote?.bid ?? null)} / ask {money(currentQuote?.ask ?? null)} · {quoteUsable(currentQuote) ? "recent timestamp" : "freshness unconfirmed"}. Confirm odd-lot availability and size with your broker.</p>
      {currentQuote?.ask && assessment.currentQuote && <button className="secondary-button" onClick={() => { setExecution({ ...execution, entry: String(currentQuote.ask), source: `${currentQuote.provenance.provider} ask · ${currentQuote.provenance.observed_at}; odd-lot fill unconfirmed` }); setEntryWasEdited(false); }}>Use displayed ask for an indicative calculation</button>}
      {execution.entry && <p className="thesis-muted">{entryWasEdited ? "Manual offer: confirm price, time and available size." : "Indicative provider ask: ordinary book liquidity may differ from the odd-lot market."}</p>}
      <div className="thesis-scenarios">{economics.map(({ name, result }) => <article key={name}><strong>{name.toUpperCase()} · AFTER COSTS</strong><b>{result ? `${(result.netReturn * 100).toFixed(1)}% · ${money(result.netPnl)}` : "Inputs incomplete"}</b>{result && <small>Outlay {money(result.cost)} · break-even {money(result.breakEven)} · {result.cashWeight === null ? "cash allocation unknown" : `${(result.cashWeight * 100).toFixed(1)}% of entered cash${result.cashWeight > 1 ? " · EXCEEDS CASH" : ""}`}</small>}</article>)}</div>
    </section>
    <section className="market-panel"><p className="eyebrow">ACTION CONDITIONS</p><div className="thesis-conditions">{[["Add", profile.add], ["Trim", profile.trim], ["Exit / impairment", profile.exit], ["Re-entry", profile.reentry]].map(([label, detail]) => <article key={label}><strong>{label}</strong><p>{detail}</p></article>)}</div></section>
    <section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">SOURCE REVIEW</p><h3>Financial inputs and news leads</h3></div><a className="text-button" href="https://www.hkexnews.hk/search/titlesearch.xhtml?lang=en" target="_blank" rel="noreferrer">HKEX issuer announcements</a></div><p className="thesis-muted">{currentResearch ? `${currentResearch.provenance.provider} · ${currentResearch.provenance.freshness} · retrieved ${currentResearch.provenance.retrieved_at}` : "No research response yet."} Search links are navigation, not reviewed filings. Reading a headline does not confirm its thesis implications.</p>
      <div className="thesis-facts">{(currentResearch?.financials ?? []).slice(0, 16).map((fact, index) => <div key={`${fact.label}-${fact.period}-${index}`}><strong>{fact.label} · {fact.period ?? "period unspecified"}</strong><span>{fact.value} · {fact.source}</span></div>)}</div>
      {(currentResearch?.news ?? []).slice(0, 6).map((item, index) => <article className="thesis-news" key={`${item.title}-${index}`}><strong>{safeSourceUrl(item.url ?? "") ? <a href={safeSourceUrl(item.url!)!} target="_blank" rel="noreferrer">{item.title}</a> : item.title}</strong><small>{item.source} · {item.published_at ?? "publication date unknown"} · research lead</small></article>)}
    </section>
    <section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">LOCAL MODEL · THESIS CHALLENGER</p><h3>Support, counterargument, valuation and next action</h3></div><button className="primary-button" disabled={reviewing || loading} onClick={() => void review()}>{reviewing ? "Checking thesis…" : "Check thesis with Qwen"}</button></div><p className="thesis-muted">Uses the saved thesis, evidence journal, sourced research, timestamps, valuation assumptions and execution costs. Works inside Markets with the configured local model.</p>{reviewError && <p role="alert" className="thesis-attention">{reviewError}</p>}{staleReview && <p className="thesis-attention">Inputs changed after this review. Run it again before using the conclusion.</p>}{answer && <div className="markets-copilot-answer"><small>{model} · {staleReview ? "previous inputs" : "current inputs"}</small><ReactMarkdown remarkPlugins={[remarkGfm]}>{answer}</ReactMarkdown></div>}</section>
    <section className="market-panel"><p className="eyebrow">LOCAL PORTFOLIO DATA</p><h3>Portfolio records stay in local application storage</h3><p className="thesis-muted">This source repository contains no account snapshots or saved holdings. Records entered in the app remain on the user's device.</p></section>
  </div>;
}
