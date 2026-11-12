export const INVESTMENT_CONTEXT_DATE = "";
export const THESIS_STORAGE_KEY = "jadx.markets.thesis-evidence.v1";
export const INVESTOR_FRAMEWORK = "Business assumptions → evidence → risks → valuation → decision conditions";
export const INVESTOR_RULES = [
  "Record the source and observation date for each factual claim.",
  "Separate reported results from estimates, headlines, and proposals.",
  "Treat missing or stale evidence as unknown.",
  "Keep portfolio records local to the user and separate from provider data."
];

export type ThesisCriterion = { id: string; label: string; test: string; critical?: boolean };
export type CompanyThesis = {
  symbol: string; name: string; shares: number; averageCost: number; snapshotPrice: number;
  position: string; thesis: string; historicalContext: string; criteria: ThesisCriterion[];
  add: string; trim: string; exit: string; reentry: string; obsoleteAnchor: string;
};

export const INVESTMENT_THESES: CompanyThesis[] = [];

export function blankThesis(symbol: string): CompanyThesis {
  return {
    symbol,
    name: symbol || "Untitled company",
    shares: 0,
    averageCost: 0,
    snapshotPrice: 0,
    position: "No holdings or portfolio snapshot are bundled.",
    thesis: "Add your own dated, source-supported thesis for this symbol.",
    historicalContext: "No historical company context is included.",
    criteria: [
      { id: "business", label: "Business assumptions", test: "Record the operating assumptions that would need to hold.", critical: true },
      { id: "evidence", label: "Evidence quality", test: "Verify reported results and distinguish them from forecasts or commentary." },
      { id: "valuation", label: "Valuation and risks", test: "Record valuation assumptions, key risks, and what would invalidate them." }
    ],
    add: "Consider a change only after recording current evidence and valuation assumptions.",
    trim: "Review position size when the thesis weakens or risk exceeds the user's stated limit.",
    exit: "Reassess when a critical assumption is contradicted by verified evidence.",
    reentry: "Require fresh evidence and a documented valuation before reconsidering.",
    obsoleteAnchor: "No prior entry price or price target is bundled."
  };
}

export type ThesisEvidence = {
  id: string; symbol: string; criterionId: string; direction: "supports" | "challenges";
  kind: "filing" | "regulatory" | "expert" | "news" | "observation";
  stage: "realized" | "production" | "design-win" | "sample" | "proposal" | "commentary";
  date: string; source: string; url: string; detail: string;
};
export type ThesisQuote = { symbol: string; last?: number | null; bid?: number | null; ask?: number | null; provenance: { freshness: string; observed_at?: string | null; retrieved_at: string; provider: string } };
export type ThesisAssessment = ReturnType<typeof assessThesis>;
export const thesisFor = (symbol: string) => INVESTMENT_THESES.find((item) => item.symbol === symbol) ?? (symbol.trim() ? blankThesis(symbol.trim()) : undefined);

export type ThesisPosition = { account: string; symbol: string; shares: number; origin: string; mismatch?: string };
export function reconcileThesisPositions(holdings: { account: string; symbol: string; shares: number }[], tradePositions: { account: string; symbol: string; shares: number }[]): ThesisPosition[] {
  const key = (item: { account: string; symbol: string }) => `${item.account.trim().toLowerCase()}::${item.symbol}`;
  const byAccount = new Map<string, ThesisPosition>();
  for (const item of holdings) {
    const previous = byAccount.get(key(item));
    byAccount.set(key(item), { ...item, shares: item.shares + (previous?.shares ?? 0), origin: "holding snapshot" });
  }
  for (const item of tradePositions) {
    const snapshot = byAccount.get(key(item));
    if (!snapshot) byAccount.set(key(item), { ...item, origin: "dated trade ledger" });
    else if (Math.abs(snapshot.shares - item.shares) > 0.000001) snapshot.mismatch = `${item.account}: holding snapshot ${snapshot.shares} versus trade-derived ${item.shares}; reconcile the source before sizing.`;
  }
  return [...byAccount.values()];
}

export function safeSourceUrl(value: string): string | null {
  try { const url = new URL(value); return url.protocol === "https:" && !url.username && !url.password ? url.href : null; } catch { return null; }
}

export function validateEvidence(entry: ThesisEvidence, now = Date.now()): string | null {
  const profile = thesisFor(entry.symbol);
  if (!profile?.criteria.some((item) => item.id === entry.criterionId)) return "Choose a valid company and thesis test.";
  if (!["supports", "challenges"].includes(entry.direction) || !["filing", "regulatory", "expert", "news", "observation"].includes(entry.kind) || !["realized", "production", "design-win", "sample", "proposal", "commentary"].includes(entry.stage)) return "Choose a valid evidence type and stage.";
  const parsed = Date.parse(`${entry.date}T00:00:00+08:00`);
  if (!/^\d{4}-\d{2}-\d{2}$/.test(entry.date) || !Number.isFinite(parsed) || new Date(`${entry.date}T12:00:00Z`).toISOString().slice(0, 10) !== entry.date || parsed > now) return "Use a real publication date that is not in the future.";
  if (entry.detail.trim().length < 12 || entry.detail.length > 3000 || !entry.source.trim() || entry.source.length > 200) return "Include a source and a specific finding (12–3,000 characters).";
  if (entry.url && !safeSourceUrl(entry.url)) return "Use an HTTPS source link without embedded credentials.";
  if ((entry.kind === "filing" || entry.kind === "regulatory") && !safeSourceUrl(entry.url)) return "A filing or regulatory claim needs a link to the actual document.";
  return null;
}

export function parseThesisEvidence(raw: string | null, now = Date.now()): ThesisEvidence[] {
  try {
    const entries: unknown = JSON.parse(raw ?? "[]");
    if (!Array.isArray(entries)) return [];
    return entries.filter((item): item is ThesisEvidence => {
      if (!item || typeof item !== "object") return false;
      const record = item as Record<string, unknown>;
      return ["id", "symbol", "criterionId", "direction", "kind", "stage", "date", "source", "url", "detail"].every((key) => typeof record[key] === "string") && validateEvidence(item as ThesisEvidence, now) === null;
    }).slice(-500);
  } catch { return []; }
}

export function quoteUsable(quote: ThesisQuote | null | undefined, now = Date.now()) {
  if (!quote || !(typeof quote.last === "number" && Number.isFinite(quote.last) && quote.last > 0)) return false;
  if (!["Live", "Recent", "Delayed"].includes(quote.provenance.freshness)) return false;
  // A fresh retrieval of an old observation does not make the quote current.
  const timestamp = Date.parse(quote.provenance.observed_at ?? "");
  return Number.isFinite(timestamp) && timestamp <= now + 60_000 && now - timestamp <= 36 * 3_600_000;
}

export function assessThesis(symbol: string, evidence: ThesisEvidence[], quote?: ThesisQuote | null, now = Date.now()) {
  const profile = thesisFor(symbol);
  const currentQuote = quote?.symbol === symbol && quoteUsable(quote, now);
  const relevant = evidence.filter((entry) => entry.symbol === symbol && validateEvidence(entry, now) === null);
  const checks = (profile?.criteria ?? []).map((criterion) => {
    const entries = relevant.filter((entry) => entry.criterionId === criterion.id);
    const current = entries.filter((entry) => now - Date.parse(`${entry.date}T00:00:00+08:00`) <= 120 * 86_400_000);
    // Headlines, expert forecasts, samples and proposals remain leads, never proof.
    const qualified = current.filter((entry) => ["filing", "regulatory"].includes(entry.kind) && ["realized", "production"].includes(entry.stage));
    // One finding or duplicate report cannot erase unresolved contrary evidence.
    const positive = qualified.some((entry) => entry.direction === "supports");
    const negative = qualified.some((entry) => entry.direction === "challenges");
    const status = positive && negative ? "MIXED" : negative ? "CHALLENGED" : positive ? "SUPPORTED" : entries.length ? "NEEDS VERIFICATION" : "MISSING";
    return { ...criterion, status, entries, currentCount: qualified.length };
  });
  const supported = checks.filter((check) => check.status === "SUPPORTED").length;
  const challenged = checks.filter((check) => check.status === "CHALLENGED" || check.status === "MIXED");
  const criticalChallenge = challenged.some((check) => check.critical);
  const status = !profile ? "NO SAVED THESIS" : criticalChallenge ? "AT RISK" : challenged.length ? "MIXED" : supported === checks.length ? "SUPPORTED BY RECORDED EVIDENCE" : "UNRESOLVED";
  const canConsiderAdding = Boolean(profile && supported === checks.length && !challenged.length && currentQuote);
  return { profile, checks, status, supported, challenged: challenged.length, criticalChallenge, canConsiderAdding, currentQuote, coverage: checks.length ? supported / checks.length : 0 };
}

export function thesisAction(assessment: ThesisAssessment) {
  if (!assessment.profile) return { action: "WATCH", reason: "Save a company thesis before making a thesis-based decision." };
  if (assessment.criticalChallenge) return { action: "REASSESS", reason: "Recorded evidence challenges a critical assumption. Review impairment and trim/exit conditions before adding." };
  if (!assessment.canConsiderAdding) return { action: "REVIEW", reason: "Thesis tests or quote freshness remain unresolved. A technical BUY or price correction alone does not justify adding." };
  return { action: "VALUE FIRST", reason: "Recorded primary evidence supports the thesis. Check normalized valuation, executable costs and portfolio fit before considering an addition." };
}

export type EarningsScenario = { eps: number; multiple: number };
export function scenarioValue(scenario: EarningsScenario) {
  return Number.isFinite(scenario.eps) && Number.isFinite(scenario.multiple) && scenario.eps > 0 && scenario.multiple > 0 ? scenario.eps * scenario.multiple : null;
}

export function executionEconomics(input: { entry: number; exit: number; quantity: number; buyFees: number; sellFees: number; capital?: number }) {
  const { entry, exit, quantity, buyFees, sellFees, capital } = input;
  if (![entry, exit, quantity, buyFees, sellFees].every(Number.isFinite) || entry <= 0 || exit <= 0 || quantity <= 0 || !Number.isInteger(quantity) || buyFees < 0 || sellFees < 0) return null;
  const cost = entry * quantity + buyFees;
  const proceeds = exit * quantity - sellFees;
  return { cost, proceeds, netPnl: proceeds - cost, netReturn: (proceeds - cost) / cost, breakEven: (cost + sellFees) / quantity, cashWeight: capital && capital > 0 ? cost / capital : null };
}

export function thesisReviewContext(symbol: string, evidence: ThesisEvidence[], quote?: ThesisQuote | null) {
  const assessment = assessThesis(symbol, evidence, quote);
  return { asOf: new Date().toISOString(), snapshotDate: null, snapshotSource: "No portfolio snapshot is bundled; user-entered records remain local", framework: INVESTOR_FRAMEWORK, rules: INVESTOR_RULES, thesis: assessment.profile ?? null, assessment, holdingsSnapshot: [], cashSnapshot: null, realizedSnapshot: null };
}
