import { describe, expect, it } from "vitest";
import { assessThesis, blankThesis, executionEconomics, INVESTMENT_THESES, parseThesisEvidence, quoteUsable, reconcileThesisPositions, safeSourceUrl, scenarioValue, thesisAction, thesisFor, validateEvidence } from "./investmentThesis";
import type { ThesisEvidence, ThesisQuote } from "./investmentThesis";

const symbol = "TEST.001";
const now = Date.parse("2025-04-10T08:00:00Z");
const quote: ThesisQuote = { symbol, last: 10, bid: 9.9, ask: 10.1, provenance: { freshness: "Live", observed_at: "2025-04-10T07:59:00Z", retrieved_at: "2025-04-10T08:00:00Z", provider: "test" } };
const finding = (changes: Partial<ThesisEvidence> = {}): ThesisEvidence => ({ id: "test", symbol, criterionId: "business", direction: "supports", kind: "filing", stage: "production", date: "2025-04-08", source: "Test issuer report", url: "https://example.com/report", detail: "Synthetic test evidence.", ...changes });

describe("data-free thesis defaults", () => {
  it("ships without user-specific saved profiles or portfolio records", () => {
    expect(INVESTMENT_THESES).toEqual([]);
    expect(blankThesis(symbol).shares).toBe(0);
    expect(thesisFor(symbol)?.symbol).toBe(symbol);
  });
});

describe("thesis evidence discipline", () => {
  it("does not treat missing evidence as support for a decision", () => {
    const assessment = assessThesis(symbol, [], quote, now);
    expect(assessment.status).toBe("UNRESOLVED");
    expect(assessment.supported).toBe(0);
    expect(assessment.canConsiderAdding).toBe(false);
    expect(thesisAction(assessment).action).toBe("REVIEW");
  });

  it.each(["news", "expert", "observation"] as const)("keeps %s reports as verification leads", (kind) => {
    expect(assessThesis(symbol, [finding({ kind })], quote, now).checks[0].status).toBe("NEEDS VERIFICATION");
  });

  it.each(["sample", "design-win", "proposal", "commentary"] as const)("does not equate %s with realized results", (stage) => {
    expect(assessThesis(symbol, [finding({ stage })], quote, now).supported).toBe(0);
  });

  it("flags primary counterevidence against a critical assumption", () => {
    const assessment = assessThesis(symbol, [finding({ direction: "challenges", kind: "regulatory", stage: "realized" })], quote, now);
    expect(assessment.status).toBe("AT RISK");
    expect(thesisAction(assessment).action).toBe("REASSESS");
  });

  it("expires old findings and rejects future or impossible dates", () => {
    expect(assessThesis(symbol, [finding({ date: "2024-01-01" })], quote, now).supported).toBe(0);
    expect(validateEvidence(finding({ date: "2025-04-11" }), now)).toMatch(/future/);
    expect(validateEvidence(finding({ date: "2025-02-30" }), now)).toMatch(/real publication/);
  });

  it("does not let retrieval time disguise stale or simulated observations", () => {
    expect(quoteUsable(quote, now)).toBe(true);
    expect(quoteUsable({ ...quote, provenance: { ...quote.provenance, observed_at: "2025-03-01T08:00:00Z" } }, now)).toBe(false);
    expect(quoteUsable({ ...quote, provenance: { ...quote.provenance, observed_at: null } }, now)).toBe(false);
    expect(quoteUsable({ ...quote, provenance: { ...quote.provenance, freshness: "Simulated" } }, now)).toBe(false);
  });

  it("validates persisted evidence and source links defensively", () => {
    expect(parseThesisEvidence("{broken", now)).toEqual([]);
    expect(parseThesisEvidence(JSON.stringify([null, {}, finding(), finding({ url: "javascript:alert(1)" })]), now)).toHaveLength(1);
    expect(validateEvidence(finding({ url: "" }), now)).toMatch(/actual document/);
    expect(safeSourceUrl("https://user:placeholder@example.com")).toBeNull();
    expect(safeSourceUrl("http://example.com")).toBeNull();
  });
});

describe("explicit valuation and costs", () => {
  it("reconciles user-entered rows without bundling account data", () => {
    const rows = reconcileThesisPositions([{ account: "Account A", symbol, shares: 1 }], [{ account: "account a", symbol, shares: 1 }, { account: "Account B", symbol, shares: 2 }]);
    expect(rows.reduce((sum, row) => sum + row.shares, 0)).toBe(3);
  });

  it("requires positive explicit EPS and multiple", () => {
    expect(scenarioValue({ eps: 10, multiple: 20 })).toBe(200);
    expect(scenarioValue({ eps: 0, multiple: 20 })).toBeNull();
    expect(scenarioValue({ eps: -10, multiple: 20 })).toBeNull();
    expect(scenarioValue({ eps: Number.NaN, multiple: 20 })).toBeNull();
  });

  it("includes both commissions in net P/L and break-even", () => {
    const result = executionEconomics({ entry: 100, exit: 120, quantity: 2, buyFees: 10, sellFees: 5, capital: 1000 })!;
    expect(result.cost).toBe(210);
    expect(result.proceeds).toBe(235);
    expect(result.netPnl).toBe(25);
    expect(result.breakEven).toBe(107.5);
  });
});
