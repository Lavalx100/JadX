import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import InvestmentThesis, { ThesisSummary } from "./ThesisWorkspace";
import { assessThesis, INVESTMENT_THESES, parseThesisEvidence, reconcileThesisPositions, THESIS_STORAGE_KEY, thesisAction, thesisFor, thesisReviewContext } from "./investmentThesis";
import type { ThesisEvidence } from "./investmentThesis";
import {
  Activity,
  AlertTriangle,
  BarChart3,
  BookOpen,
  Building2,
  CandlestickChart,
  Check,
  ChevronRight,
  CircleDollarSign,
  Cpu,
  FileText,
  FlaskConical,
  Gauge,
  Layers3,
  LineChart,
  ListFilter,
  Newspaper,
  Plus,
  Play,
  Radar,
  RefreshCw,
  RotateCcw,
  Search,
  ShieldCheck,
  SlidersHorizontal,
  Sparkles,
  Target,
  Zap,
  WalletCards,
  X,
} from "lucide-react";
import "./markets.css";

type Freshness = "Live" | "Delayed" | "Recent" | "Cached" | "Stale" | "Simulated" | "Unavailable";
type Provenance = { provider: string; observed_at?: string | null; retrieved_at: string; freshness: Freshness; realtime: boolean; message?: string | null };
type Quote = {
  symbol: string;
  name: string;
  exchange: string;
  currency: string;
  last?: number | null;
  change?: number | null;
  change_pct?: number | null;
  bid?: number | null;
  ask?: number | null;
  volume?: number | null;
  turnover?: number | null;
  day_high?: number | null;
  day_low?: number | null;
  open?: number | null;
  previous_close?: number | null;
  market_cap?: number | null;
  price_to_book?: number | null;
  pe_ratio?: number | null;
  pe_ttm?: number | null;
  dividend_yield?: number | null;
  industry?: string | null;
  market_session: string;
  quote_level?: string | null;
  provenance: Provenance;
};
type Candle = { timestamp: number; open: number; high: number; low: number; close: number; volume: number; turnover?: number | null };
type TechnicalSnapshot = { sma20?: number | null; sma50?: number | null; ema20?: number | null; rsi14?: number | null; atr14?: number | null; volume_vs_sma20?: number | null; return_1d?: number | null; return_20d?: number | null; return_60d?: number | null; max_drawdown?: number | null };
type History = { symbol: string; interval: string; candles: Candle[]; indicators: TechnicalSnapshot; provenance: Provenance };
type MacroFactor = { label: string; score: number; detail: string; source: string };
type MacroSnapshot = { as_of: string; hsi?: Quote | null; hstech?: Quote | null; sp500?: Quote | null; nasdaq?: Quote | null; vix?: Quote | null; us10y?: Quote | null; dollar_index?: Quote | null; risk_on_score?: number | null; factors: MacroFactor[]; provenance: Provenance };
type FutuHealth = { host: string; port: number; tcp_reachable: boolean; sdk_available: boolean; connection_state: string; login_state: string; quote_permissions: string; subscription_usage: string; checked_at: string; message?: string | null };
type Watchlist = { id: string; name: string; symbols: string[]; updated_at: string };
type Holding = { id: string; account: string; symbol: string; shares: number; average_cost: number; currency: string; source: string; updated_at: string };
type PortfolioRow = { holding: Holding; quote: Quote; market_value?: number | null; unrealized_pnl?: number | null; unrealized_pnl_pct?: number | null };
type PortfolioTrade = { id: string; account: string; symbol: string; side: "BUY" | "SELL"; quantity: number; price: number; commission: number; trade_date: string; notes: string; source: string; created_at: string };
type RadarEvent = { id: string; symbol: string; title: string; detail: string; severity: string; provenance: Provenance };
type Dashboard = { health: FutuHealth; indices: Quote[]; watchlists: Watchlist[]; watchlist_quotes: Quote[]; portfolio: PortfolioRow[]; trades: PortfolioTrade[]; trade_quotes: Quote[]; radar: RadarEvent[]; trading_mode: string; fixture_mode: boolean; retrieved_at: string };
type OrderPreview = { symbol: string; side: string; quantity: number; order_type: string; limit_price?: number | null; estimated_notional?: number | null; market_state: string; valid: boolean; issues: string[]; trading_mode: string; live_submission_available: boolean; provenance: Provenance };
type MarketOrder = { id: string; symbol: string; side: string; quantity: number; order_type: string; limit_price?: number | null; mode: string; status: string; created_at: string };
type ResearchFact = { label: string; value: string; period?: string | null; source: string };
type ResearchItem = { title: string; detail: string; url?: string | null; published_at?: string | null; source: string };
type CompanyResearch = { symbol: string; name: string; profile: ResearchFact[]; financials: ResearchFact[]; news: ResearchItem[]; filings: ResearchItem[]; provenance: Provenance };
type ThematicCompany = { symbol: string; name: string; themes: string[]; listing_status: string; note: string; quote?: Quote | null };
type UniverseSnapshot = { retrieved_at: string; constituent_source: string; hsi: Quote[]; hstech: Quote[]; thematic: ThematicCompany[] };
type View = "dashboard" | "radar" | "watchlist" | "universe" | "companies" | "news" | "filings" | "research" | "screener" | "factors" | "signals" | "thesis" | "backtests" | "portfolio" | "holdings" | "performance" | "recommendations" | "transactions" | "risk" | "paper" | "orders" | "history";
type CompanyTab = "overview" | "chart" | "financials" | "valuation" | "analysts" | "filings" | "news" | "peers" | "quant";
type ChartMode = "candles" | "line" | "area" | "returns";
type ChartRange = "1W" | "1M" | "3M" | "6M" | "1Y" | "ALL";
type CompanySuggestion = { symbol: string; name: string; detail: string; aliases?: string[] };

const COMMON_COMPANY_SUGGESTIONS: CompanySuggestion[] = [];

type MarketsWorkspaceProps = {
  onOpenSettings: () => void;
  onError: (message: string) => void;
};

const NAV: { group: string; items: { id: View; label: string; icon: typeof Activity }[] }[] = [
  { group: "Overview", items: [{ id: "dashboard", label: "Dashboard", icon: Gauge }, { id: "radar", label: "Market Radar", icon: Radar }, { id: "watchlist", label: "Watchlist", icon: ListFilter }, { id: "universe", label: "Universe", icon: BarChart3 }] },
  { group: "Research", items: [{ id: "companies", label: "Companies", icon: Building2 }, { id: "news", label: "News", icon: Newspaper }, { id: "filings", label: "Filings", icon: FileText }, { id: "research", label: "Research Library", icon: BookOpen }] },
  { group: "Quant", items: [{ id: "screener", label: "Screener", icon: SlidersHorizontal }, { id: "factors", label: "Factors", icon: BarChart3 }, { id: "signals", label: "Analyzer", icon: Activity }, { id: "thesis", label: "Check my thesis", icon: BookOpen }, { id: "backtests", label: "Backtests", icon: LineChart }] },
  { group: "Portfolio", items: [{ id: "portfolio", label: "Overview", icon: WalletCards }, { id: "holdings", label: "Holdings", icon: CircleDollarSign }, { id: "performance", label: "Performance", icon: LineChart }, { id: "recommendations", label: "Recommendations", icon: Sparkles }, { id: "transactions", label: "Transactions", icon: FileText }, { id: "risk", label: "Risk", icon: AlertTriangle }] },
  { group: "Trading", items: [{ id: "paper", label: "Paper", icon: CandlestickChart }, { id: "orders", label: "Orders", icon: FileText }, { id: "history", label: "History", icon: Activity }] },
];

const DEFAULT_SIMULATION_CONFIG: SimulationConfig = {
  strategy: "composite",
  fastPeriod: 20,
  slowPeriod: 50,
  initialCapital: 100_000,
  feeBps: 8,
  slippageBps: 6,
  targetExposure: 1,
  stopLossPct: 0,
  takeProfitPct: 0,
};

function formatNumber(value?: number | null, digits = 2) {
  return value === undefined || value === null || !Number.isFinite(value) ? "—" : value.toLocaleString(undefined, { maximumFractionDigits: digits, minimumFractionDigits: digits });
}

function formatPercent(value?: number | null) {
  return value === undefined || value === null || !Number.isFinite(value) ? "—" : `${value >= 0 ? "+" : ""}${(value * 100).toFixed(2)}%`;
}

function formatCompact(value?: number | null) {
  if (value === undefined || value === null || !Number.isFinite(value)) return "—";
  if (Math.abs(value) >= 1_000_000_000) return `${(value / 1_000_000_000).toFixed(1)}B`;
  if (Math.abs(value) >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (Math.abs(value) >= 1_000) return `${(value / 1_000).toFixed(1)}K`;
  return formatNumber(value, 0);
}

type QuantAnalytics = {
  volatility20: number | null;
  bollingerMiddle: number | null;
  bollingerUpper: number | null;
  bollingerLower: number | null;
  macd: number | null;
  macdSignal: number | null;
  macdHistogram: number | null;
  support20: number | null;
  resistance20: number | null;
  trend: string;
  trendTone: "positive" | "negative" | undefined;
  signal: string;
  signalTone: "positive" | "negative" | undefined;
};

type BacktestStrategy = "trend" | "momentum" | "mean_reversion" | "composite";
type SimulationConfig = {
  strategy: BacktestStrategy;
  fastPeriod: number;
  slowPeriod: number;
  initialCapital: number;
  feeBps: number;
  slippageBps: number;
  targetExposure: number;
  stopLossPct: number;
  takeProfitPct: number;
};
type SimTrade = {
  entryTimestamp: number;
  exitTimestamp: number;
  entryPrice: number;
  exitPrice: number;
  return: number;
  grossPnl: number;
  netPnl: number;
  bars: number;
  reason: string;
};
type BacktestResult = {
  totalReturn: number;
  annualizedReturn: number;
  benchmarkReturn: number;
  maxDrawdown: number;
  sharpe: number;
  sortino: number;
  calmar: number;
  volatility: number;
  trades: number;
  winRate: number | null;
  profitFactor: number | null;
  averageTrade: number | null;
  exposure: number;
  turnover: number;
  fees: number;
  slippage: number;
  finalCapital: number;
  equity: number[];
  drawdown: number[];
  tradeLog: SimTrade[];
  walkForward: { trainingReturn: number; validationReturn: number; validationSharpe: number; stability: string };
  monteCarlo: { p05: number; p50: number; p95: number };
};

type SignalAnalyzer = {
  verdict: "BUY" | "HOLD" | "SELL";
  score: number;
  strength: string;
  confidence: number;
  dataCoverage: number;
  fastPeriod: number;
  slowPeriod: number;
  trainingSessions: number;
  validationSessions: number;
  trainingReturn: number;
  validationReturn: number;
  validationHitRate: number | null;
  factors: { label: string; score: number; detail: string }[];
  reasons: string[];
  investorLens: InvestorLens;
  outlook: OutlookSnapshot;
};

type InvestorLens = {
  thesis: string;
  evidence: string;
  valuation: string;
  positionSize: string;
  riskReward: string;
  catalyst: string;
  exitReentry: string;
};

type OutlookSnapshot = {
  shortTerm: string;
  longTerm: string;
  shortTermTarget: number | null;
  longTermLow: number | null;
  longTermBase: number | null;
  longTermHigh: number | null;
  targetMethod: string;
  relevantNews: ResearchItem[];
};

type ChartSignal = {
  timestamp: number;
  action: "BUY" | "SELL";
  price: number;
  score: number;
  reason: string;
};

type RiskSnapshot = {
  var95: number | null;
  cvar95: number | null;
  volatility: number | null;
  skew: number | null;
  lossDays: number;
  observations: number;
};

type ValuationSnapshot = {
  pe: number | null;
  earningsYield: number | null;
  priceToBook: number | null;
  score: number;
  coverage: number;
  detail: string;
};

type UniverseCandidate = {
  quote: Quote;
  analyzer: SignalAnalyzer | null;
  score: number;
  history: History | null;
  research: CompanyResearch | null;
};

type MarketsCopilotRequest = { prompt: string; nonce: number; target: "company" | "universe" | "analyzer" };

function localSma(values: number[], period: number) {
  if (values.length < period) return null;
  return values.slice(-period).reduce((sum, value) => sum + value, 0) / period;
}

function localEma(values: number[], period: number) {
  if (values.length < period) return null;
  let result = values.slice(0, period).reduce((sum, value) => sum + value, 0) / period;
  const multiplier = 2 / (period + 1);
  values.slice(period).forEach((value) => { result = (value - result) * multiplier + result; });
  return result;
}

function localStd(values: number[]) {
  if (values.length < 2) return null;
  const average = values.reduce((sum, value) => sum + value, 0) / values.length;
  return Math.sqrt(values.reduce((sum, value) => sum + (value - average) ** 2, 0) / (values.length - 1));
}

function clamp(value: number, minimum: number, maximum: number) {
  return Math.max(minimum, Math.min(maximum, value));
}

function localReturn(values: number[], period: number) {
  if (values.length <= period) return null;
  const start = values[values.length - 1 - period];
  const end = values.at(-1) ?? null;
  return start && end !== null && start > 0 ? end / start - 1 : null;
}

function localRsi(values: number[], period = 14) {
  if (values.length <= period) return null;
  let gains = 0;
  let losses = 0;
  for (let index = values.length - period; index < values.length; index += 1) {
    const change = values[index] - values[index - 1];
    if (change >= 0) gains += change;
    else losses -= change;
  }
  if (losses === 0) return 100;
  const averageGain = gains / period;
  const averageLoss = losses / period;
  return 100 - 100 / (1 + averageGain / averageLoss);
}

function researchValues(research: CompanyResearch | null | undefined, labels: string[]) {
  return (research?.financials ?? [])
    .filter((fact) => labels.includes(fact.label))
    .map((fact) => ({ fact, value: parseResearchAmount(fact.value) }))
    .filter((entry): entry is { fact: ResearchFact; value: number } => entry.value !== null && Number.isFinite(entry.value));
}

function fundamentalSnapshot(research?: CompanyResearch | null) {
  const revenue = researchValues(research, ["Total Revenue"]);
  const grossProfit = researchValues(research, ["Gross Profit"]);
  const netProfit = researchValues(research, ["Net Profit"]);
  const eps = researchValues(research, ["Diluted EPS"]);
  const revenueGrowth = revenue.length > 1 && revenue[1].value > 0 ? revenue[0].value / revenue[1].value - 1 : null;
  const epsGrowth = eps.length > 1 && eps[1].value > 0 ? eps[0].value / eps[1].value - 1 : null;
  const netMargin = revenue[0] && revenue[0].value > 0 && netProfit[0] ? netProfit[0].value / revenue[0].value : null;
  const grossMargin = revenue[0] && revenue[0].value > 0 && grossProfit[0] ? grossProfit[0].value / revenue[0].value : null;
  const components: number[] = [];
  if (revenueGrowth !== null) components.push(clamp(revenueGrowth / 0.35, -1, 1) * 0.38);
  if (epsGrowth !== null) components.push(clamp(epsGrowth / 0.45, -1, 1) * 0.22);
  if (netMargin !== null) components.push(clamp((netMargin - 0.06) / 0.28, -1, 1) * 0.28);
  if (grossMargin !== null) components.push(clamp((grossMargin - 0.20) / 0.45, -1, 1) * 0.12);
  const score = components.length ? clamp(components.reduce((total, value) => total + value, 0), -1, 1) : 0;
  const detail = components.length
    ? `Revenue growth ${formatPercent(revenueGrowth)} · EPS growth ${formatPercent(epsGrowth)} · net margin ${formatPercent(netMargin)}; sourced from ${revenue[0]?.fact.period ?? "latest"} statements.`
    : "Revenue growth, EPS trend, and margin data are unavailable; fundamentals stay neutral rather than guessing.";
  return { score, coverage: components.length / 4, revenueGrowth, epsGrowth, netMargin, grossMargin, detail };
}

function valuationSnapshot(quote?: Quote | null, research?: CompanyResearch | null): ValuationSnapshot {
  const pe = quote?.pe_ttm ?? quote?.pe_ratio ?? null;
  const priceToBook = quote?.price_to_book ?? null;
  const validPe = pe !== null && Number.isFinite(pe) && pe > 0 ? pe : null;
  const validPriceToBook = priceToBook !== null && Number.isFinite(priceToBook) && priceToBook > 0 ? priceToBook : null;
  const components: number[] = [];
  if (validPe !== null) {
    components.push(validPe <= 12 ? 0.45 : validPe <= 20 ? 0.24 : validPe <= 35 ? 0.06 : validPe <= 60 ? -0.08 : -0.18);
  }
  if (validPriceToBook !== null) {
    components.push(validPriceToBook <= 1.2 ? 0.35 : validPriceToBook <= 2.5 ? 0.18 : validPriceToBook <= 5 ? 0.02 : -0.16);
  }
  const fundamentals = fundamentalSnapshot(research);
  const growthSupport = fundamentals.revenueGrowth !== null && fundamentals.revenueGrowth > 0.15 ? 0.12 : 0;
  const qualitySupport = fundamentals.netMargin !== null && fundamentals.netMargin > 0.12 ? 0.06 : 0;
  const score = components.length ? clamp(components.reduce((total, value) => total + value, 0) / components.length + growthSupport + qualitySupport, -1, 1) : 0;
  const detail = components.length
    ? `P/E ${validPe === null ? "—" : formatNumber(validPe, 1)} · P/B ${validPriceToBook === null ? "—" : formatNumber(validPriceToBook, 2)}; growth and margin evidence can justify paying more, but valuation is never ignored.`
    : "Live P/E and P/B data is unavailable; valuation is neutral rather than treated as bearish.";
  return { pe: validPe, earningsYield: validPe ? 1 / validPe : null, priceToBook: validPriceToBook, score, coverage: components.length / 2, detail };
}

function parseResearchAmount(value: string) {
  const match = value.replace(/,/g, "").match(/(-?\d+(?:\.\d+)?)\s*(B|M|K)?/i);
  if (!match) return null;
  const amount = Number(match[1]);
  if (!Number.isFinite(amount)) return null;
  const multiplier = match[2]?.toUpperCase() === "B" ? 1_000_000_000 : match[2]?.toUpperCase() === "M" ? 1_000_000 : match[2]?.toUpperCase() === "K" ? 1_000 : 1;
  return amount * multiplier;
}

function riskSnapshot(candles: Candle[]): RiskSnapshot {
  const returns = candles.slice(1).map((candle, index) => {
    const previous = candles[index]?.close ?? 0;
    return previous > 0 && candle.close > 0 ? candle.close / previous - 1 : null;
  }).filter((value): value is number => value !== null && Number.isFinite(value));
  if (returns.length < 8) return { var95: null, cvar95: null, volatility: null, skew: null, lossDays: 0, observations: returns.length };
  const sorted = [...returns].sort((left, right) => left - right);
  const var95 = sorted[Math.max(0, Math.floor(sorted.length * 0.05))] ?? null;
  const tail = var95 === null ? [] : returns.filter((value) => value <= var95);
  const cvar95 = tail.length ? tail.reduce((sum, value) => sum + value, 0) / tail.length : var95;
  const mean = returns.reduce((sum, value) => sum + value, 0) / returns.length;
  const variance = returns.reduce((sum, value) => sum + (value - mean) ** 2, 0) / Math.max(1, returns.length - 1);
  const standardDeviation = Math.sqrt(variance);
  const skew = standardDeviation > 0
    ? returns.reduce((sum, value) => sum + ((value - mean) / standardDeviation) ** 3, 0) / returns.length
    : 0;
  return {
    var95,
    cvar95,
    volatility: standardDeviation * Math.sqrt(252),
    skew,
    lossDays: returns.filter((value) => value < 0).length,
    observations: returns.length,
  };
}

function buildChartSignals(candles: Candle[], fastPeriod = 20, slowPeriod = 50, context?: LiveAnalyzerContext): ChartSignal[] {
  if (candles.length < Math.max(60, slowPeriod + 5)) return [];
  const signals: ChartSignal[] = [];
  let previousState = 0;
  for (let index = slowPeriod; index < candles.length; index += 1) {
    const candle = candles[index];
    const snapshot = factorSnapshot(candles, index, fastPeriod, slowPeriod, context);
    const trendFactor = snapshot.factors.find((factor) => factor.label === "Trend structure")?.score ?? 0;
    const momentumFactor = snapshot.factors.find((factor) => factor.label === "Multi-horizon momentum")?.score ?? 0;
    const confirmedBreakdown = trendFactor <= -0.06 && momentumFactor <= -0.04;
    const previousClose = candles[index - 1]?.close ?? candle.close;
    const previousOpen = candles[index - 1]?.open ?? previousClose;
    const sma20 = localSma(candles.slice(0, index + 1).map((item) => item.close), 20);
    const technicalSell = Boolean(sma20 && candle.close < sma20 && candle.close < candle.open && previousClose < previousOpen);
    const state = technicalSell || snapshot.score <= -18 || confirmedBreakdown ? -1 : snapshot.score >= 18 ? 1 : 0;
    if (state !== 0 && state !== previousState) {
      const action = state > 0 ? "BUY" : "SELL";
      const price = candle.close;
      const trend = trendFactor;
      const momentum = momentumFactor;
      signals.push({
        timestamp: candle.timestamp,
        action,
        price,
        score: snapshot.score,
        reason: action === "BUY"
          ? `Composite score crossed +18 with ${trend >= 0 ? "trend" : "mixed trend"} and ${momentum >= 0 ? "positive" : "weak"} momentum.`
          : snapshot.score <= -18
            ? `Composite score crossed -18; reassess trend, momentum, valuation, and the original thesis.`
            : `Trend and price structure weakened below the moving average; review the thesis before adding.`,
      });
    }
    previousState = state;
  }
  return signals;
}

type TradePerformance = {
  account: string;
  symbol: string;
  trades: number;
  shares: number;
  averageCost: number;
  marketValue: number | null;
  realizedPnl: number;
  unrealizedPnl: number | null;
  commissions: number;
  totalPnl: number | null;
  returnPct: number | null;
};

type PerformanceRange = "3M" | "6M" | "1Y" | "2Y" | "ALL";
type PerformanceBenchmark = { symbol: string; label: string; category: string };
type PerformanceSeries = { benchmark: PerformanceBenchmark; values: (number | null)[]; history: History | null };
type PerformanceChartData = { dates: string[]; portfolioValues: number[]; portfolioIndex: number[]; benchmarks: PerformanceSeries[] };
type RecommendationData = { quote: Quote | null; history: History | null; research: CompanyResearch | null; macro: MacroSnapshot | null; analyzer: SignalAnalyzer | null; valuation: ValuationSnapshot | null; error?: string };
type RecommendationDecision = { action: "BUY" | "HOLD" | "SELL" | "WATCH" | "REVIEW" | "REASSESS"; summary: string; actions: string[] };

const PERFORMANCE_BENCHMARKS: PerformanceBenchmark[] = [
  { symbol: "HK.800000", label: "HSI", category: "Hong Kong index" },
  { symbol: "HK.800700", label: "Hang Seng TECH", category: "Hong Kong index" },
  { symbol: "HK.02800", label: "Tracker Fund (2800.HK)", category: "HK index fund" },
  { symbol: "HK.03033", label: "Hang Seng TECH ETF (3033.HK)", category: "HK index fund" },
  { symbol: "US.SPX", label: "S&P 500", category: "US index" },
  { symbol: "US.SPY", label: "SPDR S&P 500 ETF (SPY)", category: "US index fund" },
  { symbol: "US.QQQ", label: "Nasdaq-100 ETF (QQQ)", category: "US index fund" },
  { symbol: "US.VTI", label: "US Total Market ETF (VTI)", category: "US index fund" },
];

const DEFAULT_PERFORMANCE_BENCHMARKS = ["HK.800000", "HK.800700", "US.SPY"];

function performanceDateKey(timestamp: number) {
  return new Date(timestamp * 1000).toISOString().slice(0, 10);
}

function rangeCount(range: PerformanceRange) {
  return { "3M": 63, "6M": 126, "1Y": 252, "2Y": 504, ALL: Number.POSITIVE_INFINITY }[range];
}

function buildPerformanceChartData(trades: PortfolioTrade[], histories: Record<string, History | null>, selectedBenchmarks: string[], range: PerformanceRange): PerformanceChartData {
  const tradeSymbols = [...new Set(trades.map((trade) => trade.symbol))];
  const usableHistories = Object.entries(histories).filter((entry): entry is [string, History] => Boolean(entry[1]?.candles.length));
  const historyDates = [...new Set(usableHistories.flatMap(([, history]) => history.candles.map((candle) => performanceDateKey(candle.timestamp))))].sort();
  const firstTradeDate = trades.map((trade) => trade.trade_date).sort()[0];
  const allDates = historyDates.filter((date) => !firstTradeDate || date >= firstTradeDate);
  if (!allDates.length || !trades.length) return { dates: [], portfolioValues: [], portfolioIndex: [], benchmarks: [] };

  const sortedTrades = [...trades].sort((left, right) => `${left.trade_date}-${left.created_at}`.localeCompare(`${right.trade_date}-${right.created_at}`));
  const closeMaps = new Map<string, Map<string, number>>();
  usableHistories.forEach(([symbol, history]) => closeMaps.set(symbol, new Map(history.candles.map((candle) => [performanceDateKey(candle.timestamp), candle.close]))));
  const positions = new Map<string, number>();
  const lastCloses = new Map<string, number>();
  const portfolioValues: number[] = [];
  const portfolioIndex: number[] = [];
  let tradeIndex = 0;
  let previousValue = 0;
  let indexValue = 100;

  allDates.forEach((date, dateIndex) => {
    let externalFlow = 0;
    while (tradeIndex < sortedTrades.length && sortedTrades[tradeIndex].trade_date <= date) {
      const trade = sortedTrades[tradeIndex];
      const shares = positions.get(trade.symbol) ?? 0;
      const notional = trade.quantity * trade.price;
      if (trade.side === "BUY") {
        positions.set(trade.symbol, shares + trade.quantity);
        externalFlow += notional + trade.commission;
      } else {
        positions.set(trade.symbol, Math.max(0, shares - trade.quantity));
        externalFlow -= Math.max(0, notional - trade.commission);
      }
      tradeIndex += 1;
    }
    let holdingsValue = 0;
    tradeSymbols.forEach((symbol) => {
      const close = closeMaps.get(symbol)?.get(date);
      if (close !== undefined && Number.isFinite(close)) lastCloses.set(symbol, close);
      holdingsValue += (positions.get(symbol) ?? 0) * (lastCloses.get(symbol) ?? 0);
    });
    if (dateIndex > 0) {
      const capitalBase = previousValue + externalFlow;
      if (capitalBase > 0 && Number.isFinite(holdingsValue)) indexValue *= Math.max(0, holdingsValue / capitalBase);
    }
    portfolioValues.push(holdingsValue);
    portfolioIndex.push(indexValue);
    previousValue = holdingsValue;
  });

  const visibleStart = Math.max(0, allDates.length - rangeCount(range));
  const dates = allDates.slice(visibleStart);
  const visiblePortfolioValues = portfolioValues.slice(visibleStart);
  const portfolioBase = portfolioIndex[visibleStart] || 100;
  const visiblePortfolioIndex = portfolioIndex.slice(visibleStart).map((value) => (portfolioBase > 0 ? (value / portfolioBase) * 100 : 100));
  const benchmarks = selectedBenchmarks.map((symbol) => {
    const benchmark = PERFORMANCE_BENCHMARKS.find((item) => item.symbol === symbol) ?? { symbol, label: symbol, category: "Comparison" };
    const history = histories[symbol] ?? null;
    const closeMap = history?.candles.length ? new Map(history.candles.map((candle) => [performanceDateKey(candle.timestamp), candle.close])) : null;
    let firstClose: number | null = null;
    let lastClose: number | null = null;
    const fullValues = allDates.map((date) => {
      const close = closeMap?.get(date);
      if (close !== undefined && Number.isFinite(close)) lastClose = close;
      if (firstClose === null && lastClose !== null) firstClose = lastClose;
      return firstClose !== null && lastClose !== null ? (lastClose / firstClose) * 100 : null;
    });
    const visibleValues = fullValues.slice(visibleStart);
    const benchmarkBase = visibleValues.find((value): value is number => value !== null) ?? 100;
    return { benchmark, values: visibleValues.map((value) => value === null ? null : (value / benchmarkBase) * 100), history };
  });
  return { dates, portfolioValues: visiblePortfolioValues, portfolioIndex: visiblePortfolioIndex, benchmarks };
}

function recommendationDecision(data: RecommendationData, symbol: string, evidence: ThesisEvidence[]): RecommendationDecision {
  const thesis = assessThesis(symbol, evidence, data.quote);
  if (thesis.profile) {
    const result = thesisAction(thesis);
    return {
      action: result.action === "REASSESS" ? "REASSESS" : "REVIEW",
      summary: `${result.action}: ${result.reason}`,
      actions: [thesis.profile.add, thesis.profile.exit, thesis.profile.obsoleteAnchor],
    };
  }
  if (!data.analyzer) {
    return {
      action: "WATCH",
      summary: data.error ?? "The analyzer needs a longer sourced price history before it can produce a directional recommendation.",
      actions: ["Keep the position small or wait", "Review the latest filings and commercial evidence", "Re-run when history and valuation inputs are available"],
    };
  }
  const { analyzer } = data;
  if (analyzer.verdict === "BUY" && analyzer.confidence >= 45) {
    return {
      action: "BUY",
      summary: "Constructive research setup, but treat this as a starter position rather than a full-size entry.",
      actions: [analyzer.investorLens.positionSize, "Add only after the catalyst or commercial proof strengthens", "Use valuation and thesis evidence to decide whether to add or wait"],
    };
  }
  if (analyzer.verdict === "SELL" && analyzer.confidence >= 45) {
    return {
      action: "SELL",
      summary: "The current evidence does not justify adding risk; reduce or exit if the original thesis has weakened.",
      actions: ["Stop averaging down while the setup remains weak", analyzer.investorLens.exitReentry, "Re-enter only after valuation resets or commercial evidence improves"],
    };
  }
  return {
    action: "HOLD",
    summary: analyzer.verdict === "BUY" ? "The bias is positive but not strong enough for an aggressive new entry." : "Keep studying the thesis and avoid forcing a trade while the evidence is mixed.",
    actions: ["Hold existing exposure if the thesis remains intact", "Do not add until valuation, trend, or commercial evidence improves", analyzer.investorLens.catalyst],
  };
}

function calculateTradePerformance(trades: PortfolioTrade[], quotes: Quote[]) {
  const quoteMap = new Map(quotes.map((item) => [item.symbol, item]));
  const groups = new Map<string, { account: string; symbol: string; shares: number; costBasis: number; realizedPnl: number; commissions: number; invested: number; trades: number }>();
  [...trades].sort((left, right) => `${left.trade_date}-${left.created_at}`.localeCompare(`${right.trade_date}-${right.created_at}`)).forEach((trade) => {
    const key = `${trade.account}::${trade.symbol}`;
    const group = groups.get(key) ?? { account: trade.account, symbol: trade.symbol, shares: 0, costBasis: 0, realizedPnl: 0, commissions: 0, invested: 0, trades: 0 };
    const notional = trade.quantity * trade.price;
    group.trades += 1;
    group.commissions += trade.commission;
    if (trade.side === "BUY") {
      group.shares += trade.quantity;
      group.costBasis += notional + trade.commission;
      group.invested += notional + trade.commission;
    } else {
      const averageCost = group.shares > 0 ? group.costBasis / group.shares : 0;
      group.realizedPnl += notional - trade.commission - averageCost * trade.quantity;
      group.shares = Math.max(0, group.shares - trade.quantity);
      group.costBasis = Math.max(0, group.costBasis - averageCost * trade.quantity);
    }
    groups.set(key, group);
  });
  return [...groups.values()].map((group): TradePerformance => {
    const quote = quoteMap.get(group.symbol);
    const marketValue = quote?.last === null || quote?.last === undefined ? null : group.shares * quote.last;
    const unrealizedPnl = marketValue === null ? null : marketValue - group.costBasis;
    const totalPnl = unrealizedPnl === null ? group.realizedPnl : group.realizedPnl + unrealizedPnl;
    return {
      account: group.account,
      symbol: group.symbol,
      trades: group.trades,
      shares: group.shares,
      averageCost: group.shares > 0 ? group.costBasis / group.shares : 0,
      marketValue,
      realizedPnl: group.realizedPnl,
      unrealizedPnl,
      commissions: group.commissions,
      totalPnl,
      returnPct: group.invested > 0 ? totalPnl / group.invested : null,
    };
  }).sort((left, right) => (right.totalPnl ?? -Infinity) - (left.totalPnl ?? -Infinity));
}

function themeProfile(quote?: Quote | null) {
  const text = `${quote?.name ?? ""} ${quote?.industry ?? ""}`.toLowerCase();
  const tags = new Set<string>();
  if (/semiconductor|chip|foundry|wafer|memory|electronics|data center/i.test(text)) tags.add("semiconductors");
  if (/artificial intelligence|\bai\b|cloud|software|internet|platform|search|e-commerce/i.test(text)) tags.add("AI platforms");
  if (/autonomous|robot|robotics|lidar|perception|adas|intelligent driving/i.test(text)) tags.add("autonomous systems");
  if (/battery|energy storage|electric vehicle|\bev\b|automobile/i.test(text)) tags.add("EV / batteries");
  if (/biotech|pharma|medical|healthcare/i.test(text)) tags.add("biotech / healthcare");
  const narrowArchitecture = /lidar|single[- ]?sensor|narrow perception/i.test(text);
  return { tags: [...tags], narrowArchitecture };
}

function growthThemeEvidence(quote?: Quote | null, candles: Candle[] = []) {
  const profile = themeProfile(quote);
  const return20 = localReturn(candles.map((candle) => candle.close), 20);
  const return60 = localReturn(candles.map((candle) => candle.close), 60);
  const trendScore = clamp((return20 ?? 0) / 0.10, -1, 1) * 0.55 + clamp((return60 ?? 0) / 0.25, -1, 1) * 0.45;
  const thematicPrior = profile.tags.length ? clamp(0.34 + Math.min(profile.tags.length, 3) * 0.12, 0, 0.72) : 0;
  const score = profile.tags.length ? clamp(thematicPrior + trendScore * 0.28 - (profile.narrowArchitecture ? 0.12 : 0), -1, 1) : 0;
  const label = profile.tags.length ? "Thematic technology exposure" : "Thematic technology fit";
  const detail = profile.tags.length
    ? `${profile.tags.join(" · ")}; 20D ${formatPercent(return20)} · 60D ${formatPercent(return60)}. Theme fit is a thesis input, not proof of revenue.`
    : "Thematic technology exposure is unavailable; this factor stays neutral rather than guessing.";
  return { label, score, detail, available: profile.tags.length > 0, tags: profile.tags };
}

function qualitySnapshot(research?: CompanyResearch | null) {
  const snapshot = fundamentalSnapshot(research);
  return { score: snapshot.score, margin: snapshot.netMargin, coverage: snapshot.coverage, detail: snapshot.detail };
}

function commercialEvidence(research?: CompanyResearch | null) {
  const corpus = [...(research?.news ?? []), ...(research?.filings ?? [])].map((item) => `${item.title} ${item.detail}`).join(" ").toLowerCase();
  const signals = ["design win", "mass production", "customer adoption", "shipment", "order", "contract", "market share", "capacity", "margin improvement", "revenue growth"].filter((term) => corpus.includes(term));
  const score = signals.length >= 3 ? 0.62 : signals.length === 2 ? 0.38 : signals.length === 1 ? 0.16 : 0;
  const detail = signals.length
    ? `Public research mentions ${signals.slice(0, 4).join(", ")}; verify the commercial proof in filings before adding size.`
    : "No commercial-proof signal was found in the returned public research; treat catalysts as unverified.";
  return { score, detail, available: signals.length > 0 };
}

function technologyOptionality(quote?: Quote | null) {
  const profile = themeProfile(quote);
  const score = profile.narrowArchitecture ? -0.28 : profile.tags.length >= 2 ? 0.52 : profile.tags.length === 1 ? 0.26 : 0;
  const detail = profile.narrowArchitecture
    ? "Exposure appears concentrated in one perception architecture; substitution risk deserves a specific hedge or smaller starter position."
    : profile.tags.length >= 2
      ? `Exposure spans ${profile.tags.join(" and ")}; that provides more than one path for the technical thesis, but does not remove execution risk.`
      : profile.tags.length === 1
        ? `The technical thesis is concentrated in ${profile.tags[0]}; monitor substitution and customer concentration.`
        : "Technology optionality is unavailable; do not assume diversification across technical outcomes.";
  return { score, detail };
}

type LiveAnalyzerContext = { quote?: Quote | null; macro?: MacroSnapshot | null; valuation?: ValuationSnapshot; research?: CompanyResearch | null };

function factorSnapshot(candles: Candle[], index: number, fastPeriod: number, slowPeriod: number, context?: LiveAnalyzerContext) {
  const closes = candles.slice(0, index + 1).map((candle) => candle.close);
  const volumes = candles.slice(0, index + 1).map((candle) => candle.volume);
  const last = closes.at(-1) ?? null;
  const fast = localSma(closes, fastPeriod);
  const slow = localSma(closes, slowPeriod);
  const sma20 = localSma(closes, 20);
  const sma50 = localSma(closes, 50);
  const sma100 = localSma(closes, 100);
  const return5 = localReturn(closes, 5);
  const return20 = localReturn(closes, 20);
  const return60 = localReturn(closes, 60);
  const rsi = localRsi(closes);
  const recentReturns = closes.slice(-21).slice(1).map((value, offset) => {
    const previous = closes.slice(-21)[offset];
    return previous > 0 ? Math.log(value / previous) : 0;
  });
  const volatility = localStd(recentReturns);
  const recent = closes.slice(-20);
  const middle = localSma(closes, 20);
  const deviation = localStd(recent);
  const zScore = last !== null && middle !== null && deviation && deviation > 0 ? (last - middle) / deviation : null;
  const macdFast = localEma(closes, 12);
  const macdSlow = localEma(closes, 26);
  const macd = macdFast !== null && macdSlow !== null ? macdFast - macdSlow : null;
  const previousCloses = closes.slice(0, -1);
  const previousMacdFast = localEma(previousCloses, 12);
  const previousMacdSlow = localEma(previousCloses, 26);
  const previousMacd = previousMacdFast !== null && previousMacdSlow !== null ? previousMacdFast - previousMacdSlow : null;
  const macdSignal = macd !== null && previousMacd !== null ? (macd + previousMacd) / 2 : null;
  const volumeAverage = localSma(volumes, 20);
  const volumeRatio = volumeAverage && volumeAverage > 0 ? (volumes.at(-1) ?? 0) / volumeAverage : null;
  let peak = closes[0] ?? 0;
  let drawdown = 0;
  closes.slice(-60).forEach((value) => {
    peak = Math.max(peak, value);
    if (peak > 0) drawdown = Math.min(drawdown, value / peak - 1);
  });
  const trend = last !== null && fast !== null && slow !== null && sma20 !== null && sma50 !== null
    ? ((last > fast ? 0.35 : -0.35) + (fast > slow ? 0.35 : -0.35) + (sma20 > sma50 ? 0.3 : -0.3))
    : 0;
  const momentum = (return5 === null || return20 === null || return60 === null)
    ? 0
    : clamp(return5 / 0.025, -1, 1) * 0.3 + clamp(return20 / 0.10, -1, 1) * 0.4 + clamp(return60 / 0.25, -1, 1) * 0.3;
  const macdScore = macd !== null && macdSignal !== null && last && last > 0 ? clamp((macd - macdSignal) / (last * 0.012), -1, 1) : 0;
  const rsiScore = rsi === null ? 0 : rsi < 25 ? 0.35 : rsi < 35 ? 0.12 : rsi > 80 ? -0.35 : rsi > 70 ? -0.12 : clamp((rsi - 50) / 60, -0.08, 0.08);
  const bollingerScore = zScore === null ? 0 : clamp(-zScore / 3, -0.55, 0.55);
  const volumeScore = volumeRatio === null || return5 === null ? 0 : clamp((volumeRatio - 1) / 1.5, -1, 1) * Math.sign(return5);
  const riskScore = volatility === null ? 0 : clamp(0.6 - volatility * Math.sqrt(252), -1, 1) * 0.35 + clamp((drawdown + 0.35) / 0.35, -1, 1) * 0.15;
  const factors = [
    { label: "Trend structure", score: trend, detail: fast === null || slow === null ? "Moving-average structure needs more history." : `${last !== null && fast !== null && last > fast ? "Price is above" : "Price is below"} the calibrated fast average; trend is one input, not a standalone SELL trigger.` },
    { label: "Multi-horizon momentum", score: momentum, detail: `5D ${formatPercent(return5)} · 20D ${formatPercent(return20)} · 60D ${formatPercent(return60)}.` },
    { label: "MACD impulse", score: macdScore, detail: macdSignal === null ? "MACD signal comparison needs more history." : macdScore >= 0 ? "MACD histogram is positive or improving." : "MACD histogram is negative or weakening." },
    { label: "RSI regime", score: rsiScore, detail: rsi === null ? "RSI is unavailable." : `RSI ${formatNumber(rsi, 1)}; normal readings stay close to neutral and extremes are treated as risk, not certainty.` },
    { label: "Bollinger location", score: bollingerScore, detail: zScore === null ? "Bollinger position is unavailable." : `${zScore >= 0 ? "Above" : "Below"} the 20-session mean by ${Math.abs(zScore).toFixed(1)} standard deviations; mean reversion is capped.` },
    { label: "Volume confirmation", score: volumeScore, detail: volumeRatio === null ? "Volume confirmation is unavailable." : `Latest volume is ${volumeRatio.toFixed(2)}x its 20-session average.` },
    { label: "Risk / stability", score: riskScore, detail: `Annualized 20D volatility ${formatPercent(volatility === null ? null : volatility * Math.sqrt(252))}; 60D drawdown ${formatPercent(drawdown)}.` },
  ];
  const weights = [8, 7, 3, 2, 1, 1, 3];
  if (context) {
    const macroScore = context.macro?.risk_on_score ?? 0;
    const macroDetail = context.macro
      ? context.macro.factors.map((factor) => `${factor.label}: ${factor.detail}`).join(" · ") || "Macro sources returned without a directional factor."
      : "Live macro sources are unavailable; the regime overlay stays neutral rather than bearish.";
    factors.push({ label: "Live market regime", score: macroScore, detail: macroDetail });
    factors.push({ label: "Valuation / expectations", score: context.valuation?.score ?? 0, detail: context.valuation?.detail ?? "Valuation inputs are unavailable; this overlay stays neutral." });
    const growth = growthThemeEvidence(context.quote, candles.slice(0, index + 1));
    factors.push({ label: growth.label, score: growth.score, detail: growth.detail });
    const quality = qualitySnapshot(context.research);
    factors.push({ label: "Fundamental quality & growth", score: quality.score, detail: quality.detail });
    const commercial = commercialEvidence(context.research);
    factors.push({ label: "Commercial proof / catalysts", score: commercial.score, detail: commercial.detail });
    const optionality = technologyOptionality(context.quote);
    factors.push({ label: "Technology optionality", score: optionality.score, detail: optionality.detail });
  }
  const liveWeights = context ? [...weights, 12, 18, 22, 20, 9, 8] : weights;
  const score = factors.reduce((total, factor, factorIndex) => total + factor.score * liveWeights[factorIndex], 0) / liveWeights.reduce((total, weight) => total + weight, 0) * 100;
  const missing = factors.filter((factor) => factor.score === 0 && /unavailable|needs more history/i.test(factor.detail)).length;
  return { score, factors, coverage: 1 - missing / factors.length, volatility, drawdown };
}

function deriveAnalytics(history: History | null): QuantAnalytics {
  const closes = history?.candles.map((candle) => candle.close) ?? [];
  const recent = closes.slice(-20);
  const middle = localSma(closes, 20);
  const deviation = localStd(recent);
  const returns = closes.slice(-21).slice(1).map((value, index) => {
    const previous = closes.slice(-21)[index];
    return previous > 0 ? Math.log(value / previous) : 0;
  });
  const volatility = localStd(returns);
  const macdSeries = closes.map((_, index) => {
    const fast = localEma(closes.slice(0, index + 1), 12);
    const slow = localEma(closes.slice(0, index + 1), 26);
    return fast !== null && slow !== null ? fast - slow : null;
  }).filter((value): value is number => value !== null);
  const macd = macdSeries.at(-1) ?? null;
  const macdSignal = localEma(macdSeries, 9);
  const sma50 = localSma(closes, 50);
  const last = closes.at(-1) ?? null;
  const rsi = history?.indicators.rsi14 ?? null;
  const bullish = last !== null && middle !== null && sma50 !== null && last > middle && middle > sma50;
  const bearish = last !== null && middle !== null && sma50 !== null && last < middle && middle < sma50;
  const trend = bullish ? "Bullish trend" : bearish ? "Bearish trend" : "Range / transition";
  const signal = bullish && (rsi === null || rsi < 70) ? "Long bias" : bearish && (rsi === null || rsi > 30) ? "Short bias" : "Neutral / wait";
  return {
    volatility20: volatility === null ? null : volatility * Math.sqrt(252),
    bollingerMiddle: middle,
    bollingerUpper: middle !== null && deviation !== null ? middle + deviation * 2 : null,
    bollingerLower: middle !== null && deviation !== null ? middle - deviation * 2 : null,
    macd,
    macdSignal,
    macdHistogram: macd !== null && macdSignal !== null ? macd - macdSignal : null,
    support20: recent.length ? Math.min(...recent) : null,
    resistance20: recent.length ? Math.max(...recent) : null,
    trend,
    trendTone: bullish ? "positive" : bearish ? "negative" : undefined,
    signal,
    signalTone: signal === "Long bias" ? "positive" : signal === "Short bias" ? "negative" : undefined,
  };
}

function strategySignal(candles: Candle[], index: number, config: SimulationConfig) {
  if (index < 1) return 0;
  const closes = candles.slice(0, index + 1).map((candle) => candle.close);
  const fast = localSma(closes, config.fastPeriod);
  const slow = localSma(closes, config.slowPeriod);
  const return20 = localReturn(closes, Math.min(20, config.slowPeriod));
  const mean = localSma(closes, 20);
  const deviation = localStd(closes.slice(-20));
  const last = closes.at(-1) ?? null;
  if (last === null) return 0;
  if (config.strategy === "trend") return fast !== null && slow !== null && fast > slow && last > fast ? config.targetExposure : 0;
  if (config.strategy === "momentum") return return20 !== null && return20 > 0 && fast !== null && last > fast ? config.targetExposure : 0;
  if (config.strategy === "mean_reversion") {
    if (mean === null || deviation === null || deviation === 0) return 0;
    const zScore = (last - mean) / deviation;
    return zScore < -1 ? config.targetExposure : zScore > 0.2 ? 0 : config.targetExposure * 0.5;
  }
  if (fast === null || slow === null || mean === null || deviation === null) return 0;
  const zScore = deviation > 0 ? (last - mean) / deviation : 0;
  const composite = (fast > slow ? 0.4 : -0.25) + (return20 !== null ? clamp(return20 / 0.12, -1, 1) * 0.35 : 0) + clamp(-zScore / 4, -0.25, 0.25);
  return composite > 0.12 ? config.targetExposure : composite < -0.12 ? 0 : config.targetExposure * 0.35;
}

function simulateWindow(candles: Candle[], config: SimulationConfig, startIndex: number, endIndex: number) {
  const closes = candles.map((candle) => candle.close);
  const initialCapital = Math.max(1, config.initialCapital);
  const feeRate = Math.max(0, config.feeBps) / 10_000;
  const slippageRate = Math.max(0, config.slippageBps) / 10_000;
  let capital = initialCapital;
  let position = 0;
  let entryPrice = 0;
  let entryCapital = initialCapital;
  let entryIndex = 0;
  let lastMark = candles[Math.max(0, startIndex - 1)]?.close ?? candles[startIndex]?.open ?? 0;
  let exposureBars = 0;
  let turnover = 0;
  let fees = 0;
  let slippage = 0;
  let peak = initialCapital;
  let maxDrawdown = 0;
  const equity = [1];
  const drawdown = [0];
  const tradeLog: SimTrade[] = [];
  let pendingTradeCosts = 0;

  const recordTrade = (exitIndex: number, exitPrice: number, reason: string) => {
    if (position <= 0 || entryPrice <= 0) return;
    const grossPnl = entryCapital * (exitPrice / entryPrice - 1);
    tradeLog.push({
      entryTimestamp: candles[entryIndex]?.timestamp ?? 0,
      exitTimestamp: candles[exitIndex]?.timestamp ?? 0,
      entryPrice,
      exitPrice,
      return: exitPrice / entryPrice - 1,
      grossPnl,
      netPnl: grossPnl - pendingTradeCosts,
      bars: Math.max(1, exitIndex - entryIndex),
      reason,
    });
    pendingTradeCosts = 0;
  };

  const first = Math.max(1, startIndex);
  const last = Math.min(endIndex, candles.length - 1);
  for (let index = first; index <= last; index += 1) {
    const candle = candles[index];
    const open = candle.open > 0 ? candle.open : candle.close;
    const close = candle.close > 0 ? candle.close : open;
    if (position > 0 && lastMark > 0 && open > 0) {
      capital *= open / lastMark;
      lastMark = open;
    }
    let desired = strategySignal(candles, index - 1, config);
    let exitPrice = open;
    let exitReason = "signal";
    if (position > 0 && entryPrice > 0) {
      const stopPrice = config.stopLossPct > 0 ? entryPrice * (1 - config.stopLossPct / 100) : null;
      const takeProfitPrice = config.takeProfitPct > 0 ? entryPrice * (1 + config.takeProfitPct / 100) : null;
      if (stopPrice !== null && candle.low <= stopPrice) { desired = 0; exitPrice = stopPrice; exitReason = "stop loss"; }
      else if (takeProfitPrice !== null && candle.high >= takeProfitPrice) { desired = 0; exitPrice = takeProfitPrice; exitReason = "take profit"; }
    }
    if (desired !== position) {
      if (position > 0 && exitPrice !== open && open > 0) capital *= exitPrice / open;
      const notional = capital * Math.abs(desired - position);
      const fee = notional * feeRate;
      const slip = notional * slippageRate;
      capital = Math.max(0, capital - fee - slip);
      fees += fee;
      slippage += slip;
      turnover += Math.abs(desired - position);
      pendingTradeCosts += fee + slip;
      if (position > 0 && desired <= 0) recordTrade(index, exitPrice, exitReason);
      if (desired > 0) {
        entryPrice = open;
        entryCapital = capital;
        entryIndex = index;
      } else {
        entryPrice = 0;
        entryCapital = capital;
        entryIndex = index;
      }
      position = desired;
      lastMark = exitPrice;
    }
    if (position > 0 && lastMark > 0) {
      capital *= close / lastMark;
      exposureBars += 1;
    }
    lastMark = close;
    peak = Math.max(peak, capital);
    const currentDrawdown = peak > 0 ? capital / peak - 1 : 0;
    maxDrawdown = Math.min(maxDrawdown, currentDrawdown);
    equity.push(capital / initialCapital);
    drawdown.push(currentDrawdown);
  }
  if (position > 0 && last > 0) recordTrade(last, lastMark, "end of test");
  const dailyReturns = equity.slice(1).map((value, index) => equity[index] > 0 ? value / equity[index] - 1 : 0).filter(Number.isFinite);
  const average = dailyReturns.length ? dailyReturns.reduce((sum, value) => sum + value, 0) / dailyReturns.length : 0;
  const variance = dailyReturns.length > 1 ? dailyReturns.reduce((sum, value) => sum + (value - average) ** 2, 0) / (dailyReturns.length - 1) : 0;
  const standardDeviation = Math.sqrt(variance);
  const downside = dailyReturns.filter((value) => value < 0);
  const downsideDeviation = downside.length ? Math.sqrt(downside.reduce((sum, value) => sum + value ** 2, 0) / downside.length) : 0;
  const years = Math.max((last - first + 1) / 252, 1 / 252);
  const totalReturn = capital / initialCapital - 1;
  const annualizedReturn = Math.pow(Math.max(0.0001, capital / initialCapital), 1 / years) - 1;
  const benchmarkReturn = closes[Math.max(0, first - 1)] > 0 && closes[last] > 0 ? closes[last] / closes[Math.max(0, first - 1)] - 1 : 0;
  const positivePnl = tradeLog.filter((trade) => trade.netPnl > 0).reduce((sum, trade) => sum + trade.netPnl, 0);
  const negativePnl = Math.abs(tradeLog.filter((trade) => trade.netPnl < 0).reduce((sum, trade) => sum + trade.netPnl, 0));
  const sharpe = standardDeviation > 0 ? Math.sqrt(252) * average / standardDeviation : 0;
  const sortino = downsideDeviation > 0 ? Math.sqrt(252) * average / downsideDeviation : 0;
  return {
    totalReturn,
    annualizedReturn,
    benchmarkReturn,
    maxDrawdown,
    sharpe,
    sortino,
    calmar: Math.abs(maxDrawdown) > 0 ? annualizedReturn / Math.abs(maxDrawdown) : 0,
    volatility: standardDeviation * Math.sqrt(252),
    trades: tradeLog.length,
    winRate: tradeLog.length ? tradeLog.filter((trade) => trade.netPnl > 0).length / tradeLog.length : null,
    profitFactor: negativePnl > 0 ? positivePnl / negativePnl : positivePnl > 0 ? null : 0,
    averageTrade: tradeLog.length ? tradeLog.reduce((sum, trade) => sum + trade.return, 0) / tradeLog.length : null,
    exposure: (last - first + 1) > 0 ? exposureBars / (last - first + 1) : 0,
    turnover,
    fees,
    slippage,
    finalCapital: capital,
    equity,
    drawdown,
    tradeLog,
    dailyReturns,
  };
}

function monteCarloBand(returns: number[]) {
  if (!returns.length) return { p05: 0, p50: 0, p95: 0 };
  const finals: number[] = [];
  for (let path = 0; path < 240; path += 1) {
    let value = 1;
    for (let step = 0; step < returns.length; step += 1) {
      const index = (path * 37 + step * 17 + (path % 11) * 7) % returns.length;
      value *= Math.max(0.01, 1 + returns[index]);
    }
    finals.push(value - 1);
  }
  finals.sort((left, right) => left - right);
  return { p05: finals[Math.floor(finals.length * 0.05)], p50: finals[Math.floor(finals.length * 0.5)], p95: finals[Math.floor(finals.length * 0.95)] };
}

function runStrategyBacktest(history: History | null, config: SimulationConfig): BacktestResult | null {
  const candles = history?.candles ?? [];
  if (candles.length < Math.max(60, config.slowPeriod + 10)) return null;
  const full = simulateWindow(candles, config, 1, candles.length - 1);
  const split = Math.max(config.slowPeriod + 5, Math.floor(candles.length * 0.65));
  const training = simulateWindow(candles, config, 1, split);
  const validation = simulateWindow(candles, config, split, candles.length - 1);
  const stability = validation.totalReturn > 0 && validation.sharpe >= 0.5 ? "Robust across the holdout window" : validation.totalReturn > -0.05 ? "Mixed holdout; treat as exploratory" : "Fragile holdout; do not promote without more evidence";
  return {
    ...full,
    walkForward: { trainingReturn: training.totalReturn, validationReturn: validation.totalReturn, validationSharpe: validation.sharpe, stability },
    monteCarlo: monteCarloBand(full.dailyReturns),
  };
}

function runQuantWindow(candles: Candle[], fastPeriod: number, slowPeriod: number, startIndex: number, endIndex: number) {
  const closes = candles.map((candle) => candle.close);
  let equity = 1;
  let observations = 0;
  let directionalObservations = 0;
  let wins = 0;
  const firstIndex = Math.max(startIndex, slowPeriod, 20);
  const finalIndex = Math.min(endIndex, closes.length - 1);
  for (let index = firstIndex; index < finalIndex; index += 1) {
    const snapshot = factorSnapshot(candles, index, fastPeriod, slowPeriod);
    const nextClose = closes[index + 1];
    const currentClose = closes[index];
    if (!Number.isFinite(nextClose) || !Number.isFinite(currentClose) || currentClose <= 0) continue;
    const nextReturn = nextClose / currentClose - 1;
    const position = snapshot.score >= 18 ? 1 : snapshot.score <= -18 ? -1 : 0;
    if (position !== 0) {
      directionalObservations += 1;
      equity *= Math.max(0.01, 1 + position * nextReturn);
      if (Math.sign(snapshot.score) === Math.sign(nextReturn)) wins += 1;
    }
    observations += 1;
  }
  return {
    returnValue: equity - 1,
    hitRate: directionalObservations ? wins / directionalObservations : null,
    observations,
  };
}

function relevantNewsForThesis(quote: Quote | null | undefined, research?: CompanyResearch | null) {
  const themeTerms = themeProfile(quote).tags.flatMap((tag) => tag.toLowerCase().split(/[^a-z0-9]+/).filter((term) => term.length > 2));
  const evidenceTerms = ["design", "production", "shipment", "order", "contract", "adoption", "margin", "revenue", "capacity", "market share", "customer", "chip", "ai"];
  const scored = (research?.news ?? []).map((item) => {
    const text = `${item.title} ${item.detail}`.toLowerCase();
    const score = [...themeTerms, ...evidenceTerms].filter((term) => text.includes(term)).length;
    return { item, score };
  }).filter((entry) => entry.score > 0).sort((left, right) => right.score - left.score).map((entry) => entry.item);
  return scored.length ? scored.slice(0, 3) : (research?.news ?? []).slice(0, 3);
}

function buildOutlook(candles: Candle[], quote: Quote | null | undefined, research: CompanyResearch | null | undefined, macro: MacroSnapshot | null | undefined, score: number, confidence: number, valuation: ValuationSnapshot): OutlookSnapshot {
  const savedThesis = thesisFor(quote?.symbol ?? "");
  const closes = candles.map((candle) => candle.close);
  const current = quote?.last ?? closes.at(-1) ?? null;
  const theme = growthThemeEvidence(quote, candles);
  const fundamentals = fundamentalSnapshot(research);
  const riskOn = macro?.risk_on_score ?? 0;
  const shortBias = score >= 24
    ? "Constructive 1–3 month setup, with volatility expected; use pullbacks only if the thesis evidence remains intact."
    : score >= 0
      ? "Neutral-to-constructive 1–3 month setup; wait for a catalyst or better valuation rather than chasing a move."
      : "Cautious 1–3 month setup; this is not an automatic SELL, but the entry should require stronger evidence or a better price.";
  const longBias = theme.tags.length && (fundamentals.revenueGrowth !== null || fundamentals.netMargin !== null)
    ? "Constructive 12–24 month thematic thesis if revenue, margins, production, or customer adoption validate the technology shift."
    : theme.tags.length
      ? "Potentially attractive 12–24 month thematic thesis, but commercial and financial proof is still incomplete."
      : "Long-term thesis is not sufficiently evidenced by the available data; keep the position exploratory until the theme is verified.";
  const shortMove = clamp((score / 100) * 0.08 + riskOn * 0.03, -0.07, 0.12);
  const growth = (fundamentals.revenueGrowth ?? fundamentals.epsGrowth ?? 0) * 0.55 + (fundamentals.epsGrowth ?? 0) * 0.25;
  const longMove = clamp(growth + valuation.score * 0.10 + theme.score * 0.10, -0.25, 0.70);
  const shortTermTarget = current === null ? null : current * (1 + shortMove);
  const longTermBase = current === null ? null : current * (1 + longMove);
  const longTermLow = current === null ? null : current * (1 + clamp(longMove - 0.20, -0.50, 0.15));
  const longTermHigh = current === null ? null : current * (1 + clamp(longMove + 0.25, -0.10, 1.00));
  return {
    shortTerm: `${shortBias} Confidence is ${confidence}% because ${confidence >= 55 ? "the primary quote, history, and research inputs are reasonably complete" : "some evidence is still incomplete"}.`,
    longTerm: savedThesis ? `${savedThesis.thesis} Test current support and counterevidence in Check my thesis before deciding whether to add, hold or reduce.` : longBias,
    shortTermTarget: savedThesis ? null : shortTermTarget,
    longTermLow: savedThesis ? null : longTermLow,
    longTermBase: savedThesis ? null : longTermBase,
    longTermHigh: savedThesis ? null : longTermHigh,
    targetMethod: savedThesis ? "Targets require revised normalized earnings and valuation assumptions. Use Check my thesis for explicit bear/base/bull scenarios after costs; purchase price and the technical score do not determine fair value." : "Model reference only: short-term target blends composite score and live regime; long-term range blends sourced growth/margins, valuation, and theme fit. It is not a broker target or a forecast.",
    relevantNews: relevantNewsForThesis(quote, research),
  };
}

function buildInvestorLens(quote: Quote | null | undefined, research: CompanyResearch | null | undefined, macro: MacroSnapshot | null | undefined, score: number, confidence: number, coverage: number, valuation: ValuationSnapshot): InvestorLens {
  const savedThesis = thesisFor(quote?.symbol ?? "");
  const theme = growthThemeEvidence(quote);
  const fundamentals = fundamentalSnapshot(research);
  const commercial = commercialEvidence(research);
  const optionality = technologyOptionality(quote);
  const name = quote?.name ?? quote?.symbol ?? "This company";
  const themeText = theme.tags.length ? theme.tags.join(", ") : "a technology theme that still needs to be verified";
  const regimeText = macro?.risk_on_score === null || macro?.risk_on_score === undefined
    ? "The live market regime is unavailable, so the starter position should stay small."
    : `The live regime score is ${macro.risk_on_score >= 0 ? "+" : ""}${(macro.risk_on_score * 100).toFixed(0)}; use it to adjust exposure, not to override the company thesis.`;
  return {
    thesis: savedThesis?.thesis ?? `${name} is being evaluated as a ${themeText} thesis. A positive score means the evidence/reward setup is improving, not that the next price move is known.`,
    evidence: `${fundamentals.detail} ${commercial.detail} ${regimeText}`,
    valuation: valuation.coverage > 0 ? valuation.detail : "Valuation data is incomplete; do not pay up without stronger commercial evidence or a clearer catalyst.",
    positionSize: savedThesis ? `${savedThesis.add} Verify current cash and account exposure before sizing; the historical cash snapshot is not available buying power.` : confidence >= 60 && coverage >= 0.75 && score > 15
      ? "Starter position: roughly 1–3% of the intended allocation. Add only after commercial proof, margin/revenue improvement, or a more attractive valuation."
      : "Starter position: roughly 0.5–1% of the intended allocation while inputs are partial or the thesis is unproven. Do not blindly average down.",
    riskReward: score > 20 && valuation.score >= 0
      ? "Risk/reward is constructive, but upside depends on the market recognizing earnings or adoption evidence; volatility can still be high."
      : "Risk/reward is mixed: wait for a better entry, stronger evidence, or a clearer catalyst instead of forcing a BUY.",
    catalyst: savedThesis ? `Company-specific tests: ${savedThesis.criteria.map((item) => item.label).join("; ")}. Review the evidence journal before treating a headline as proof.` : "Next proof point: design wins, mass production, customer adoption, shipments, market-share gains, revenue growth, or margin improvement. Headlines alone are not enough.",
    exitReentry: savedThesis ? `${savedThesis.trim} ${savedThesis.exit} ${savedThesis.reentry}` : `${optionality.detail} Reduce if the commercial thesis weakens, expectations outrun fundamentals, or substitution risk rises. Re-enter only after valuation resets and the evidence improves.`,
  };
}

function runSignalAnalyzer(history: History | null, quote?: Quote | null, macro?: MacroSnapshot | null, research?: CompanyResearch | null): SignalAnalyzer | null {
  const candles = history?.candles ?? [];
  if (candles.length < 80) return null;
  const trainingEnd = Math.max(65, Math.floor(candles.length * 0.65));
  const candidates = [
    [10, 40], [10, 50], [10, 80],
    [20, 40], [20, 50], [20, 80],
    [30, 50], [30, 80],
  ];
  let best: { fast: number; slow: number; result: ReturnType<typeof runQuantWindow> } | null = null;
  for (const [fast, slow] of candidates) {
    const result = runQuantWindow(candles, fast, slow, 0, trainingEnd);
    const quality = result.returnValue + ((result.hitRate ?? 0.5) - 0.5) * 0.4;
    const bestQuality = best ? best.result.returnValue + ((best.result.hitRate ?? 0.5) - 0.5) * 0.4 : -Infinity;
    if (!best || quality > bestQuality) best = { fast, slow, result };
  }
  if (!best) return null;
  const validation = runQuantWindow(candles, best.fast, best.slow, trainingEnd, candles.length);
  const valuation = valuationSnapshot(quote, research);
  const liveContext = quote !== undefined || macro !== undefined || research !== undefined ? { quote, macro, valuation, research } : undefined;
  const latest = factorSnapshot(candles, candles.length - 1, best.fast, best.slow, liveContext);
  const validationAdjustment = validation.hitRate === null ? 1 : clamp(0.9 + (validation.hitRate - 0.5) * 0.7, 0.78, 1.12);
  const score = Math.round(clamp(latest.score * validationAdjustment * (0.8 + latest.coverage * 0.2), -100, 100));
  const verdict = score >= 24 ? "BUY" : score <= -40 ? "SELL" : "HOLD";
  const confidence = Math.round(clamp(Math.abs(score) * 0.45 + Math.abs((validation.hitRate ?? 0.5) - 0.5) * 65 + latest.coverage * 30, 0, 100));
  const strength = confidence >= 70 ? "Strong" : confidence >= 45 ? "Moderate" : "Weak / mixed";
  const positiveFactors = latest.factors.filter((factor) => factor.score > 0.2).map((factor) => factor.label).slice(0, 3);
  const negativeFactors = latest.factors.filter((factor) => factor.score < -0.2).map((factor) => factor.label).slice(0, 3);
  return {
    verdict,
    score,
    strength,
    confidence,
    dataCoverage: latest.coverage,
    fastPeriod: best.fast,
    slowPeriod: best.slow,
    trainingSessions: best.result.observations,
    validationSessions: validation.observations,
    trainingReturn: best.result.returnValue,
    validationReturn: validation.returnValue,
    validationHitRate: validation.hitRate,
    factors: latest.factors,
    reasons: [
      `Walk-forward calibration selected ${best.fast}/${best.slow} SMA windows using ${best.result.observations} earlier sessions and a return-plus-hit-rate objective.`,
      positiveFactors.length ? `Positive contributors: ${positiveFactors.join(", ")}.` : "No factor group currently has a strong positive contribution.",
      negativeFactors.length ? `Negative contributors: ${negativeFactors.join(", ")}.` : "No factor group currently has a strong negative contribution.",
      `The score is thematic and fundamentals-first: technical structure is a smaller timing input, while valuation, fundamental quality/growth, commercial proof, technology optionality, and live market regime carry more weight.` ,
      `Live regime score is ${macro?.risk_on_score === null || macro?.risk_on_score === undefined ? "unavailable" : `${macro.risk_on_score >= 0 ? "+" : ""}${(macro.risk_on_score * 100).toFixed(0)}`}; valuation overlay is ${valuation.coverage > 0 ? "available" : "unavailable"}.`,
      `Unseen-session validation returned ${formatPercent(validation.returnValue)} with ${validation.observations} sessions and hit rate ${formatPercent(validation.hitRate)}.`,
      `Data coverage is ${(latest.coverage * 100).toFixed(0)}%; unavailable inputs reduce confidence rather than being silently treated as bullish or bearish.`,
    ],
    investorLens: buildInvestorLens(quote, research, macro, score, confidence, latest.coverage, valuation),
    outlook: buildOutlook(candles, quote, research, macro, score, confidence, valuation),
  };
}

const QUANT_GLOSSARY = [
  ["SMA", "Simple moving average: the average closing price over a fixed number of sessions."],
  ["RSI", "Relative Strength Index: a 0–100 momentum oscillator. High or low readings can mean a move is stretched, not that a reversal is guaranteed."],
  ["MACD", "Moving Average Convergence Divergence: the gap between fast and slow exponential averages; the histogram compares it with its signal line."],
  ["Volatility", "How widely returns vary. Higher volatility means wider historical price swings and usually larger risk."],
  ["Max drawdown", "The largest peak-to-trough loss in a tested equity curve."],
  ["Walk-forward", "Tune parameters on earlier candles, then evaluate them on later candles that were not used for tuning."],
  ["Look-ahead bias", "Accidentally using future information. JadX calculates each signal from closes available before that session."],
];

function freshnessClass(freshness: Freshness) {
  return freshness.toLowerCase().replace("unavailable", "unavailable");
}

function FreshnessBadge({ provenance }: { provenance: Provenance }) {
  return <span className={`market-freshness ${freshnessClass(provenance.freshness)}`} title={provenance.message ?? `${provenance.provider} · retrieved ${provenance.retrieved_at}`}><i />{provenance.freshness.toUpperCase()}{provenance.observed_at && <small> · {new Date(provenance.observed_at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</small>}</span>;
}

function LiveMarketDeck({ quote, macro, health }: { quote: Quote | null; macro: MacroSnapshot | null; health?: FutuHealth }) {
  const riskOn = macro?.risk_on_score ?? null;
  return <div className="markets-live-deck" aria-label="Live market telemetry"><div className="live-deck-status"><span className="live-pulse" /><div><strong>LIVE MARKET TELEMETRY</strong><small>{health?.connection_state ?? "Connecting to OpenD"} · local-first</small></div></div><div className="live-deck-item"><span>ACTIVE</span><strong>{quote?.symbol ?? "—"}</strong><small>{quote?.name ?? "Waiting for security"}</small></div><div className="live-deck-item"><span>LAST</span><strong>{quote?.last === null || quote?.last === undefined ? "—" : `${quote.currency} ${formatNumber(quote.last)}`}</strong><small className={(quote?.change_pct ?? 0) >= 0 ? "market-positive" : "market-negative"}>{formatPercent(quote?.change_pct)} today</small></div><div className="live-deck-item"><span>REGIME</span><strong className={(riskOn ?? 0) >= 0 ? "market-positive" : "market-negative"}>{riskOn === null ? "—" : `${riskOn >= 0 ? "+" : ""}${(riskOn * 100).toFixed(0)}`}</strong><small>risk-on score</small></div><div className="live-deck-item"><span>INDEX PULSE</span><strong>{formatPercent(macro?.hstech?.change_pct)}</strong><small>HSTECH · HSI {formatPercent(macro?.hsi?.change_pct)}</small></div><FreshnessBadge provenance={quote?.provenance ?? macro?.provenance ?? { provider: "markets", retrieved_at: new Date().toISOString(), freshness: "Unavailable", realtime: false }} /></div>;
}

function CompanyAutocomplete({ value, onChange, suggestions, onSelect, onSubmit, placeholder, ariaLabel }: { value: string; onChange: (value: string) => void; suggestions: CompanySuggestion[]; onSelect: (suggestion: CompanySuggestion) => void; onSubmit: () => void; placeholder: string; ariaLabel: string }) {
  return <div className="company-autocomplete"><div className="company-autocomplete-input"><Search size={15} /><input aria-label={ariaLabel} aria-autocomplete="list" value={value} onChange={(event) => onChange(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") { event.preventDefault(); onSubmit(); } }} placeholder={placeholder} /></div>{value.trim() && suggestions.length > 0 && <div className="company-autocomplete-list" role="listbox">{suggestions.map((suggestion) => <button key={suggestion.symbol} role="option" onMouseDown={(event) => event.preventDefault()} onClick={() => onSelect(suggestion)}><strong>{suggestion.name}</strong><span>{suggestion.symbol} · {suggestion.detail}</span></button>)}</div>}</div>;
}

function QuoteCard({ quote, onSelect }: { quote: Quote; onSelect?: () => void }) {
  const move = quote.change_pct ?? 0;
  return <button className="market-index-card" onClick={onSelect} disabled={!onSelect}><div className="market-index-card-head"><span>{quote.name}</span><FreshnessBadge provenance={quote.provenance} /></div><strong>{quote.last === undefined || quote.last === null ? "Data unavailable" : `${quote.currency} ${formatNumber(quote.last)}`}</strong><div className={move >= 0 ? "market-positive" : "market-negative"}>{quote.change_pct === undefined || quote.change_pct === null ? "—" : `${move >= 0 ? "+" : ""}${(move * 100).toFixed(2)}%`} {quote.change !== undefined && quote.change !== null && <small>{quote.change >= 0 ? "+" : ""}{formatNumber(quote.change)}</small>}</div><small className="market-card-source">{quote.provenance.provider} · {quote.market_session}</small></button>;
}

function SignalGuide({ signals, currentScore, risk }: { signals: ChartSignal[]; currentScore?: number | null; risk: RiskSnapshot }) {
  const lastSignal = signals.at(-1);
  return <div className="signal-guide">
    <div className="signal-guide-head"><div><p className="eyebrow">CHART DECISION MAP</p><strong>Read the markers before reading the verdict</strong></div><span className="market-muted-label">Model research aid · not an order</span></div>
    <div className="signal-guide-grid">
      <article className="signal-guide-rule buy"><b>BUY marker</b><span>Timing score crosses +18</span><small>Trend, momentum, valuation, theme, and live regime are combined. It is an entry condition for research; the validation-adjusted verdict still has to agree.</small></article>
      <article className="signal-guide-rule hold"><b>HOLD / WAIT</b><span>Score remains between the bands</span><small>Keep studying the thesis, wait for a better valuation, or wait for commercial evidence instead of forcing a trade.</small></article>
      <article className="signal-guide-rule sell"><b>SELL / REDUCE marker</b><span>Timing score crosses −18</span><small>Re-check the thesis, risk budget, and evidence. A marker means the setup weakened; it does not predict a precise top.</small></article>
    </div>
    <div className="signal-guide-footer"><span>{lastSignal ? `Last marker: ${lastSignal.action} at ${formatNumber(lastSignal.price)} · ${lastSignal.reason}` : "No threshold crossing in the displayed history."}</span><span>Current analyzer score {currentScore === null || currentScore === undefined ? "—" : `${currentScore >= 0 ? "+" : ""}${Math.round(currentScore)}`} · CVaR 95% {formatPercent(risk.cvar95)}</span></div>
  </div>;
}

function ChartControls({ mode, range, onModeChange, onRangeChange }: { mode: ChartMode; range: ChartRange; onModeChange: (mode: ChartMode) => void; onRangeChange: (range: ChartRange) => void }) {
  return <div className="chart-controls" aria-label="Chart controls"><div className="chart-control-group"><span>View</span>{([ ["candles", "Candles"], ["line", "Line"], ["area", "Area"], ["returns", "Indexed returns"] ] as [ChartMode, string][]).map(([value, label]) => <button className={mode === value ? "selected" : ""} key={value} onClick={() => onModeChange(value)}>{label}</button>)}</div><div className="chart-control-group"><span>History</span>{([ ["1W", "1W"], ["1M", "1M"], ["3M", "3M"], ["6M", "6M"], ["1Y", "1Y"], ["ALL", "All"] ] as [ChartRange, string][]).map(([value, label]) => <button className={range === value ? "selected" : ""} key={value} onClick={() => onRangeChange(value)}>{label}</button>)}</div><small>Hover the plot for OHLCV · ranges are sliced from the sourced daily history</small></div>;
}

function CandlesChart({ history, signals = [], levels, mode = "candles", range = "6M" }: { history: History | null; signals?: ChartSignal[]; levels?: { support?: number | null; resistance?: number | null; target?: number | null }; mode?: ChartMode; range?: ChartRange }) {
  const [hoverIndex, setHoverIndex] = useState<number | null>(null);
  if (!history) return <div className="market-empty-chart"><RefreshCw size={24} className="spin-icon" /><strong>Loading historical data</strong><span>Fetching the selected security’s daily candles from the configured providers.</span></div>;
  if (history.candles.length === 0) return <div className="market-empty-chart"><CandlestickChart size={24} /><strong>Historical data unavailable</strong><span>{history.provenance.message ?? "Connect Futu OpenD or enable clearly labelled development fixtures."}</span><FreshnessBadge provenance={history.provenance} /></div>;
  const rangeCount: Record<ChartRange, number> = { "1W": 5, "1M": 22, "3M": 63, "6M": 126, "1Y": 252, ALL: history.candles.length };
  const candles = history.candles.slice(-Math.min(rangeCount[range], history.candles.length));
  const firstClose = candles[0]?.close ?? 1;
  const chartValue = (candle: Candle) => mode === "returns" ? (candle.close / firstClose) * 100 : candle.close;
  const chartHigh = (candle: Candle) => mode === "returns" ? (candle.high / firstClose) * 100 : candle.high;
  const chartLow = (candle: Candle) => mode === "returns" ? (candle.low / firstClose) * 100 : candle.low;
  const signalPrices = mode === "returns" ? signals.map((signal) => (signal.price / firstClose) * 100) : signals.map((signal) => signal.price);
  const levelPrices = mode === "returns" ? [levels?.support, levels?.resistance, levels?.target].filter((value): value is number => value !== null && value !== undefined && Number.isFinite(value)).map((value) => (value / firstClose) * 100) : [levels?.support, levels?.resistance, levels?.target].filter((value): value is number => value !== null && value !== undefined && Number.isFinite(value));
  const max = Math.max(...candles.map(chartHigh), ...signalPrices, ...levelPrices);
  const min = Math.min(...candles.map(chartLow), ...signalPrices, ...levelPrices);
  const maxVolume = Math.max(...candles.map((item) => item.volume), 1);
  const plotRange = Math.max(max - min, 0.0001);
  const x = (index: number) => 14 + (index / Math.max(candles.length - 1, 1)) * 572;
  const y = (value: number) => 18 + (1 - (value - min) / plotRange) * 212;
  const visibleSignals = signals.filter((signal) => candles.some((candle) => candle.timestamp === signal.timestamp));
  const visibleLevels = [
    levels?.support ? { label: "SUPPORT", value: mode === "returns" ? (levels.support / firstClose) * 100 : levels.support, className: "chart-level-support" } : null,
    levels?.resistance ? { label: "RESISTANCE", value: mode === "returns" ? (levels.resistance / firstClose) * 100 : levels.resistance, className: "chart-level-resistance" } : null,
    levels?.target ? { label: "REFERENCE TARGET", value: mode === "returns" ? (levels.target / firstClose) * 100 : levels.target, className: "chart-level-target" } : null,
  ].filter((level): level is { label: string; value: number; className: string } => level !== null && Number.isFinite(level.value));
  const movingAveragePath = (period: number) => candles.map((_, index) => {
    const average = localSma(candles.slice(0, index + 1).map(chartValue), period);
    return average === null ? null : `${x(index)},${y(average)}`;
  }).filter((point): point is string => point !== null).join(" ");
  const closePath = candles.map((item, index) => `${x(index)},${y(chartValue(item))}`).join(" ");
  const areaPath = candles.length ? `M${x(0)} 245 L${candles.map((item, index) => `${x(index)} ${y(chartValue(item))}`).join(" L")} L${x(candles.length - 1)} 245 Z` : "";
  const hovered = hoverIndex === null ? null : candles[hoverIndex];
  return <div className="market-chart-wrap chart-with-signals"><svg className="market-chart" viewBox="0 0 600 270" preserveAspectRatio="none" role="img" aria-label={`${history.symbol} ${mode} chart with research buy and sell markers`} onMouseLeave={() => setHoverIndex(null)} onMouseMove={(event) => { const bounds = event.currentTarget.getBoundingClientRect(); const index = Math.round(((event.clientX - bounds.left) / Math.max(bounds.width, 1)) * (candles.length - 1)); setHoverIndex(Math.max(0, Math.min(candles.length - 1, index))); }}><path className="market-grid" d="M0 18H600 M0 71H600 M0 124H600 M0 177H600 M0 230H600" />{visibleLevels.map((level) => <g key={level.label}><path className={level.className} d={`M0 ${y(level.value)}H600`} /><text className="chart-level-label" x="8" y={Math.max(12, y(level.value) - 4)}>{level.label} {mode === "returns" ? `${formatNumber(level.value - 100)}%` : formatNumber(level.value)}</text></g>)}{mode === "area" && <path className="chart-area-fill" d={areaPath} />}{mode === "candles" ? candles.map((item, index) => { const positive = item.close >= item.open; const cx = x(index); const bodyTop = y(Math.max(item.open, item.close)); const bodyBottom = y(Math.min(item.open, item.close)); return <g key={item.timestamp}><line x1={cx} x2={cx} y1={y(chartHigh(item))} y2={y(chartLow(item))} className={positive ? "market-candle-up" : "market-candle-down"} /><rect x={cx - 2.4} y={bodyTop} width="4.8" height={Math.max(2, bodyBottom - bodyTop)} className={positive ? "market-candle-up-fill" : "market-candle-down-fill"} /><rect x={cx - 2.2} y={245 - (item.volume / maxVolume) * 28} width="4.4" height={(item.volume / maxVolume) * 28} className="market-volume" /></g>; }) : <>{mode === "area" && <polyline className="chart-line-primary" points={closePath} />}<polyline className="chart-line-primary" points={closePath} /></>}{mode === "candles" && <><polyline className="chart-sma-fast" points={movingAveragePath(20)} /><polyline className="chart-sma-slow" points={movingAveragePath(50)} /></>}{visibleSignals.map((signal) => { const index = candles.findIndex((candle) => candle.timestamp === signal.timestamp); const cx = x(index); const markerY = signal.action === "BUY" ? Math.min(224, y(mode === "returns" ? (signal.price / firstClose) * 100 : signal.price) + 12) : Math.max(25, y(mode === "returns" ? (signal.price / firstClose) * 100 : signal.price) - 12); return <g key={`${signal.timestamp}-${signal.action}`} className={`chart-signal-marker ${signal.action.toLowerCase()}`}><title>{`${signal.action} · ${formatNumber(signal.price)} · ${signal.reason}`}</title><circle cx={cx} cy={markerY} r="7" /><text x={cx} y={markerY + 3} textAnchor="middle">{signal.action === "BUY" ? "B" : "S"}</text></g>; })}<path className="market-last-line" d={`M0 ${y(chartValue(candles[candles.length - 1]))}H600`} />{hovered && hoverIndex !== null && <><path className="chart-crosshair" d={`M${x(hoverIndex)} 18V245`} /><circle className="chart-hover-dot" cx={x(hoverIndex)} cy={y(chartValue(hovered))} r="3" /></>}</svg>{hovered && <div className="chart-hover-card"><strong>{new Date(hovered.timestamp * 1000).toLocaleDateString()}</strong><span>O {formatNumber(hovered.open)} · H {formatNumber(hovered.high)} · L {formatNumber(hovered.low)} · C {formatNumber(hovered.close)}</span><span>Volume {formatCompact(hovered.volume)} · {mode === "returns" ? `${formatNumber((hovered.close / firstClose - 1) * 100)}% indexed return` : "daily close"}</span></div>}<div className="chart-legend"><span><i className="legend-candle-up" /> {mode === "candles" ? "up / down candles" : mode === "returns" ? "indexed close" : `${mode} close`}</span>{mode === "candles" && <><span><i className="legend-sma-fast" /> SMA 20</span><span><i className="legend-sma-slow" /> SMA 50</span></>}<span><i className="legend-buy" /> B entry condition</span><span><i className="legend-sell" /> S reduce / exit condition</span></div><div className="market-chart-footer"><span>{mode === "returns" ? "Indexed return · base 100" : history.interval} · {range} · {candles.length} sessions · hover for details</span><span><FreshnessBadge provenance={history.provenance} /></span></div></div>;
}

function EquityCurve({ values }: { values: number[] }) {
  if (values.length < 2) return <div className="market-table-empty">At least 60 daily candles are required for the backtest.</div>;
  const min = Math.min(...values);
  const max = Math.max(...values);
  const range = Math.max(max - min, 0.0001);
  const points = values.map((value, index) => `${(index / (values.length - 1)) * 600},${18 + (1 - (value - min) / range) * 190}`).join(" ");
  return <div className="equity-curve-wrap"><svg className="equity-curve" viewBox="0 0 600 225" preserveAspectRatio="none" role="img" aria-label="Backtest equity curve"><path className="market-grid" d="M0 18H600 M0 65H600 M0 113H600 M0 160H600 M0 208H600" /><polyline points={points} className="equity-curve-line" /></svg><div className="market-chart-footer"><span>Equity curve · close-to-close signal execution</span><span>{values.length - 1} sessions</span></div></div>;
}

function PerformanceChart({ data, loading }: { data: PerformanceChartData; loading: boolean }) {
  const [hoverIndex, setHoverIndex] = useState<number | null>(null);
  if (loading) return <div className="market-empty-chart"><RefreshCw size={24} className="spin-icon" /><strong>Loading portfolio history</strong><span>Fetching dated trade history and public benchmark series.</span></div>;
  if (data.dates.length < 2) return <div className="market-table-empty">Add at least one dated trade with available price history to plot holdings over time.</div>;
  const allValues = [
    ...data.portfolioIndex,
    ...data.benchmarks.flatMap((series) => series.values.filter((value): value is number => value !== null)),
  ].filter(Number.isFinite);
  const min = Math.min(...allValues, 95);
  const max = Math.max(...allValues, 105);
  const plotRange = Math.max(max - min, 0.0001);
  const x = (index: number) => 14 + (index / Math.max(data.dates.length - 1, 1)) * 572;
  const y = (value: number) => 18 + (1 - (value - min) / plotRange) * 172;
  const lineSegments = (values: (number | null)[]) => {
    const segments: string[][] = [];
    values.forEach((value, index) => {
      if (value === null || !Number.isFinite(value)) {
        if (segments.length && segments.at(-1)?.length) segments.push([]);
        return;
      }
      if (!segments.length) segments.push([]);
      const current = segments.at(-1);
      if (current) current.push(`${x(index)},${y(value)}`);
    });
    return segments.filter((segment) => segment.length > 1);
  };
  const series = [
    { label: "Portfolio · indexed", values: data.portfolioIndex, className: "performance-line-portfolio", history: null as History | null },
    ...data.benchmarks.map((item, index) => ({ label: item.benchmark.label, values: item.values, className: `performance-line-benchmark performance-line-benchmark-${index % 6}`, history: item.history })),
  ];
  const hoveredDate = hoverIndex === null ? null : data.dates[hoverIndex];
  return <div className="performance-chart-wrap"><div className="performance-chart-note"><span>Base 100 at the start of the selected period · portfolio return is time-weighted around dated trade cash flows</span><span>Portfolio value: {hoverIndex === null ? formatNumber(data.portfolioValues.at(-1)) : formatNumber(data.portfolioValues[hoverIndex])} HKD</span></div><svg className="performance-chart" viewBox="0 0 600 210" preserveAspectRatio="none" role="img" aria-label="Portfolio holdings performance compared with selected benchmarks" onMouseLeave={() => setHoverIndex(null)} onMouseMove={(event) => { const bounds = event.currentTarget.getBoundingClientRect(); const index = Math.round(((event.clientX - bounds.left) / Math.max(bounds.width, 1)) * (data.dates.length - 1)); setHoverIndex(Math.max(0, Math.min(data.dates.length - 1, index))); }}><path className="market-grid" d="M0 18H600 M0 61H600 M0 104H600 M0 147H600 M0 190H600" /><path className="performance-baseline" d={`M0 ${y(100)}H600`} />{series.map((item) => lineSegments(item.values).map((points, segmentIndex) => <polyline key={`${item.label}-${segmentIndex}`} points={points.join(" ")} className={item.className} />))}{hoverIndex !== null && <path className="chart-crosshair" d={`M${x(hoverIndex)} 18V190`} />}{hoverIndex !== null && <circle className="chart-hover-dot performance-hover-dot" cx={x(hoverIndex)} cy={y(data.portfolioIndex[hoverIndex])} r="3" />}</svg>{hoveredDate && <div className="performance-hover-card"><strong>{hoveredDate}</strong><span>Portfolio {formatNumber(data.portfolioIndex[hoverIndex ?? 0] - 100)}% · {formatNumber(data.portfolioValues[hoverIndex ?? 0])} HKD holdings</span>{data.benchmarks.map((item) => <span key={item.benchmark.symbol}>{item.benchmark.label} {item.values[hoverIndex ?? 0] === null ? "unavailable" : formatNumber((item.values[hoverIndex ?? 0] ?? 100) - 100) + "%"}</span>)}</div>}<div className="performance-chart-legend">{series.map((item) => <span key={item.label} className={!item.history && item.label !== "Portfolio · indexed" ? "benchmark-unavailable" : ""}><i className={item.className} /> {item.label}{item.history && <small> · {item.history.provenance.freshness}</small>}</span>)}</div><div className="market-chart-footer"><span>{data.dates[0]} → {data.dates.at(-1)} · {data.dates.length} sessions · hover for details</span><span>Index 100 = selected-period start</span></div></div>;
}

function DrawdownCurve({ values }: { values: number[] }) {
  if (values.length < 2) return <div className="market-table-empty">Drawdown needs a completed simulation.</div>;
  const min = Math.min(...values, 0);
  const range = Math.max(Math.abs(min), 0.0001);
  const points = values.map((value, index) => `${(index / (values.length - 1)) * 600},${18 + (Math.abs(value) / range) * 190}`).join(" ");
  return <div className="equity-curve-wrap drawdown-curve-wrap"><svg className="equity-curve" viewBox="0 0 600 225" preserveAspectRatio="none" role="img" aria-label="Backtest drawdown curve"><path className="market-grid" d="M0 18H600 M0 65H600 M0 113H600 M0 160H600 M0 208H600" /><polyline points={points} className="drawdown-curve-line" /></svg><div className="market-chart-footer"><span>Underwater curve · peak-to-trough loss</span><span>{formatPercent(min)}</span></div></div>;
}

function Metric({ label, value, tone }: { label: string; value: string; tone?: "positive" | "negative" }) {
  return <div className="market-metric"><span>{label}</span><strong className={tone ? `market-${tone}` : ""}>{value}</strong></div>;
}

function ResearchFacts({ facts, emptyLabel }: { facts: ResearchFact[]; emptyLabel: string }) {
  if (!facts.length) return <div className="market-table-empty">{emptyLabel}</div>;
  return <div className="source-fact-list">{facts.map((fact) => <div key={`${fact.label}-${fact.period ?? ""}`}><span>{fact.label}{fact.period && <small> · {fact.period}</small>}</span><strong title={fact.source}>{fact.value}</strong></div>)}</div>;
}

const HKEX_ISSUER_SEARCH_URL = "https://www.hkexnews.hk/search/titlesearch.xhtml?lang=en";

function HkexIssuerSearch({ symbol }: { symbol?: string }) {
  if (!symbol?.toUpperCase().startsWith("HK.")) return null;
  return <a className="secondary-button hkex-search-link" href={HKEX_ISSUER_SEARCH_URL} target="_blank" rel="noreferrer" aria-label="Open HKEX issuer announcements search" onClick={(event) => { event.preventDefault(); void invoke("markets_open_hkex").catch(() => window.open(HKEX_ISSUER_SEARCH_URL, "_blank", "noopener,noreferrer")); }}>Open HKEX issuer announcements</a>;
}

function ResearchItems({ items, emptyLabel }: { items: ResearchItem[]; emptyLabel: string }) {
  if (!items.length) return <div className="market-table-empty">{emptyLabel}</div>;
  return <div className="market-research-list">{items.map((item) => <article className="market-research-item" key={`${item.source}-${item.title}-${item.published_at ?? ""}`}><div><span className="market-research-source">{item.source}</span><h4>{item.url ? <a href={item.url} target="_blank" rel="noreferrer">{item.title}</a> : item.title}</h4><p>{item.detail}</p>{item.published_at && <small>{item.published_at}</small>}</div></article>)}</div>;
}

function TermGlossary() {
  return <section className="market-panel quant-glossary"><div className="market-panel-heading"><div><p className="eyebrow">QUANT GLOSSARY</p><h3>Plain-English definitions</h3></div><BookOpen size={16} /></div><dl>{QUANT_GLOSSARY.map(([term, definition]) => <div key={term}><dt>{term}</dt><dd>{definition}</dd></div>)}</dl></section>;
}

function QuantPipeline({ history, analyzer }: { history: History | null; analyzer?: SignalAnalyzer | null }) {
  const stages = [
    ["01", "Observed", history ? `${history.candles.length} candles · ${history.provenance.provider}` : "Waiting for a sourced candle set"],
    ["02", "Derived", "Returns, moving averages, RSI, MACD, volatility, tail risk"],
    ["03", "Validated", analyzer ? `${analyzer.trainingSessions} training · ${analyzer.validationSessions} unseen sessions` : "Walk-forward validation starts after enough history"],
    ["04", "Decision", analyzer ? `${analyzer.verdict} · ${analyzer.confidence}% confidence · research only` : "No decision until inputs are present"],
  ];
  return <section className="market-panel quant-pipeline"><div className="market-panel-heading"><div><p className="eyebrow">RESEARCH PIPELINE</p><h3>How JadX reaches a view</h3></div><span className="market-muted-label">Source → feature → validation → explanation</span></div><div className="quant-pipeline-grid">{stages.map(([number, label, detail]) => <article key={number}><span>{number}</span><strong>{label}</strong><p>{detail}</p></article>)}</div></section>;
}

function EvidenceLedger({ quote, history, research, macro, analyzer }: { quote: Quote | null; history: History | null; research: CompanyResearch | null; macro: MacroSnapshot | null; analyzer?: SignalAnalyzer | null }) {
  const rows = [
    ["Observed quote", quote ? `${quote.currency} ${formatNumber(quote.last)} · ${quote.provenance.provider}` : "Unavailable", quote?.provenance.freshness ?? "Unavailable"],
    ["Observed history", history ? `${history.candles.length} candles · ${history.interval}` : "Unavailable", history?.provenance.freshness ?? "Unavailable"],
    ["Public research", research ? `${research.financials.length} financial facts · ${research.news.length} news items` : "Unavailable", research?.provenance.freshness ?? "Unavailable"],
    ["Macro overlay", macro ? `Risk-on ${macro.risk_on_score === null || macro.risk_on_score === undefined ? "—" : `${macro.risk_on_score >= 0 ? "+" : ""}${(macro.risk_on_score * 100).toFixed(0)}`}` : "Unavailable", macro?.provenance.freshness ?? "Unavailable"],
    ["Model inference", analyzer ? `${analyzer.verdict} · score ${analyzer.score >= 0 ? "+" : ""}${analyzer.score}` : "Not run", "Derived"],
  ];
  return <section className="market-panel evidence-ledger"><div className="market-panel-heading"><div><p className="eyebrow">EVIDENCE LEDGER</p><h3>What is fact, what is derived</h3></div><span className="market-muted-label">Every row keeps its source state</span></div><div className="evidence-ledger-table"><div className="evidence-ledger-row evidence-ledger-head"><span>Layer</span><span>Value</span><span>State</span></div>{rows.map(([label, value, state]) => <div className="evidence-ledger-row" key={label}><strong>{label}</strong><span>{value}</span><b className={state === "Live" ? "market-positive" : state === "Unavailable" ? "market-negative" : ""}>{state}</b></div>)}</div></section>;
}

function TailRiskPanel({ risk }: { risk: RiskSnapshot }) {
  return <section className="market-panel tail-risk-panel"><div className="market-panel-heading"><div><p className="eyebrow">TAIL RISK</p><h3>What a bad day looked like</h3></div><span className="market-muted-label">Historical distribution · not a forecast</span></div><div className="market-stat-grid"><Metric label="VaR 95%" value={formatPercent(risk.var95)} tone="negative" /><Metric label="CVaR 95%" value={formatPercent(risk.cvar95)} tone="negative" /><Metric label="Annualized volatility" value={formatPercent(risk.volatility)} /><Metric label="Return skew" value={risk.skew === null ? "—" : risk.skew.toFixed(2)} /></div><p className="muted-copy tail-risk-copy">VaR marks the 5th-percentile daily loss. CVaR averages the losses beyond that threshold, so it describes the tail rather than hiding it in a single cutoff. Based on {risk.observations} observations and {risk.lossDays} negative sessions.</p></section>;
}

function InvestorLensPanel({ lens }: { lens: InvestorLens }) {
  const items: [string, string][] = [
    ["Thesis", lens.thesis],
    ["Evidence", lens.evidence],
    ["Valuation", lens.valuation],
    ["Position size", lens.positionSize],
    ["Risk / reward", lens.riskReward],
    ["Catalyst", lens.catalyst],
    ["Exit / re-entry", lens.exitReentry],
  ];
  return <section className="market-panel investor-lens-panel"><div className="market-panel-heading"><div><p className="eyebrow">INVESTOR LENS</p><h3>Thesis → evidence → valuation → action</h3></div><span className="market-muted-label">Small starter positions · no blind averaging down</span></div><div className="investor-lens-grid">{items.map(([label, detail]) => <article key={label}><strong>{label}</strong><p>{detail}</p></article>)}</div></section>;
}

function OutlookPanel({ outlook, quote }: { outlook: OutlookSnapshot; quote: Quote }) {
  return <section className="market-panel outlook-panel"><div className="market-panel-heading"><div><p className="eyebrow">OUTLOOK & REFERENCE TARGETS</p><h3>Short term vs long term</h3></div><span className="market-muted-label">Scenario ranges · not broker targets</span></div><div className="outlook-grid"><article><strong>1–3 month outlook</strong><p>{outlook.shortTerm}</p><b>{quote.currency} {formatNumber(outlook.shortTermTarget)}</b></article><article><strong>12–24 month outlook</strong><p>{outlook.longTerm}</p><b>{quote.currency} {formatNumber(outlook.longTermLow)} – {formatNumber(outlook.longTermHigh)}</b><small>Base case: {quote.currency} {formatNumber(outlook.longTermBase)}</small></article></div><p className="muted-copy outlook-method">{outlook.targetMethod}</p><div className="research-section-title"><Newspaper size={16} /><strong>Relevant news evidence</strong><span className="market-muted-label">Qwen can extract only what is relevant to the thesis</span></div><ResearchItems items={outlook.relevantNews} emptyLabel="No relevant public headline was returned; do not infer a catalyst." /></section>;
}

function ThematicCompaniesPanel({ companies }: { companies: ThematicCompany[] }) {
  return <section className="market-panel thematic-universe-panel"><div className="market-panel-heading"><div><p className="eyebrow">EXPANDED TECHNOLOGY UNIVERSE</p><h3>Beyond HSI / Hang Seng TECH</h3><span className="market-muted-label">AI infrastructure · semiconductors · autonomy · perception · batteries · robotics</span></div><span className="market-muted-label">Listed quotes plus research-only names</span></div><div className="thematic-company-grid">{companies.map((company) => <article key={company.symbol}><div><strong>{company.name}</strong><span>{company.symbol}</span></div><small>{company.themes.join(" · ")}</small><p>{company.note}</p><em>{company.listing_status}</em>{company.quote ? <><FreshnessBadge provenance={company.quote.provenance} /><button className="text-button" onClick={() => window.dispatchEvent(new CustomEvent("jadx-markets-open-symbol", { detail: company.quote?.symbol }))}>Open analyzer <ChevronRight size={13} /></button></> : <span className="market-muted-label">Research-only · no live price target</span>}</article>)}</div></section>;
}

function MarketsCopilot({ context, defaultPrompt, request, target }: { context: string; defaultPrompt: string; request?: MarketsCopilotRequest; target: MarketsCopilotRequest["target"] }) {
  const [prompt, setPrompt] = useState(defaultPrompt);
  const [answer, setAnswer] = useState("");
  const [model, setModel] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  let thematic: ThematicCompany[] = [];
  if (target === "universe") {
    try { thematic = (JSON.parse(context) as { thematic?: ThematicCompany[] }).thematic ?? []; } catch { thematic = []; }
  }

  async function ask(nextPrompt = prompt) {
    if (!nextPrompt.trim()) return;
    setLoading(true);
    setError("");
    try {
      const raw = JSON.parse(context) as Record<string, any>;
      const symbol = raw.quote?.symbol ?? raw.symbol ?? "";
      const savedEvidence = parseThesisEvidence(localStorage.getItem(THESIS_STORAGE_KEY));
      const compactContext = JSON.stringify({
        investment: thesisReviewContext(symbol, savedEvidence, raw.quote),
        quote: raw.quote,
        indicators: raw.indicators,
        valuation: raw.valuation,
        analyzer: raw.analyzer ? { verdict: raw.analyzer.verdict, score: raw.analyzer.score, confidence: raw.analyzer.confidence, factors: raw.analyzer.factors, outlook: raw.analyzer.outlook } : null,
        macro: raw.macro ? { as_of: raw.macro.as_of, risk_on_score: raw.macro.risk_on_score, provenance: raw.macro.provenance, factors: raw.macro.factors } : null,
        research: raw.research ? { provenance: raw.research.provenance, financials: raw.research.financials?.slice(0, 16), news: raw.research.news?.slice(0, 4) } : null,
        index: raw.index,
        candidates: raw.candidates?.map((item: any) => ({ symbol: item.symbol, name: item.name, last: item.last, freshness: item.freshness, score: item.analyzer?.score, confidence: item.analyzer?.confidence, news: item.news?.slice(0, 2) })),
      });
      const response = await invoke<{ answer: string; model: string; role: string }>("markets_qwen_review", { prompt: nextPrompt.trim(), context: compactContext });
      setAnswer(response.answer);
      setModel(response.model);
    } catch (cause) {
      setError(String(cause));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    if (request?.target === target) {
      setPrompt(request.prompt);
      void ask(request.prompt);
    }
  }, [request?.nonce, target]);

  return <>{thematic.length > 0 && <ThematicCompaniesPanel companies={thematic} />}<section className="market-panel markets-copilot"><div className="market-panel-heading"><div><p className="eyebrow">LOCAL QWEN ROLE</p><h3>Markets Quant Copilot</h3><span className="market-muted-label">Embedded in Markets · facts first · no order access</span></div><Sparkles size={16} /></div><div className="markets-copilot-form"><textarea value={prompt} onChange={(event) => setPrompt(event.target.value)} rows={2} placeholder="Ask the Markets Copilot to explain the signal, compare candidates, or stress-test the thesis…" /><button className="primary-button" onClick={() => void ask()} disabled={loading || !prompt.trim()}>{loading ? <><RefreshCw size={14} className="spin-icon" /> Reviewing…</> : <><Sparkles size={14} /> Run Qwen review</>}</button></div>{error && <div className="markets-copilot-error">Qwen review unavailable: {error}</div>}{answer && <div className="markets-copilot-answer"><div className="markets-copilot-meta">{model || "Local Qwen"} · Markets Quant Copilot</div><ReactMarkdown remarkPlugins={[remarkGfm]}>{answer}</ReactMarkdown></div>}</section></>;
}

function MarketsWorkspace({ onOpenSettings, onError }: MarketsWorkspaceProps) {
  const [view, setView] = useState<View>("dashboard");
  const [dashboard, setDashboard] = useState<Dashboard | null>(null);
  const [selectedSymbol, setSelectedSymbol] = useState("");
  const [search, setSearch] = useState("");
  const [quote, setQuote] = useState<Quote | null>(null);
  const [history, setHistory] = useState<History | null>(null);
  const [research, setResearch] = useState<CompanyResearch | null>(null);
  const [macroSnapshot, setMacroSnapshot] = useState<MacroSnapshot | null>(null);
  const [companyTab, setCompanyTab] = useState<CompanyTab>("overview");
  const [universe, setUniverse] = useState<UniverseSnapshot | null>(null);
  const [universeTab, setUniverseTab] = useState<"hsi" | "hstech">("hstech");
  const [universeSearch, setUniverseSearch] = useState("");
  const [universeLoading, setUniverseLoading] = useState(false);
  const [universeCandidates, setUniverseCandidates] = useState<UniverseCandidate[]>([]);
  const [universeAnalysisTab, setUniverseAnalysisTab] = useState<"hsi" | "hstech">("hstech");
  const [universeAnalyzing, setUniverseAnalyzing] = useState(false);
  const [copilotRequest, setCopilotRequest] = useState<MarketsCopilotRequest | undefined>();
  const [loading, setLoading] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [watchlistId, setWatchlistId] = useState<string | null>(null);
  const [holdingForm, setHoldingForm] = useState({ account: "Manual", symbol: "", shares: "", averageCost: "" });
  const [tradeForm, setTradeForm] = useState({ account: "Manual", symbol: "", side: "BUY" as "BUY" | "SELL", quantity: "", price: "", commission: "0", tradeDate: new Date().toISOString().slice(0, 10), notes: "" });
  const [editingTradeId, setEditingTradeId] = useState<string | null>(null);
  const [performanceHistories, setPerformanceHistories] = useState<Record<string, History | null>>({});
  const [performanceLoading, setPerformanceLoading] = useState(false);
  const [performanceRange, setPerformanceRange] = useState<PerformanceRange>("1Y");
  const [selectedPerformanceBenchmarks, setSelectedPerformanceBenchmarks] = useState<string[]>(DEFAULT_PERFORMANCE_BENCHMARKS);
  const [recommendationSymbols, setRecommendationSymbols] = useState<string[]>([]);
  const [recommendationData, setRecommendationData] = useState<Record<string, RecommendationData>>({});
  const [recommendationInput, setRecommendationInput] = useState("");
  const [recommendationLoading, setRecommendationLoading] = useState(false);
  const [orderForm, setOrderForm] = useState({ side: "BUY", symbol: "", quantity: "100", limitPrice: "" });
  const [orderPreview, setOrderPreview] = useState<OrderPreview | null>(null);
  const [orders, setOrders] = useState<MarketOrder[]>([]);
  const [chartMode, setChartMode] = useState<ChartMode>("candles");
  const [chartRange, setChartRange] = useState<ChartRange>("6M");
  const [simulationConfig, setSimulationConfig] = useState<SimulationConfig>(DEFAULT_SIMULATION_CONFIG);
  const [simulationNonce, setSimulationNonce] = useState(0);
  const [thesisSymbol, setThesisSymbol] = useState("EXAMPLE");
  const [thesisEvidence, setThesisEvidence] = useState<ThesisEvidence[]>(() => {
    try { return parseThesisEvidence(localStorage.getItem(THESIS_STORAGE_KEY)); } catch { return []; }
  });

  function saveThesisEvidence(entries: ThesisEvidence[]) {
    try {
      localStorage.setItem(THESIS_STORAGE_KEY, JSON.stringify(entries));
      setThesisEvidence(entries);
      return true;
    } catch { onError("Could not save the thesis evidence locally. Your previous saved findings are unchanged."); return false; }
  }

  function openThesis(symbol: string) { setThesisSymbol(symbol); setView("thesis"); }

  const selectedWatchlist = useMemo(() => dashboard?.watchlists.find((item) => item.id === watchlistId) ?? dashboard?.watchlists[0], [dashboard?.watchlists, watchlistId]);
  const companySuggestions = useMemo(() => {
    const bySymbol = new Map<string, CompanySuggestion>();
    const add = (suggestion: CompanySuggestion) => { if (suggestion.symbol && !bySymbol.has(suggestion.symbol)) bySymbol.set(suggestion.symbol, suggestion); };
    COMMON_COMPANY_SUGGESTIONS.forEach(add);
    [...(dashboard?.indices ?? []), ...(dashboard?.watchlist_quotes ?? []), ...(dashboard?.portfolio ?? []).map((row) => row.quote), ...(dashboard?.trade_quotes ?? [])].forEach((item) => add({ symbol: item.symbol, name: item.name, detail: `${item.exchange} · ${item.industry ?? "market quote"}` }));
    [...(universe?.hsi ?? []), ...(universe?.hstech ?? [])].forEach((item) => add({ symbol: item.symbol, name: item.name, detail: `${item.exchange} · index constituent` }));
    (universe?.thematic ?? []).forEach((item) => add({ symbol: item.symbol, name: item.name, detail: item.themes.join(" · ") }));
    return [...bySymbol.values()];
  }, [dashboard, universe]);
  const matchingCompanySuggestions = useMemo(() => {
    const needle = recommendationInput.trim().toLowerCase();
    if (!needle) return [];
    return companySuggestions.filter((item) => [item.symbol, item.name, ...(item.aliases ?? [])].some((value) => value.toLowerCase().includes(needle))).slice(0, 7);
  }, [companySuggestions, recommendationInput]);
  const matchingSearchSuggestions = useMemo(() => {
    const needle = search.trim().toLowerCase();
    if (!needle) return [];
    return companySuggestions.filter((item) => [item.symbol, item.name, ...(item.aliases ?? [])].some((value) => value.toLowerCase().includes(needle))).slice(0, 7);
  }, [companySuggestions, search]);
  const selectedName = quote?.name ?? "No security selected";
  const analytics = useMemo(() => deriveAnalytics(history), [history]);
  const backtest = useMemo(() => runStrategyBacktest(history, simulationConfig), [history, simulationConfig, simulationNonce]);
  const analyzer = useMemo(() => runSignalAnalyzer(history, quote, macroSnapshot, research), [history, quote, macroSnapshot, research]);
  const valuation = useMemo(() => valuationSnapshot(quote, research), [quote, research]);
  const performanceQuotes = useMemo(() => {
    const bySymbol = new Map<string, Quote>();
    [...(dashboard?.indices ?? []), ...(dashboard?.watchlist_quotes ?? []), ...(dashboard?.portfolio ?? []).map((row) => row.quote), ...(dashboard?.trade_quotes ?? [])].forEach((item) => bySymbol.set(item.symbol, item));
    return [...bySymbol.values()];
  }, [dashboard]);
  const tradePerformance = useMemo(() => calculateTradePerformance(dashboard?.trades ?? [], performanceQuotes), [dashboard?.trades, performanceQuotes]);
  const thesisPositions = useMemo(() => reconcileThesisPositions((dashboard?.portfolio ?? []).map((row) => row.holding), tradePerformance), [dashboard?.portfolio, tradePerformance]);
  const performanceChartData = useMemo(() => buildPerformanceChartData(dashboard?.trades ?? [], performanceHistories, selectedPerformanceBenchmarks, performanceRange), [dashboard?.trades, performanceHistories, selectedPerformanceBenchmarks, performanceRange]);
  const recommendationKey = useMemo(() => [...(dashboard?.portfolio ?? []).map((row) => row.holding.symbol), ...(dashboard?.trades ?? []).map((trade) => trade.symbol)].sort().join("|"), [dashboard?.portfolio, dashboard?.trades]);

  async function loadDashboard(showSpinner = false) {
    if (showSpinner) setRefreshing(true);
    try {
      const next = await invoke<Dashboard>("markets_dashboard");
      setDashboard(next);
      if (!watchlistId && next.watchlists[0]) setWatchlistId(next.watchlists[0].id);
    } catch (cause) { onError(String(cause)); }
    finally { setRefreshing(false); }
  }

  async function loadSecurity(input: string) {
    if (!input.trim()) return;
    setLoading(true);
    try {
      const nextQuote = await invoke<Quote>("markets_quote", { symbol: input });
      setQuote(nextQuote);
      setSelectedSymbol(nextQuote.symbol);
      const [nextHistory, nextResearch, nextMacro] = await Promise.all([
        invoke<History>("markets_history", { symbol: nextQuote.symbol, count: 180 }),
        invoke<CompanyResearch>("markets_company_research", { symbol: nextQuote.symbol }),
        invoke<MacroSnapshot>("markets_macro"),
      ]);
      setHistory(nextHistory);
      setResearch(nextResearch);
      setMacroSnapshot(nextMacro);
      setCompanyTab("overview");
      setSearch("");
      setView("companies");
    } catch (cause) { onError(String(cause)); }
    finally { setLoading(false); }
  }

  async function addRecommendationCompany(inputOverride?: string) {
    const input = (inputOverride ?? recommendationInput).trim();
    if (!input) return;
    try {
      const nextQuote = await invoke<Quote>("markets_quote", { symbol: input });
      setRecommendationSymbols((current) => current.includes(nextQuote.symbol) ? current : [...current, nextQuote.symbol]);
      setRecommendationInput("");
      setView("recommendations");
    } catch (cause) { onError(String(cause)); }
  }

  function removeRecommendationCompany(symbol: string) {
    setRecommendationSymbols((current) => current.filter((item) => item !== symbol));
    setRecommendationData((current) => {
      const next = { ...current };
      delete next[symbol];
      return next;
    });
  }

  useEffect(() => {
    const openSymbol = (event: Event) => {
      const symbol = (event as CustomEvent<string>).detail;
      if (symbol) void loadSecurity(symbol);
    };
    window.addEventListener("jadx-markets-open-symbol", openSymbol);
    return () => window.removeEventListener("jadx-markets-open-symbol", openSymbol);
  }, []);

  async function loadUniverse() {
    setUniverseLoading(true);
    try {
      const [nextUniverse, nextMacro] = await Promise.all([
        invoke<UniverseSnapshot>("markets_universe"),
        invoke<MacroSnapshot>("markets_macro"),
      ]);
      setUniverse(nextUniverse);
      setMacroSnapshot(nextMacro);
    } catch (cause) { onError(String(cause)); }
    finally { setUniverseLoading(false); }
  }

  async function analyzeBestUniverse(tab = universeTab) {
    if (!universe) return;
    setUniverseAnalyzing(true);
    setUniverseAnalysisTab(tab);
    try {
      const eligible = (tab === "hsi" ? universe.hsi : universe.hstech)
        .filter((item) => item.last !== null && item.last !== undefined && item.provenance.freshness !== "Unavailable");
      const momentumRanked = [...eligible].sort((left, right) => {
          const leftMove = left.change_pct ?? -Infinity;
          const rightMove = right.change_pct ?? -Infinity;
          const leftQuality = left.provenance.freshness === "Live" ? 0.02 : 0;
          const rightQuality = right.provenance.freshness === "Live" ? 0.02 : 0;
          return rightMove + rightQuality - leftMove - leftQuality;
        });
      const themeRanked = [...eligible].sort((left, right) => growthThemeEvidence(right).score - growthThemeEvidence(left).score);
      const quoteMap = new Map<string, Quote>();
      [...momentumRanked.slice(0, 6), ...themeRanked.slice(0, 10)].forEach((item) => quoteMap.set(item.symbol, item));
      const quotes = [...quoteMap.values()].slice(0, 14);
      const candidates = await Promise.all(quotes.map(async (item) => {
        try {
          const [nextHistory, nextResearch] = await Promise.all([
            invoke<History>("markets_history", { symbol: item.symbol, count: 180 }),
            invoke<CompanyResearch>("markets_company_research", { symbol: item.symbol }),
          ]);
          const nextAnalyzer = runSignalAnalyzer(nextHistory, item, macroSnapshot, nextResearch);
          const quoteMomentum = (item.change_pct ?? 0) * 100;
          const theme = growthThemeEvidence(item, nextHistory.candles);
          const fundamentals = fundamentalSnapshot(nextResearch);
          const commercial = commercialEvidence(nextResearch);
          const sourceQuality = item.provenance.freshness === "Live" ? 2 : item.provenance.freshness === "Recent" ? 1 : -2;
          const modelQuality = nextAnalyzer ? 0.86 + nextAnalyzer.confidence / 500 : 0;
          const styleFit = theme.score * 3 + fundamentals.score * 4 + commercial.score * 2;
          const score = clamp((nextAnalyzer?.score ?? 0) * modelQuality + quoteMomentum * 0.06 + styleFit + sourceQuality, -100, 100);
          return { quote: item, analyzer: nextAnalyzer, score, history: nextHistory, research: nextResearch } satisfies UniverseCandidate;
        } catch {
          const theme = growthThemeEvidence(item);
          return { quote: item, analyzer: null, score: clamp((item.change_pct ?? 0) * 8 + theme.score * 8, -100, 100), history: null, research: null } satisfies UniverseCandidate;
        }
      }));
      setUniverseCandidates(candidates.sort((left, right) => right.score - left.score));
    } finally {
      setUniverseAnalyzing(false);
    }
  }

  useEffect(() => { void loadDashboard(); void loadSecurity("700"); }, []);

  useEffect(() => {
    if (view !== "dashboard" && view !== "watchlist" && view !== "radar" && view !== "portfolio" && view !== "performance") return;
    const interval = window.setInterval(() => void loadDashboard(), dashboard?.health.tcp_reachable ? 15_000 : 30_000);
    return () => window.clearInterval(interval);
  }, [view, dashboard?.health.tcp_reachable]);

  useEffect(() => {
    if (view !== "performance" || !dashboard) return;
    let cancelled = false;
    const symbols = [...new Set([...dashboard.trades.map((trade) => trade.symbol), ...PERFORMANCE_BENCHMARKS.map((benchmark) => benchmark.symbol)])];
    setPerformanceLoading(true);
    void Promise.all(symbols.map(async (symbol) => {
      try {
        return [symbol, await invoke<History>("markets_history", { symbol, count: 520 })] as const;
      } catch {
        return [symbol, null] as const;
      }
    })).then((entries) => {
      if (!cancelled) setPerformanceHistories(Object.fromEntries(entries));
    }).finally(() => {
      if (!cancelled) setPerformanceLoading(false);
    });
    return () => { cancelled = true; };
  }, [view, dashboard?.trades?.map((trade) => `${trade.id}:${trade.symbol}`).join("|")]);

  useEffect(() => {
    if (!recommendationKey) return;
    const heldSymbols = recommendationKey.split("|").filter(Boolean);
    setRecommendationSymbols((current) => current.length ? current : [...new Set([...INVESTMENT_THESES.map((item) => item.symbol), ...heldSymbols])]);
  }, [recommendationKey]);

  useEffect(() => {
    if (view !== "recommendations" || !recommendationSymbols.length) return;
    let cancelled = false;
    setRecommendationLoading(true);
    const macroPromise = invoke<MacroSnapshot>("markets_macro").catch(() => null);
    void Promise.all(recommendationSymbols.map(async (symbol) => {
      try {
        const [nextQuote, nextHistory, nextResearch, nextMacro] = await Promise.all([
          invoke<Quote>("markets_quote", { symbol }),
          invoke<History>("markets_history", { symbol, count: 180 }),
          invoke<CompanyResearch>("markets_company_research", { symbol }),
          macroPromise,
        ]);
        const nextValuation = valuationSnapshot(nextQuote, nextResearch);
        return [symbol, { quote: nextQuote, history: nextHistory, research: nextResearch, macro: nextMacro, analyzer: runSignalAnalyzer(nextHistory, nextQuote, nextMacro, nextResearch), valuation: nextValuation } satisfies RecommendationData] as const;
      } catch (cause) {
        return [symbol, { quote: null, history: null, research: null, macro: null, analyzer: null, valuation: null, error: String(cause) } satisfies RecommendationData] as const;
      }
    })).then((entries) => {
      if (!cancelled) setRecommendationData(Object.fromEntries(entries));
    }).finally(() => {
      if (!cancelled) setRecommendationLoading(false);
    });
    return () => { cancelled = true; };
  }, [view, recommendationSymbols.join("|")]);

  useEffect(() => {
    if (!selectedSymbol || view !== "companies") return;
    void (async () => {
      try {
        const [nextQuote, nextHistory, nextResearch, nextMacro] = await Promise.all([
          invoke<Quote>("markets_quote", { symbol: selectedSymbol }),
          invoke<History>("markets_history", { symbol: selectedSymbol, count: 180 }),
          invoke<CompanyResearch>("markets_company_research", { symbol: selectedSymbol }),
          invoke<MacroSnapshot>("markets_macro"),
        ]);
        setQuote(nextQuote);
        setHistory(nextHistory);
        setResearch(nextResearch);
        setMacroSnapshot(nextMacro);
      } catch (cause) { onError(String(cause)); }
    })();
  }, [selectedSymbol, view]);

  useEffect(() => {
    if (view !== "orders" && view !== "history") return;
    void invoke<MarketOrder[]>("markets_orders")
      .then(setOrders)
      .catch((cause) => onError(String(cause)));
  }, [view]);

  useEffect(() => {
    if (view === "universe") void loadUniverse();
  }, [view]);

  async function saveHolding() {
    try {
      await invoke("markets_save_holding", { account: holdingForm.account, symbol: holdingForm.symbol, shares: Number(holdingForm.shares), averageCost: Number(holdingForm.averageCost), currency: "HKD", source: "manual" });
      await loadDashboard(true);
      setView("portfolio");
    } catch (cause) { onError(String(cause)); }
  }

  async function saveTrade() {
    try {
      const payload = {
        account: tradeForm.account,
        symbol: tradeForm.symbol,
        side: tradeForm.side,
        quantity: Number(tradeForm.quantity),
        price: Number(tradeForm.price),
        commission: Number(tradeForm.commission),
        tradeDate: tradeForm.tradeDate,
        notes: tradeForm.notes,
        source: "manual",
      };
      if (editingTradeId) {
        await invoke("markets_save_trade", { ...payload, id: editingTradeId });
      } else {
        await invoke("markets_save_trade", payload);
      }
      await loadDashboard(true);
      setEditingTradeId(null);
      setView("transactions");
    } catch (cause) { onError(String(cause)); }
  }

  function openNewTradeForm() {
    setEditingTradeId(null);
    setTradeForm({ account: "Manual", symbol: "700", side: "BUY", quantity: "100", price: "0", commission: "0", tradeDate: new Date().toISOString().slice(0, 10), notes: "" });
    setView("holdings");
  }

  function editTrade(trade: PortfolioTrade) {
    setEditingTradeId(trade.id);
    setTradeForm({
      account: trade.account,
      symbol: trade.symbol,
      side: trade.side,
      quantity: String(trade.quantity),
      price: String(trade.price),
      commission: String(trade.commission),
      tradeDate: trade.trade_date,
      notes: trade.notes,
    });
    setView("holdings");
  }

  function cancelTradeForm() {
    setEditingTradeId(null);
    setView("transactions");
  }

  async function deleteTrade(trade: PortfolioTrade) {
    const description = `${trade.side} ${formatNumber(trade.quantity, 0)} ${trade.symbol} on ${trade.trade_date}`;
    if (!window.confirm(`Remove ${description} from the local transaction history? This also removes it from performance calculations.`)) return;
    try {
      await invoke("markets_delete_trade", { id: trade.id });
      await loadDashboard(true);
    } catch (cause) { onError(String(cause)); }
  }

  async function toggleWatchlistSymbol(symbol: string) {
    if (!selectedWatchlist) return;
    const isInWatchlist = selectedWatchlist.symbols.includes(symbol);
    const symbols = isInWatchlist
      ? selectedWatchlist.symbols.filter((item) => item !== symbol)
      : [...selectedWatchlist.symbols, symbol];
    try {
      await invoke("markets_save_watchlist", { id: selectedWatchlist.id, name: selectedWatchlist.name, symbols });
      await loadDashboard(true);
    } catch (cause) { onError(String(cause)); }
  }

  async function addToWatchlist() {
    if (!quote || selectedWatchlist?.symbols.includes(quote.symbol)) return;
    await toggleWatchlistSymbol(quote.symbol);
  }

  async function previewOrder() {
    try {
      setOrderPreview(await invoke<OrderPreview>("markets_preview_order", { symbol: orderForm.symbol, side: orderForm.side, quantity: Number(orderForm.quantity), limitPrice: orderForm.limitPrice ? Number(orderForm.limitPrice) : null }));
    } catch (cause) { onError(String(cause)); }
  }

  async function submitPaperOrder() {
    if (!orderPreview) return;
    try {
      await invoke<string>("markets_paper_submit", { preview: orderPreview });
      setOrderPreview(null);
      onError("Paper order recorded locally.");
    } catch (cause) { onError(String(cause)); }
  }

  const renderUnavailable = (title: string, description: string) => <div className="market-placeholder"><div className="market-placeholder-icon"><ShieldCheck size={20} /></div><h2>{title}</h2><p>{description}</p><div className="market-source-note"><span>Source-aware status</span><strong>UNAVAILABLE</strong><small>No provider response was substituted.</small></div><button className="secondary-button" onClick={onOpenSettings}>Open Markets settings</button></div>;

  function renderDashboard() {
    if (!dashboard) return <div className="market-loading"><RefreshCw size={18} className="spin-icon" /> Connecting to local Markets services…</div>;
    const portfolioValue = dashboard.portfolio.reduce((total, row) => total + (row.market_value ?? 0), 0);
    const portfolioPnl = dashboard.portfolio.reduce((total, row) => total + (row.unrealized_pnl ?? 0), 0);
    return <div className="markets-page"><div className="markets-page-heading"><div><p className="eyebrow">MARKET CONTEXT</p><h2>Dashboard</h2><p className="muted-copy">Hong Kong first · sourced facts only · {new Date(dashboard.retrieved_at).toLocaleTimeString()}</p></div><button className="secondary-button" onClick={() => void loadDashboard(true)} disabled={refreshing}><RefreshCw size={14} className={refreshing ? "spin-icon" : ""} /> Refresh</button></div><div className="market-index-grid">{dashboard.indices.map((item) => <QuoteCard key={item.symbol} quote={item} onSelect={() => void loadSecurity(item.symbol)} />)}</div><div className="markets-dashboard-grid"><section className="market-panel market-chart-panel"><div className="market-panel-heading"><div><p className="eyebrow">ACTIVE SECURITY</p><h3>{selectedName} <code>{selectedSymbol}</code></h3></div><button className="text-button" onClick={() => setView("companies")}>Open company <ChevronRight size={13} /></button></div><div className="market-active-quote"><strong>{quote?.last === undefined || quote?.last === null ? "Data unavailable" : `${quote.currency} ${formatNumber(quote.last)}`}</strong><span className={(quote?.change_pct ?? 0) >= 0 ? "market-positive" : "market-negative"}>{formatPercent(quote?.change_pct)} · <FreshnessBadge provenance={quote?.provenance ?? { provider: "none", retrieved_at: new Date().toISOString(), freshness: "Unavailable", realtime: false }} /></span></div><CandlesChart history={history} /></section><section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">PORTFOLIO</p><h3>Exposure snapshot</h3></div><button className="text-button" onClick={() => setView("portfolio")}>Open portfolio <ChevronRight size={13} /></button></div><div className="market-stat-grid"><Metric label="Market value" value={dashboard.portfolio.length ? `HKD ${formatNumber(portfolioValue)}` : "No holdings"} /><Metric label="Unrealized P/L" value={dashboard.portfolio.length ? `HKD ${formatNumber(portfolioPnl)}` : "—"} tone={portfolioPnl >= 0 ? "positive" : "negative"} /><Metric label="Accounts" value={new Set(dashboard.portfolio.map((row) => row.holding.account)).size.toString()} /><Metric label="Trading" value={dashboard.trading_mode === "OFF" ? "OFF" : dashboard.trading_mode} /></div>{dashboard.portfolio.length === 0 ? <p className="muted-copy market-panel-copy">No portfolio data is imported. Add a manual position or connect the Futu portfolio adapter in the next setup step.</p> : <div className="market-mini-list">{dashboard.portfolio.slice(0, 4).map((row) => <button key={row.holding.id} onClick={() => void loadSecurity(row.holding.symbol)}><span><strong>{row.holding.symbol}</strong><small>{row.holding.shares} shares · {row.holding.account}</small></span><b className={(row.unrealized_pnl ?? 0) >= 0 ? "market-positive" : "market-negative"}>{formatNumber(row.unrealized_pnl)}</b></button>)}</div>}</section></div><div className="markets-dashboard-grid lower"><section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">WATCHLIST MOVERS</p><h3>{selectedWatchlist?.name ?? "Main"}</h3></div><button className="text-button" onClick={() => setView("watchlist")}>Manage <ChevronRight size={13} /></button></div><QuoteTable quotes={dashboard.watchlist_quotes} onSelect={(symbol) => void loadSecurity(symbol)} /></section><section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">MARKET RADAR</p><h3>Deterministic events</h3></div><button className="text-button" onClick={() => setView("radar")}>View all <ChevronRight size={13} /></button></div>{dashboard.radar.length ? <div className="market-event-list">{dashboard.radar.slice(0, 4).map((event) => <div className="market-event" key={event.id}><span className={`event-severity ${event.severity}`} /><div><strong>{event.title}</strong><p>{event.detail}</p><FreshnessBadge provenance={event.provenance} /></div></div>)}</div> : <p className="muted-copy market-panel-copy">No threshold events from the currently available quotes.</p>}</section></div></div>;
  }

  function renderCompany() {
    if (!quote) return renderUnavailable("Choose a security", "Search for a supported exchange-qualified symbol.");
    const indicator = history?.indicators ?? {};
    const companyProvenance = research?.provenance ?? quote.provenance;
    const inWatchlist = selectedWatchlist?.symbols.includes(quote.symbol) ?? false;
    const peerQuotes = universe ? [...universe.hsi, ...universe.hstech].filter((item, index, items) => item.symbol !== quote.symbol && items.findIndex((candidate) => candidate.symbol === item.symbol) === index).slice(0, 6) : [];
    const chartRisk = riskSnapshot(history?.candles ?? []);
    const chartSignals = buildChartSignals(history?.candles ?? [], analyzer?.fastPeriod ?? 20, analyzer?.slowPeriod ?? 50, { quote, macro: macroSnapshot, valuation, research });
    const selectCompanyTab = (tab: CompanyTab) => setCompanyTab(tab);
    const companySurface = companyTab === "overview" ? <>
      <div className="company-grid"><section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">PRICE HISTORY</p><h3>Daily candles</h3></div><span className="market-muted-label">{history?.provenance.provider ?? "provider"} · no look-ahead</span></div><ChartControls mode={chartMode} range={chartRange} onModeChange={setChartMode} onRangeChange={setChartRange} /><CandlesChart history={history} signals={chartSignals} levels={{ support: analytics.support20, resistance: analytics.resistance20, target: analyzer?.outlook.shortTermTarget }} mode={chartMode} range={chartRange} /></section><section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">QUANT SNAPSHOT</p><h3>Deterministic technicals</h3></div><button className="text-button" onClick={() => selectCompanyTab("quant")}>Open factors <ChevronRight size={13} /></button></div><div className="market-stat-grid"><Metric label="1D return" value={formatPercent(indicator.return_1d)} tone={(indicator.return_1d ?? 0) >= 0 ? "positive" : "negative"} /><Metric label="20D return" value={formatPercent(indicator.return_20d)} tone={(indicator.return_20d ?? 0) >= 0 ? "positive" : "negative"} /><Metric label="RSI 14" value={formatNumber(indicator.rsi14)} /><Metric label="Volume / 20D" value={indicator.volume_vs_sma20 ? `${indicator.volume_vs_sma20.toFixed(2)}x` : "—"} /></div><div className="source-fact-list"><div><span>SMA 20</span><strong>{formatNumber(indicator.sma20)}</strong></div><div><span>EMA 20</span><strong>{formatNumber(indicator.ema20)}</strong></div><div><span>ATR 14</span><strong>{formatNumber(indicator.atr14)}</strong></div><div><span>Max drawdown</span><strong>{formatPercent(indicator.max_drawdown)}</strong></div></div></section></div>
      <SignalGuide signals={chartSignals} currentScore={analyzer?.score} risk={chartRisk} />
      <TailRiskPanel risk={chartRisk} />
      <EvidenceLedger quote={quote} history={history} research={research} macro={macroSnapshot} analyzer={analyzer} />
      <section className="market-panel company-data-panel"><div className="market-panel-heading"><div><p className="eyebrow">SOURCE STATUS</p><h3>Fundamentals, filings, news</h3><span className="market-muted-label">{companyProvenance.provider}</span></div><FreshnessBadge provenance={companyProvenance} /></div><div className="company-research-grid"><div><div className="research-section-title"><Building2 size={16} /><strong>Company profile</strong></div><ResearchFacts facts={research?.profile ?? []} emptyLabel="No profile response returned yet." /></div><div><div className="research-section-title"><CircleDollarSign size={16} /><strong>Latest financials</strong></div><ResearchFacts facts={research?.financials ?? []} emptyLabel="No financial statement response returned yet." /></div><div><div className="research-section-title"><Newspaper size={16} /><strong>Recent news</strong></div><ResearchItems items={(research?.news ?? []).slice(0, 3)} emptyLabel={companyProvenance.message ?? "No public RSS item returned; retry later or open the filing search."} /></div></div><div className="research-filing-strip"><FileText size={16} /><span>Official filings</span><ResearchItems items={research?.filings ?? []} emptyLabel="No official filing search link returned." /></div></section>
    </> : companyTab === "chart" ? <>
      <section className="market-panel chart-primary-panel"><div className="market-panel-heading"><div><p className="eyebrow">CHART</p><h3>{quote.symbol} price, volume & decision markers</h3></div><FreshnessBadge provenance={history?.provenance ?? quote.provenance} /></div><ChartControls mode={chartMode} range={chartRange} onModeChange={setChartMode} onRangeChange={setChartRange} /><CandlesChart history={history} signals={chartSignals} levels={{ support: analytics.support20, resistance: analytics.resistance20, target: analyzer?.outlook.shortTermTarget }} mode={chartMode} range={chartRange} /></section><SignalGuide signals={chartSignals} currentScore={analyzer?.score} risk={chartRisk} /><section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">CHART CONTEXT</p><h3>Levels used by the analyzer</h3></div><span className="market-muted-label">Daily inputs</span></div><div className="market-stat-grid"><Metric label="SMA 20" value={formatNumber(indicator.sma20)} /><Metric label="SMA 50" value={formatNumber(indicator.sma50)} /><Metric label="Support 20D" value={formatNumber(analytics.support20)} /><Metric label="Resistance 20D" value={formatNumber(analytics.resistance20)} /><Metric label="ATR 14" value={formatNumber(indicator.atr14)} /><Metric label="Ann. volatility" value={formatPercent(analytics.volatility20)} /></div></section><TailRiskPanel risk={chartRisk} />
    </> : companyTab === "financials" ? <>
      <section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">FINANCIALS</p><h3>Latest sourced statements</h3></div><FreshnessBadge provenance={companyProvenance} /></div><ResearchFacts facts={research?.financials ?? []} emptyLabel={companyProvenance.message ?? "No financial statement response returned."} /></section><section className="market-panel"><div className="research-section-title"><Building2 size={16} /><strong>Issuer profile</strong></div><ResearchFacts facts={research?.profile ?? []} emptyLabel="No profile response returned." /></section>
    </> : companyTab === "valuation" ? <>
      <div className="factor-grid"><Metric label="P/E" value={formatNumber(valuation.pe, 1)} /><Metric label="Earnings yield" value={formatPercent(valuation.earningsYield)} /><Metric label="P/B" value={formatNumber(valuation.priceToBook, 2)} /><Metric label="Market cap" value={formatCompact(quote.market_cap)} /><Metric label="Dividend yield" value={formatPercent(quote.dividend_yield ? quote.dividend_yield / 100 : null)} /><Metric label="Valuation score" value={`${valuation.score >= 0 ? "+" : ""}${Math.round(valuation.score * 100)}`} tone={valuation.score >= 0 ? "positive" : "negative"} /></div><section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">VALUATION CONTEXT</p><h3>Current market pricing</h3></div><FreshnessBadge provenance={quote.provenance} /></div><p className="muted-copy">{valuation.detail} P/E compares price with earnings; P/B compares price with book value; earnings yield is the inverse of P/E. These are context inputs and do not predict the next move.</p></section>
    </> : companyTab === "analysts" ? <>
      <section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">ANALYSTS</p><h3>JadX quant research view</h3></div><span className="market-muted-label">No broker target is invented</span></div>{analyzer ? <><div className="signal-grid"><article className="signal-card analyzer-verdict"><span>Research bias</span><strong className={analyzer.verdict === "BUY" ? "market-positive" : analyzer.verdict === "SELL" ? "market-negative" : ""}>{analyzer.verdict} · {analyzer.score >= 0 ? "+" : ""}{analyzer.score}</strong><p>{analyzer.confidence}% confidence · {analyzer.dataCoverage * 100 >= 99 ? "full" : "partial"} inputs.</p></article><article className="signal-card"><span>Market regime</span><strong>{macroSnapshot?.risk_on_score === null || macroSnapshot?.risk_on_score === undefined ? "—" : `${macroSnapshot.risk_on_score >= 0 ? "+" : ""}${(macroSnapshot.risk_on_score * 100).toFixed(0)}`}</strong><p>Live broad-market overlay.</p></article><article className="signal-card"><span>Valuation</span><strong>{formatNumber(valuation.pe, 1)} P/E</strong><p>{formatNumber(valuation.priceToBook, 2)} P/B · {formatPercent(valuation.earningsYield)} earnings yield.</p></article></div><div className="analyzer-factor-list">{analyzer.factors.map((factor) => <div className="analyzer-factor" key={factor.label}><div><strong>{factor.label}</strong><small>{factor.detail}</small></div><b className={factor.score > 0.08 ? "market-positive" : factor.score < -0.08 ? "market-negative" : ""}>{factor.score > 0 ? "+" : ""}{Math.round(factor.score * 100)}</b></div>)}</div><InvestorLensPanel lens={analyzer.investorLens} /><OutlookPanel outlook={analyzer.outlook} quote={quote} /><button className="secondary-button" onClick={() => setView("signals")}>Open full walk-forward analyzer <ChevronRight size={13} /></button></> : <div className="market-table-empty">At least 80 daily candles are required before the analyzer can validate this company.</div>}</section>
    </> : companyTab === "filings" ? <section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">FILINGS</p><h3>Official filing links</h3></div><FreshnessBadge provenance={companyProvenance} /></div><ResearchItems items={research?.filings ?? []} emptyLabel="No official filing search link returned." /></section> : companyTab === "news" ? <section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">NEWS</p><h3>Recent public headlines</h3></div><FreshnessBadge provenance={companyProvenance} /></div><ResearchItems items={research?.news ?? []} emptyLabel={companyProvenance.message ?? "No public RSS item returned."} /></section> : companyTab === "peers" ? <section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">PEERS</p><h3>Comparable HK technology names</h3></div><span className="market-muted-label">Click a peer to open its own analyzer</span></div>{peerQuotes.length ? <QuoteTable quotes={peerQuotes} onSelect={(symbol) => void loadSecurity(symbol)} /> : <div className="market-table-empty">Peer quotes load from the Universe surface. Open Universe once to populate this comparison set.</div>}</section> : <>
      <div className="factor-grid"><Metric label="1D momentum" value={formatPercent(indicator.return_1d)} /><Metric label="20D momentum" value={formatPercent(indicator.return_20d)} /><Metric label="RSI 14" value={formatNumber(indicator.rsi14)} /><Metric label="P/E" value={formatNumber(valuation.pe, 1)} /><Metric label="Risk-on score" value={macroSnapshot?.risk_on_score === null || macroSnapshot?.risk_on_score === undefined ? "—" : `${macroSnapshot.risk_on_score >= 0 ? "+" : ""}${(macroSnapshot.risk_on_score * 100).toFixed(0)}`} /></div><section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">QUANT</p><h3>Regime-aware analyzer</h3></div><button className="primary-button" onClick={() => setView("signals")}>Open full Analyzer <ChevronRight size={13} /></button></div>{analyzer ? <div className="analyzer-factor-list">{analyzer.factors.slice(0, 6).map((factor) => <div className="analyzer-factor" key={factor.label}><div><strong>{factor.label}</strong><small>{factor.detail}</small></div><b>{factor.score > 0 ? "+" : ""}{Math.round(factor.score * 100)}</b></div>)}</div> : <div className="market-table-empty">No valid analyzer window is available yet.</div>}</section>
    </>;
    return <div className="markets-page">
      <div className="company-hero"><div><p className="eyebrow">COMPANY RESEARCH</p><h2>{research?.name ?? quote.name}</h2><span className="company-symbol">{quote.symbol} · {quote.exchange} · {quote.currency}</span></div><div className="company-hero-actions"><button className="secondary-button" onClick={() => void toggleWatchlistSymbol(quote.symbol)}>{inWatchlist ? <X size={14} /> : <Plus size={14} />} {inWatchlist ? "Remove from watchlist" : "Add to watchlist"}</button><button className="primary-button" onClick={() => setCopilotRequest({ target: "company", nonce: Date.now(), prompt: `Review ${quote.symbol} (${quote.name}) as a thematic, fundamentals-first analyst. Separate sourced facts from inference; extract only news relevant to the thesis, catalysts, customer adoption, production, margins, valuation, substitution risk, and invalidation conditions. Give short-term and long-term outlooks, but do not invent a target or treat a headline as proof.` })}><Sparkles size={14} /> Ask Markets Qwen</button></div></div>
      <div className="company-quote-strip"><div><span>Last</span><strong>{quote.last === undefined || quote.last === null ? "Data unavailable" : `${quote.currency} ${formatNumber(quote.last)}`}</strong><FreshnessBadge provenance={quote.provenance} /></div><Metric label="Day change" value={formatPercent(quote.change_pct)} tone={(quote.change_pct ?? 0) >= 0 ? "positive" : "negative"} /><Metric label="Day range" value={`${formatNumber(quote.day_low)} – ${formatNumber(quote.day_high)}`} /><Metric label="Volume" value={formatCompact(quote.volume)} /><Metric label="Market cap" value={formatCompact(quote.market_cap)} /></div>
      <div className="company-tabs">{([ ["overview", "Overview"], ["chart", "Chart"], ["financials", "Financials"], ["valuation", "Valuation"], ["analysts", "Analysts"], ["filings", "Filings"], ["news", "News"], ["peers", "Peers"], ["quant", "Quant"] ] as [CompanyTab, string][]).map(([tab, label]) => <button className={companyTab === tab ? "selected" : ""} key={tab} onClick={() => selectCompanyTab(tab)}>{label}</button>)}</div>
      {companyTab === "filings" && <div className="filings-actions"><HkexIssuerSearch symbol={quote.symbol} /></div>}
      {companySurface}
      <ThesisSummary symbol={quote.symbol} evidence={thesisEvidence} quote={quote} onOpen={() => openThesis(quote.symbol)} />
      <MarketsCopilot target="company" request={copilotRequest} defaultPrompt={`Review ${quote.symbol} (${quote.name}) as a thematic, fundamentals-first analyst using the local quote, quant factors, live market regime, valuation context, and sourced research facts. Extract only relevant news evidence, separate facts from inference, explain the harder terms, and give short-term/long-term outlooks without inventing a price target.`} context={JSON.stringify({ quote, indicators: history?.indicators ?? null, analyzer, macro: macroSnapshot, valuation, research })} />
    </div>;
  }

  function renderCompanyLegacy() {
    if (!quote) return renderUnavailable("Choose a security", "Search for a supported exchange-qualified symbol.");
    const indicator = history?.indicators ?? {};
    return <div className="markets-page"><div className="company-hero"><div><p className="eyebrow">COMPANY RESEARCH</p><h2>{quote.name}</h2><span className="company-symbol">{quote.symbol} · {quote.exchange} · {quote.currency}</span></div><div className="company-hero-actions"><button className="secondary-button" onClick={() => void addToWatchlist()}><Plus size={14} /> Watchlist</button><button className="primary-button" onClick={() => setCopilotRequest({ target: "company", nonce: Date.now(), prompt: `Review ${quote.symbol} (${quote.name}) using only sourced market facts.` })}><Sparkles size={14} /> Ask Markets Qwen</button></div></div><div className="company-quote-strip"><div><span>Last</span><strong>{quote.last === undefined || quote.last === null ? "Data unavailable" : `${quote.currency} ${formatNumber(quote.last)}`}</strong><FreshnessBadge provenance={quote.provenance} /></div><Metric label="Day change" value={formatPercent(quote.change_pct)} tone={(quote.change_pct ?? 0) >= 0 ? "positive" : "negative"} /><Metric label="Day range" value={`${formatNumber(quote.day_low)} – ${formatNumber(quote.day_high)}`} /><Metric label="Volume" value={formatCompact(quote.volume)} /><Metric label="Turnover" value={formatCompact(quote.turnover)} /></div><div className="company-tabs"><button className="selected">Overview</button><button>Chart</button><button>Financials</button><button>Valuation</button><button>Analysts</button><button>Filings</button><button>News</button><button>Peers</button><button>Quant</button></div><div className="company-grid"><section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">PRICE HISTORY</p><h3>Daily candles</h3></div><span className="market-muted-label">Local cache first · no look-ahead</span></div><CandlesChart history={history} /></section><section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">QUANT SNAPSHOT</p><h3>Deterministic technicals</h3></div><button className="text-button" onClick={() => setView("factors")}>Open factors <ChevronRight size={13} /></button></div><div className="market-stat-grid"><Metric label="1D return" value={formatPercent(indicator.return_1d)} tone={(indicator.return_1d ?? 0) >= 0 ? "positive" : "negative"} /><Metric label="20D return" value={formatPercent(indicator.return_20d)} tone={(indicator.return_20d ?? 0) >= 0 ? "positive" : "negative"} /><Metric label="RSI 14" value={formatNumber(indicator.rsi14)} /><Metric label="Volume / 20D" value={indicator.volume_vs_sma20 ? `${indicator.volume_vs_sma20.toFixed(2)}x` : "—"} /></div><div className="source-fact-list"><div><span>SMA 20</span><strong>{formatNumber(indicator.sma20)}</strong></div><div><span>EMA 20</span><strong>{formatNumber(indicator.ema20)}</strong></div><div><span>ATR 14</span><strong>{formatNumber(indicator.atr14)}</strong></div><div><span>Max drawdown</span><strong>{formatPercent(indicator.max_drawdown)}</strong></div></div></section></div><section className="market-panel company-data-panel"><div className="market-panel-heading"><div><p className="eyebrow">SOURCE STATUS</p><h3>Fundamentals, filings, news</h3></div><FreshnessBadge provenance={quote.provenance} /></div><div className="source-status-grid"><div><FileText size={16} /><strong>HKEX announcements</strong><span>Provider adapter not enabled in this build.</span><b>UNAVAILABLE</b></div><div><Newspaper size={16} /><strong>Relevant news</strong><span>News ingestion is separate from quote data.</span><b>UNAVAILABLE</b></div><div><Building2 size={16} /><strong>Financial statements</strong><span>No statement value is substituted for missing filings.</span><b>UNAVAILABLE</b></div></div></section></div>;
  }

  function renderPortfolio() {
    if (!dashboard) return renderUnavailable("Portfolio unavailable", "Markets is still loading its local ledger.");
    const tradePnl = tradePerformance.reduce((sum, row) => sum + (row.totalPnl ?? 0), 0);
    const commissions = tradePerformance.reduce((sum, row) => sum + row.commissions, 0);
    return <div className="markets-page"><div className="markets-page-heading"><div><p className="eyebrow">PORTFOLIO</p><h2>Overview</h2><p className="muted-copy">Holdings and dated executions stay local. Trade performance includes commissions and separates realized from mark-to-market P/L.</p></div><div className="page-heading-actions"><button className="secondary-button" onClick={openNewTradeForm}><FileText size={14} /> Add past trade</button><button className="primary-button" onClick={() => setView("holdings")}><Plus size={14} /> Add holding</button></div></div><div className="portfolio-summary"><Metric label="Positions" value={dashboard.portfolio.length.toString()} /><Metric label="Market value" value={formatCompact(dashboard.portfolio.reduce((sum, row) => sum + (row.market_value ?? 0), 0))} /><Metric label="Trade-book P/L" value={formatNumber(tradePnl)} tone={tradePnl >= 0 ? "positive" : "negative"} /><Metric label="Commissions" value={formatNumber(commissions)} /></div><section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">CURRENT POSITIONS</p><h3>Mark-to-market holdings</h3></div><button className="text-button" onClick={() => setView("performance")}>Open performance <ChevronRight size={13} /></button></div><HoldingsTable rows={dashboard.portfolio} onSelect={(symbol) => void loadSecurity(symbol)} /></section><section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">TRADE BOOK</p><h3>Historical performance</h3></div><button className="text-button" onClick={() => setView("transactions")}>View ledger <ChevronRight size={13} /></button></div><TradePerformanceTable rows={tradePerformance.slice(0, 6)} onSelect={(symbol) => void loadSecurity(symbol)} /></section></div>;
  }

  function renderHoldings() {
    return <div className="markets-page"><div className="markets-page-heading"><div><p className="eyebrow">PORTFOLIO LEDGER</p><h2>Positions & executions</h2><p className="muted-copy">Use a holding for a current snapshot, or record each past BUY / SELL to reconstruct performance with commissions.</p></div></div><section className="market-panel holding-form"><div className="market-panel-heading"><div><p className="eyebrow">CURRENT SNAPSHOT</p><h3>Add or replace a manual holding</h3></div><span className="market-muted-label">One row per account and symbol</span></div><label><span>Account</span><input value={holdingForm.account} onChange={(event) => setHoldingForm({ ...holdingForm, account: event.target.value })} /></label><label><span>Ticker</span><input value={holdingForm.symbol} onChange={(event) => setHoldingForm({ ...holdingForm, symbol: event.target.value })} placeholder="700" /></label><label><span>Shares</span><input type="number" min="0" value={holdingForm.shares} onChange={(event) => setHoldingForm({ ...holdingForm, shares: event.target.value })} /></label><label><span>Average cost · HKD</span><input type="number" min="0" step="0.01" value={holdingForm.averageCost} onChange={(event) => setHoldingForm({ ...holdingForm, averageCost: event.target.value })} /></label><div className="holding-form-actions"><button className="secondary-button" onClick={() => setView("portfolio")}><X size={14} /> Cancel</button><button className="primary-button" onClick={() => void saveHolding()}><Check size={14} /> Save local position</button></div></section><section className="market-panel holding-form trade-entry-form"><div className="market-panel-heading"><div><p className="eyebrow">HISTORICAL EXECUTION</p><h3>{editingTradeId ? "Edit past trade" : "Record a past trade"}</h3><span className="market-muted-label">Commission is charged on the execution and included in return calculations.</span></div><button className="text-button" onClick={cancelTradeForm}>Cancel <X size={13} /></button></div><label><span>Account</span><input value={tradeForm.account} onChange={(event) => setTradeForm({ ...tradeForm, account: event.target.value })} /></label><label><span>Ticker</span><input value={tradeForm.symbol} onChange={(event) => setTradeForm({ ...tradeForm, symbol: event.target.value })} placeholder="700" /></label><label><span>Side</span><select value={tradeForm.side} onChange={(event) => setTradeForm({ ...tradeForm, side: event.target.value as "BUY" | "SELL" })}><option value="BUY">BUY</option><option value="SELL">SELL</option></select></label><label><span>Quantity</span><input type="number" min="0" step="1" value={tradeForm.quantity} onChange={(event) => setTradeForm({ ...tradeForm, quantity: event.target.value })} /></label><label><span>Execution price · HKD</span><input type="number" min="0" step="0.01" value={tradeForm.price} onChange={(event) => setTradeForm({ ...tradeForm, price: event.target.value })} /></label><label><span>Commission · HKD</span><input type="number" min="0" step="0.01" value={tradeForm.commission} onChange={(event) => setTradeForm({ ...tradeForm, commission: event.target.value })} /></label><label><span>Trade date</span><input type="date" value={tradeForm.tradeDate} onChange={(event) => setTradeForm({ ...tradeForm, tradeDate: event.target.value })} /></label><label className="form-wide"><span>Notes / thesis tag</span><input value={tradeForm.notes} onChange={(event) => setTradeForm({ ...tradeForm, notes: event.target.value })} placeholder="e.g. AI infrastructure starter position" /></label><div className="holding-form-actions"><button className="secondary-button" onClick={cancelTradeForm}><X size={14} /> Cancel</button><button className="primary-button" onClick={() => void saveTrade()}><Check size={14} /> {editingTradeId ? "Save trade changes" : "Save dated trade"}</button></div></section><section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">BROKERAGE SOURCES</p><h3>Connection boundaries</h3></div></div><div className="source-status-grid"><div><WalletCards size={16} /><strong>Futu</strong><span>Read-only sync is routed through local OpenD when configured.</span><b>CHECK IN SETTINGS</b></div><div><FileText size={16} /><strong>DBS</strong><span>CSV/XLSX import is the safe path; browser login automation is not used.</span><b>IMPORT ADAPTER NEXT</b></div><div><CircleDollarSign size={16} /><strong>Manual</strong><span>Active now. Stored locally in the JadX SQLite ledger.</span><b>AVAILABLE</b></div></div></section></div>;
  }

  function renderQuant(mode: View = "factors") {
    const indicator = history?.indicators ?? {};
    const quantRisk = riskSnapshot(history?.candles ?? []);
    const quantSignals = buildChartSignals(history?.candles ?? [], analyzer?.fastPeriod ?? 20, analyzer?.slowPeriod ?? 50, { quote, macro: macroSnapshot, valuation, research });
    if (mode === "screener") {
      const quotes = [...(dashboard?.watchlist_quotes ?? [])].sort((left, right) => (right.change_pct ?? -Infinity) - (left.change_pct ?? -Infinity));
      const averageMove = quotes.length ? quotes.reduce((sum, item) => sum + (item.change_pct ?? 0), 0) / quotes.length : null;
      const leader = quotes[0];
      return <div className="markets-page"><div className="markets-page-heading"><div><p className="eyebrow">QUANT SCREENER</p><h2>Watchlist scan</h2><p className="muted-copy">Deterministic ranking over the current watchlist. No model-generated scores or unsupported fundamentals.</p></div><button className="secondary-button" onClick={() => void loadDashboard(true)}><RefreshCw size={14} /> Refresh universe</button></div><div className="portfolio-summary"><Metric label="Universe" value={quotes.length.toString()} /><Metric label="Average day move" value={formatPercent(averageMove)} tone={(averageMove ?? 0) >= 0 ? "positive" : "negative"} /><Metric label="Top mover" value={leader?.symbol ?? "—"} /><Metric label="Live quotes" value={quotes.filter((item) => item.provenance.freshness === "Live").length.toString()} /></div><section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">RANKED OUTPUT</p><h3>Momentum by one-day move</h3></div><span className="market-muted-label">Sort: change % descending</span></div><QuoteTable quotes={quotes} onSelect={(symbol) => void loadSecurity(symbol)} /></section></div>;
    }
    if (mode === "signals") {
      if (!analyzer) return <div className="markets-page"><div className="markets-page-heading"><div><p className="eyebrow">QUANT ANALYZER</p><h2>Historical analyzer</h2><p className="muted-copy">At least 80 daily candles are required to calibrate and validate the research bias.</p></div><button className="secondary-button" onClick={() => void loadSecurity(selectedSymbol)}><RefreshCw size={14} /> Load history</button></div><section className="market-panel"><div className="market-table-empty">No valid training window is available for {selectedSymbol} yet.</div></section><TermGlossary /></div>;
      const verdictTone = analyzer.verdict === "BUY" ? "positive" : analyzer.verdict === "SELL" ? "negative" : undefined;
      return <div className="markets-page"><div className="markets-page-heading"><div><p className="eyebrow">QUANT ANALYZER</p><h2>Regime-aware multi-factor analyzer</h2><p className="muted-copy">A thematic, fundamentals-first BUY / HOLD / SELL research bias that uses technicals as timing context. It never submits orders.</p></div>{history && <FreshnessBadge provenance={history.provenance} />}</div><section className="signal-grid"><article className="signal-card analyzer-verdict"><span>Timing / factor bias · check thesis separately</span><strong className={verdictTone ? `market-${verdictTone}` : ""}>{analyzer.verdict} · {analyzer.strength}</strong><p>Composite score {analyzer.score > 0 ? "+" : ""}{analyzer.score}/100 · {analyzer.confidence}% confidence. Research output only.</p></article><article className="signal-card"><span>Walk-forward calibration</span><strong>{analyzer.fastPeriod}/{analyzer.slowPeriod} SMA</strong><p>Windows were selected on earlier candles, then evaluated on later unseen candles without look-ahead.</p></article><article className="signal-card"><span>Training return</span><strong className={analyzer.trainingReturn >= 0 ? "market-positive" : "market-negative"}>{formatPercent(analyzer.trainingReturn)}</strong><p>{analyzer.trainingSessions} scored sessions before validation.</p></article><article className="signal-card"><span>Validation return</span><strong className={analyzer.validationReturn >= 0 ? "market-positive" : "market-negative"}>{formatPercent(analyzer.validationReturn)}</strong><p>{analyzer.validationSessions} later sessions · directional hit rate {formatPercent(analyzer.validationHitRate)}</p></article></section><ThesisSummary symbol={selectedSymbol} evidence={thesisEvidence} quote={quote} onOpen={() => openThesis(selectedSymbol)} /><QuantPipeline history={history} analyzer={analyzer} /><section className="market-panel analyzer-context-panel"><div className="market-panel-heading"><div><p className="eyebrow">LIVE CONTEXT OVERLAY</p><h3>Market regime & valuation</h3></div><FreshnessBadge provenance={macroSnapshot?.provenance ?? { provider: "macro-sources", retrieved_at: new Date().toISOString(), freshness: "Unavailable", realtime: false }} /></div><div className="market-stat-grid"><Metric label="Risk-on score" value={macroSnapshot?.risk_on_score === null || macroSnapshot?.risk_on_score === undefined ? "—" : `${macroSnapshot.risk_on_score >= 0 ? "+" : ""}${(macroSnapshot.risk_on_score * 100).toFixed(0)}`} tone={(macroSnapshot?.risk_on_score ?? 0) >= 0 ? "positive" : "negative"} /><Metric label="HSI today" value={formatPercent(macroSnapshot?.hsi?.change_pct)} tone={(macroSnapshot?.hsi?.change_pct ?? 0) >= 0 ? "positive" : "negative"} /><Metric label="HSTECH today" value={formatPercent(macroSnapshot?.hstech?.change_pct)} tone={(macroSnapshot?.hstech?.change_pct ?? 0) >= 0 ? "positive" : "negative"} /><Metric label="P/E" value={formatNumber(valuation.pe, 1)} /><Metric label="P/B" value={formatNumber(valuation.priceToBook, 2)} /><Metric label="Earnings yield" value={formatPercent(valuation.earningsYield)} /></div><p className="muted-copy analyzer-context-copy">The live overlay adapts conviction when the broader market is rallying or falling. Public cross-asset series may be delayed; unavailable valuation inputs remain neutral and reduce confidence.</p></section><section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">FACTOR LEDGER</p><h3>What moved the decision</h3></div><span className="market-muted-label">{(analyzer.dataCoverage * 100).toFixed(0)}% input coverage</span></div><div className="analyzer-factor-list">{analyzer.factors.map((factor) => <div className="analyzer-factor" key={factor.label}><div><strong>{factor.label}</strong><small>{factor.detail}</small></div><b className={factor.score > 0.08 ? "market-positive" : factor.score < -0.08 ? "market-negative" : ""}>{factor.score > 0 ? "+" : ""}{Math.round(factor.score * 100)}</b></div>)}</div></section><SignalGuide signals={quantSignals} currentScore={analyzer.score} risk={quantRisk} /><TailRiskPanel risk={quantRisk} /><InvestorLensPanel lens={analyzer.investorLens} /><OutlookPanel outlook={analyzer.outlook} quote={quote ?? { currency: "HKD" } as Quote} /><EvidenceLedger quote={quote} history={history} research={research} macro={macroSnapshot} analyzer={analyzer} /><section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">EVIDENCE</p><h3>Why the analyzer landed here</h3></div><span className="market-muted-label">No look-ahead</span></div><ul className="analyzer-reasons">{analyzer.reasons.map((reason) => <li key={reason}>{reason}</li>)}</ul></section><MarketsCopilot target="analyzer" request={copilotRequest} defaultPrompt={`Explain the ${analyzer.verdict} / ${analyzer.score} analyzer result for ${selectedSymbol} in plain English, including the strongest factors and what could invalidate it.`} context={JSON.stringify({ symbol: selectedSymbol, quote, analyzer, macro: macroSnapshot, valuation, indicators: history?.indicators ?? null })} /><TermGlossary /></div>;
    }
    if (mode === "backtests") {
      if (!backtest) return <div className="markets-page"><div className="markets-page-heading"><div><p className="eyebrow">QUANT BACKTEST LAB</p><h2>Simulation workspace</h2><p className="muted-copy">At least {Math.max(60, simulationConfig.slowPeriod + 10)} candles are required for a clean simulation.</p></div><button className="secondary-button" onClick={() => void loadSecurity(selectedSymbol)}><RefreshCw size={14} /> Load history</button></div><section className="market-panel"><div className="market-table-empty">No valid simulation window is available for {selectedSymbol} yet.</div></section></div>;
      const strategyLabel: Record<BacktestStrategy, string> = { trend: "Trend following", momentum: "Momentum continuation", mean_reversion: "Mean reversion", composite: "Quant composite" };
      const comparison = (Object.keys(strategyLabel) as BacktestStrategy[]).map((strategy) => ({ strategy, result: runStrategyBacktest(history, { ...simulationConfig, strategy }) })).filter((item): item is { strategy: BacktestStrategy; result: BacktestResult } => item.result !== null);
      return <div className="markets-page backtest-lab-page"><div className="markets-page-heading"><div><p className="eyebrow">QUANT BACKTEST LAB</p><h2>Simulate before you trust it</h2><p className="muted-copy">Event-aware fills · fees · slippage · walk-forward validation · deterministic Monte Carlo bands.</p></div><div className="backtest-heading-actions"><FreshnessBadge provenance={history?.provenance ?? { provider: "none", retrieved_at: new Date().toISOString(), freshness: "Unavailable", realtime: false }} /><button className="secondary-button" onClick={() => { setSimulationConfig(DEFAULT_SIMULATION_CONFIG); setSimulationNonce((value) => value + 1); }}><RotateCcw size={14} /> Reset</button></div></div><section className="market-panel simulation-control-panel"><div className="market-panel-heading"><div><p className="eyebrow">SIMULATION CONTROL ROOM</p><h3>{strategyLabel[simulationConfig.strategy]} · {selectedSymbol}</h3><span className="market-muted-label">Signals use information available before the simulated fill. Results are research-only.</span></div><button className="primary-button" onClick={() => setSimulationNonce((value) => value + 1)}><Play size={14} /> Run simulation</button></div><div className="simulation-control-grid"><label><span>Strategy</span><select value={simulationConfig.strategy} onChange={(event) => setSimulationConfig({ ...simulationConfig, strategy: event.target.value as BacktestStrategy })}><option value="composite">Quant composite</option><option value="trend">Trend following</option><option value="momentum">Momentum continuation</option><option value="mean_reversion">Mean reversion</option></select></label><label><span>Fast window</span><input type="number" min="5" max="100" value={simulationConfig.fastPeriod} onChange={(event) => setSimulationConfig({ ...simulationConfig, fastPeriod: Math.max(5, Number(event.target.value) || 5) })} /></label><label><span>Slow window</span><input type="number" min="20" max="180" value={simulationConfig.slowPeriod} onChange={(event) => setSimulationConfig({ ...simulationConfig, slowPeriod: Math.max(20, Number(event.target.value) || 20) })} /></label><label><span>Initial capital</span><input type="number" min="1000" step="1000" value={simulationConfig.initialCapital} onChange={(event) => setSimulationConfig({ ...simulationConfig, initialCapital: Math.max(1000, Number(event.target.value) || 1000) })} /></label><label><span>Fee · bps per side</span><input type="number" min="0" max="100" step="1" value={simulationConfig.feeBps} onChange={(event) => setSimulationConfig({ ...simulationConfig, feeBps: Math.max(0, Number(event.target.value) || 0) })} /></label><label><span>Slippage · bps per side</span><input type="number" min="0" max="200" step="1" value={simulationConfig.slippageBps} onChange={(event) => setSimulationConfig({ ...simulationConfig, slippageBps: Math.max(0, Number(event.target.value) || 0) })} /></label><label><span>Target exposure</span><input type="number" min="0.1" max="1" step="0.1" value={simulationConfig.targetExposure} onChange={(event) => setSimulationConfig({ ...simulationConfig, targetExposure: clamp(Number(event.target.value) || 0.1, 0.1, 1) })} /></label><label><span>Stops · % / %</span><div className="simulation-dual-input"><input aria-label="Stop loss percent" type="number" min="0" max="50" step="0.5" value={simulationConfig.stopLossPct} onChange={(event) => setSimulationConfig({ ...simulationConfig, stopLossPct: Math.max(0, Number(event.target.value) || 0) })} /><input aria-label="Take profit percent" type="number" min="0" max="100" step="0.5" value={simulationConfig.takeProfitPct} onChange={(event) => setSimulationConfig({ ...simulationConfig, takeProfitPct: Math.max(0, Number(event.target.value) || 0) })} /></div></label></div></section><section className="market-panel strategy-comparison"><div className="market-panel-heading"><div><p className="eyebrow">STRATEGY SHOWDOWN</p><h3>Compare the same costs across models</h3></div><span className="market-muted-label">Click a model to make it active</span></div><div className="strategy-comparison-grid">{comparison.map(({ strategy, result }) => <button className={`strategy-comparison-card ${strategy === simulationConfig.strategy ? "active" : ""}`} key={strategy} onClick={() => setSimulationConfig({ ...simulationConfig, strategy })}><span>{strategyLabel[strategy]}</span><strong className={result.totalReturn >= 0 ? "market-positive" : "market-negative"}>{formatPercent(result.totalReturn)}</strong><small>Sharpe {result.sharpe.toFixed(2)} · DD {formatPercent(result.maxDrawdown)} · {result.trades} trades</small><em>{result.walkForward.stability}</em></button>)}</div></section><div className="factor-grid backtest-metric-grid"><Metric label="Net return" value={formatPercent(backtest.totalReturn)} tone={backtest.totalReturn >= 0 ? "positive" : "negative"} /><Metric label="Final capital" value={`${quote?.currency ?? "HKD"} ${formatNumber(backtest.finalCapital, 0)}`} /><Metric label="Annualized" value={formatPercent(backtest.annualizedReturn)} tone={backtest.annualizedReturn >= 0 ? "positive" : "negative"} /><Metric label="Benchmark" value={formatPercent(backtest.benchmarkReturn)} tone={backtest.benchmarkReturn >= 0 ? "positive" : "negative"} /><Metric label="Sharpe / Sortino" value={`${backtest.sharpe.toFixed(2)} / ${backtest.sortino.toFixed(2)}`} /><Metric label="Max drawdown" value={formatPercent(backtest.maxDrawdown)} tone="negative" /><Metric label="Win rate" value={formatPercent(backtest.winRate)} /><Metric label="Profit factor" value={backtest.profitFactor === null ? "∞" : formatNumber(backtest.profitFactor)} /></div><section className="market-panel simulation-curve-panel"><div className="market-panel-heading"><div><p className="eyebrow">PERFORMANCE RECONSTRUCTION</p><h3>Equity and drawdown</h3></div><div className="simulation-summary-badges"><span><Layers3 size={13} /> {backtest.trades} trades</span><span><Target size={13} /> {(backtest.exposure * 100).toFixed(0)}% exposure</span><span><Zap size={13} /> {formatNumber(backtest.fees + backtest.slippage, 0)} costs</span></div></div><div className="simulation-curve-grid"><EquityCurve values={backtest.equity} /><DrawdownCurve values={backtest.drawdown} /></div></section><div className="simulation-detail-grid"><section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">ROBUSTNESS</p><h3>Holdout and resampled paths</h3></div><FlaskConical size={16} /></div><div className="signal-grid"><article className="signal-card"><span>Walk-forward holdout</span><strong className={backtest.walkForward.validationReturn >= 0 ? "market-positive" : "market-negative"}>{formatPercent(backtest.walkForward.validationReturn)}</strong><p>Training {formatPercent(backtest.walkForward.trainingReturn)} · validation Sharpe {backtest.walkForward.validationSharpe.toFixed(2)}.</p></article><article className="signal-card"><span>Stability read</span><strong>{backtest.walkForward.stability}</strong><p>Parameters are judged on later candles that were not used for calibration.</p></article><article className="signal-card"><span>Monte Carlo p05</span><strong className={backtest.monteCarlo.p05 >= 0 ? "market-positive" : "market-negative"}>{formatPercent(backtest.monteCarlo.p05)}</strong><p>Lower-tail ending return from deterministic bootstrap paths.</p></article><article className="signal-card"><span>Monte Carlo p50 / p95</span><strong>{formatPercent(backtest.monteCarlo.p50)} / {formatPercent(backtest.monteCarlo.p95)}</strong><p>Distribution is a stress test, not a forecast.</p></article></div></section><section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">EXECUTION AUDIT</p><h3>What the simulator charged</h3></div><Cpu size={16} /></div><div className="source-fact-list"><div><span>Fees</span><strong>{quote?.currency ?? "HKD"} {formatNumber(backtest.fees)}</strong></div><div><span>Slippage</span><strong>{quote?.currency ?? "HKD"} {formatNumber(backtest.slippage)}</strong></div><div><span>Turnover</span><strong>{backtest.turnover.toFixed(2)}× capital</strong></div><div><span>Annualized volatility</span><strong>{formatPercent(backtest.volatility)}</strong></div><div><span>Calmar ratio</span><strong>{backtest.calmar.toFixed(2)}</strong></div><div><span>Average trade</span><strong>{formatPercent(backtest.averageTrade)}</strong></div></div></section></div><section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">TRADE REPLAY</p><h3>Simulated fills</h3><span className="market-muted-label">Open and close timestamps are sourced from the candle set.</span></div><span className="market-muted-label">{backtest.tradeLog.length} reconstructed trades</span></div>{backtest.tradeLog.length ? <div className="market-table trade-replay-table"><div className="market-table-row market-table-head"><span>Entry</span><span>Exit</span><span>Entry px</span><span>Exit px</span><span>Return</span><span>Reason</span></div>{backtest.tradeLog.slice().reverse().slice(0, 20).map((trade) => <div className="market-table-row" key={`${trade.entryTimestamp}-${trade.exitTimestamp}`}><span>{new Date(trade.entryTimestamp * 1000).toLocaleDateString()}</span><span>{new Date(trade.exitTimestamp * 1000).toLocaleDateString()}</span><span>{formatNumber(trade.entryPrice)}</span><span>{formatNumber(trade.exitPrice)}</span><span className={trade.netPnl >= 0 ? "market-positive" : "market-negative"}>{formatPercent(trade.return)}</span><span>{trade.reason}</span></div>)}</div> : <div className="market-table-empty">This configuration generated no completed trades.</div>}</section><section className="market-panel simulation-disclaimer"><AlertTriangle size={16} /><p>Simulation quality depends on data quality. Corporate actions, borrow, halts, queue position, market impact, survivorship bias, and delisted names are not magically solved by a polished chart. Use this to challenge a thesis and compare assumptions before paper trading.</p></section><TermGlossary /></div>;
    }
      return <div className="markets-page"><div className="markets-page-heading"><div><p className="eyebrow">QUANT</p><h2>Factors & technicals</h2><p className="muted-copy">Calculated locally from the retrieved candle set. The model is not used for arithmetic.</p></div><button className="secondary-button" onClick={() => void loadSecurity(selectedSymbol)} disabled={loading}><RefreshCw size={14} /> Recalculate</button></div>{history && <div className="quant-provenance"><span>{history.symbol} · {history.candles.length} candles · {history.interval}</span><FreshnessBadge provenance={history.provenance} /></div>}<section className="factor-grid"><Metric label="1D momentum" value={formatPercent(indicator.return_1d)} tone={(indicator.return_1d ?? 0) >= 0 ? "positive" : "negative"} /><Metric label="20D momentum" value={formatPercent(indicator.return_20d)} tone={(indicator.return_20d ?? 0) >= 0 ? "positive" : "negative"} /><Metric label="60D momentum" value={formatPercent(indicator.return_60d)} tone={(indicator.return_60d ?? 0) >= 0 ? "positive" : "negative"} /><Metric label="RSI 14" value={formatNumber(indicator.rsi14)} /><Metric label="Ann. volatility" value={formatPercent(analytics.volatility20)} /><Metric label="MACD histogram" value={formatNumber(analytics.macdHistogram)} tone={(analytics.macdHistogram ?? 0) >= 0 ? "positive" : "negative"} /><Metric label="Bollinger mid" value={formatNumber(analytics.bollingerMiddle)} /><Metric label="Max drawdown" value={formatPercent(indicator.max_drawdown)} tone="negative" /></section><section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">RANGE & LEVELS</p><h3>20-session context</h3></div><span className={analytics.signalTone ? `market-${analytics.signalTone}` : "market-muted-label"}>{analytics.signal}</span></div><div className="market-stat-grid"><Metric label="Support 20D" value={formatNumber(analytics.support20)} /><Metric label="Resistance 20D" value={formatNumber(analytics.resistance20)} /><Metric label="Bollinger upper" value={formatNumber(analytics.bollingerUpper)} /><Metric label="Bollinger lower" value={formatNumber(analytics.bollingerLower)} /><Metric label="ATR 14" value={formatNumber(indicator.atr14)} /><Metric label="Volume / 20D" value={indicator.volume_vs_sma20 ? `${indicator.volume_vs_sma20.toFixed(2)}x` : "—"} /></div></section><section className="market-panel chart-primary-panel"><div className="market-panel-heading"><div><p className="eyebrow">DECISION CHART</p><h3>Price with model entry / exit markers</h3></div><span className="market-muted-label">Thresholds are explained below</span></div><ChartControls mode={chartMode} range={chartRange} onModeChange={setChartMode} onRangeChange={setChartRange} /><CandlesChart history={history} signals={quantSignals} levels={{ support: analytics.support20, resistance: analytics.resistance20 }} mode={chartMode} range={chartRange} /></section><SignalGuide signals={quantSignals} currentScore={analyzer?.score} risk={quantRisk} /><TailRiskPanel risk={quantRisk} /><QuantPipeline history={history} analyzer={analyzer} />{!history?.candles.length && <section className="market-panel"><div className="market-table-empty">No candle inputs are available for this security.</div></section>}</div>;
  }

  function renderQuantLegacy() {
    const indicator = history?.indicators ?? {};
    return <div className="markets-page"><div className="markets-page-heading"><div><p className="eyebrow">QUANT</p><h2>Factors & technicals</h2><p className="muted-copy">Numbers are calculated locally from the retrieved candle set. The model is not used for arithmetic.</p></div><button className="secondary-button" onClick={() => void loadSecurity(selectedSymbol)} disabled={loading}><RefreshCw size={14} /> Recalculate</button></div>{history && <div className="quant-provenance"><span>{history.symbol} · {history.candles.length} candles · {history.interval}</span><FreshnessBadge provenance={history.provenance} /></div>}<section className="factor-grid"><Metric label="1D momentum" value={formatPercent(indicator.return_1d)} tone={(indicator.return_1d ?? 0) >= 0 ? "positive" : "negative"} /><Metric label="20D momentum" value={formatPercent(indicator.return_20d)} tone={(indicator.return_20d ?? 0) >= 0 ? "positive" : "negative"} /><Metric label="60D momentum" value={formatPercent(indicator.return_60d)} tone={(indicator.return_60d ?? 0) >= 0 ? "positive" : "negative"} /><Metric label="RSI 14" value={formatNumber(indicator.rsi14)} /><Metric label="ATR 14" value={formatNumber(indicator.atr14)} /><Metric label="Volume anomaly" value={indicator.volume_vs_sma20 ? `${indicator.volume_vs_sma20.toFixed(2)}x` : "—"} /><Metric label="SMA 20" value={formatNumber(indicator.sma20)} /><Metric label="Max drawdown" value={formatPercent(indicator.max_drawdown)} tone="negative" /></section>{!history?.candles.length && renderUnavailable("Quant inputs unavailable", "Connect OpenD or enable SIMULATED development fixtures. No factors are inferred without input data.")}</div>;
  }

  function renderTrading(kind: "paper" | "orders" | "history") {
    if (kind === "orders") return <div className="markets-page"><div className="markets-page-heading"><div><p className="eyebrow">TRADING</p><h2>Open orders</h2><p className="muted-copy">Only locally recorded paper orders are visible here. Live broker orders are not exposed.</p></div><button className="secondary-button" onClick={() => setView("paper")}><ShieldCheck size={14} /> Paper trading</button></div><section className="market-panel"><div className="research-section-title"><Activity size={16} /><strong>Working orders</strong></div><div className="market-table-empty">{orders.length ? `${orders.length} locally recorded order${orders.length === 1 ? "" : "s"}; none are routed to a broker.` : "No open orders. Validate a paper order to create a local preview."}</div></section></div>;
    if (kind === "history") return <div className="markets-page"><div className="markets-page-heading"><div><p className="eyebrow">TRADING</p><h2>Order history</h2><p className="muted-copy">Local paper activity with explicit source and execution boundaries.</p></div><button className="secondary-button" onClick={() => setView("paper")}><ShieldCheck size={14} /> New paper order</button></div><section className="market-panel"><div className="research-section-title"><FileText size={16} /><strong>Recorded activity</strong></div>{orders.length ? <div className="market-table orders-table"><div className="market-table-row market-table-head"><span>Order</span><span>Side</span><span>Quantity</span><span>Type</span><span>Status</span><span>Created</span></div>{orders.map((order) => <div className="market-table-row" key={order.id}><span><strong>{order.symbol}</strong><small>{order.mode}</small></span><span className={order.side === "BUY" ? "market-positive" : "market-negative"}>{order.side}</span><span>{formatNumber(order.quantity, 0)}</span><span>{order.order_type}{order.limit_price ? ` · ${formatNumber(order.limit_price)}` : ""}</span><span>{order.status}</span><span>{new Date(order.created_at).toLocaleString()}</span></div>)}</div> : <div className="market-table-empty">No paper orders have been recorded on this device.</div>}</section></div>;
    return <div className="markets-page"><div className="markets-page-heading"><div><p className="eyebrow">TRADING SAFETY</p><h2>Paper orders</h2><p className="muted-copy">Live submission is intentionally not exposed by this Markets runtime.</p></div><span className={`trading-mode-badge ${dashboard?.trading_mode === "OFF" ? "off" : "paper"}`}><ShieldCheck size={14} /> Trading {dashboard?.trading_mode ?? "OFF"}</span></div><section className="market-panel order-form"><label><span>Symbol</span><input value={orderForm.symbol} onChange={(event) => setOrderForm({ ...orderForm, symbol: event.target.value })} placeholder="700" /></label><label><span>Side</span><select value={orderForm.side} onChange={(event) => setOrderForm({ ...orderForm, side: event.target.value })}><option>BUY</option><option>SELL</option></select></label><label><span>Quantity</span><input type="number" min="1" value={orderForm.quantity} onChange={(event) => setOrderForm({ ...orderForm, quantity: event.target.value })} /></label><label><span>Limit price (optional)</span><input type="number" min="0" step="0.01" value={orderForm.limitPrice} onChange={(event) => setOrderForm({ ...orderForm, limitPrice: event.target.value })} /></label><div className="holding-form-actions"><button className="primary-button" onClick={() => void previewOrder()}><ShieldCheck size={14} /> Validate order</button></div></section>{orderPreview && <section className={`market-panel order-preview ${orderPreview.valid ? "valid" : "invalid"}`}><div className="market-panel-heading"><div><p className="eyebrow">DETERMINISTIC PREVIEW</p><h3>{orderPreview.side} {orderPreview.quantity} {orderPreview.symbol}</h3></div><FreshnessBadge provenance={orderPreview.provenance} /></div><div className="market-stat-grid"><Metric label="Type" value={orderPreview.order_type} /><Metric label="Notional" value={formatNumber(orderPreview.estimated_notional)} /><Metric label="Market" value={orderPreview.market_state} /><Metric label="Permission" value={orderPreview.trading_mode} /></div>{orderPreview.issues.length > 0 && <ul className="order-issues">{orderPreview.issues.map((issue) => <li key={issue}>{issue}</li>)}</ul>}{orderPreview.valid && <button className="primary-button" onClick={() => void submitPaperOrder()}><Check size={14} /> Submit paper order</button>}</section>}<section className="market-panel safety-panel"><AlertTriangle size={18} /><div><strong>Live trading is disabled</strong><p>The preview endpoint never submits a real-money order. A future live mode must remain outside direct Qwen control and require a fresh human confirmation.</p></div></section></div>;
  }

  function renderResearchView(kind: "news" | "filings" | "research") {
    const provenance = research?.provenance ?? quote?.provenance ?? { provider: "none", retrieved_at: new Date().toISOString(), freshness: "Unavailable" as Freshness, realtime: false };
    if (kind === "news") return <div className="markets-page"><div className="markets-page-heading"><div><p className="eyebrow">RESEARCH</p><h2>News</h2><p className="muted-copy">Public RSS headlines are supplemental and may be delayed or rate-limited.</p></div><FreshnessBadge provenance={provenance} /></div><section className="market-panel"><ResearchItems items={research?.news ?? []} emptyLabel={provenance.message ?? "No public news item returned for this security."} /></section></div>;
    if (kind === "filings") return <div className="markets-page"><div className="markets-page-heading"><div><p className="eyebrow">RESEARCH</p><h2>Filings</h2><p className="muted-copy">Links point to official issuer-search portals; JadX does not fabricate filing content.</p></div><FreshnessBadge provenance={provenance} /></div><section className="market-panel"><ResearchItems items={research?.filings ?? []} emptyLabel="No official filing search link returned." /></section></div>;
    return <div className="markets-page"><div className="markets-page-heading"><div><p className="eyebrow">RESEARCH LIBRARY</p><h2>{research?.name ?? quote?.name ?? "Security research"}</h2><p className="muted-copy">Read-only profile and latest financial facts from configured providers.</p></div><FreshnessBadge provenance={provenance} /></div><div className="company-grid"><section className="market-panel"><div className="research-section-title"><Building2 size={16} /><strong>Profile</strong></div><ResearchFacts facts={research?.profile ?? []} emptyLabel="No profile response returned yet." /></section><section className="market-panel"><div className="research-section-title"><CircleDollarSign size={16} /><strong>Financial statements</strong></div><ResearchFacts facts={research?.financials ?? []} emptyLabel="No financial statement response returned yet." /></section></div><section className="market-panel"><div className="research-section-title"><FileText size={16} /><strong>Official filing links</strong></div><ResearchItems items={research?.filings ?? []} emptyLabel="No official filing search link returned." /></section></div>;
  }

  function renderPortfolioSurface(kind: "performance" | "recommendations" | "transactions" | "risk") {
    if (!dashboard) return <div className="market-loading"><RefreshCw size={18} className="spin-icon" /> Loading local portfolio ledger…</div>;
    const totalValue = dashboard.portfolio.reduce((sum, row) => sum + (row.market_value ?? 0), 0);
    const totalPnl = dashboard.portfolio.reduce((sum, row) => sum + (row.unrealized_pnl ?? 0), 0);
    const largest = dashboard.portfolio.reduce<PortfolioRow | null>((current, row) => !current || (row.market_value ?? 0) > (current.market_value ?? 0) ? row : current, null);
    const commissionTotal = tradePerformance.reduce((sum, row) => sum + row.commissions, 0);
    const tradePnl = tradePerformance.reduce((sum, row) => sum + (row.totalPnl ?? 0), 0);
    const heldRecommendationSymbols = new Set(thesisPositions.filter((row) => row.shares > 0).map((row) => row.symbol));
    if (kind === "transactions") return <div className="markets-page"><div className="markets-page-heading"><div><p className="eyebrow">PORTFOLIO</p><h2>Transactions</h2><p className="muted-copy">Record past executions with their actual date, fill price, side, and commission. Edit any detail or remove a row after confirmation; performance recalculates immediately.</p></div><div className="page-heading-actions"><button className="secondary-button" onClick={openNewTradeForm}><Plus size={14} /> Add dated trade</button><button className="secondary-button" onClick={() => setView("paper")}><ShieldCheck size={14} /> Paper trading</button></div></div><div className="portfolio-summary"><Metric label="Recorded trades" value={dashboard.trades.length.toString()} /><Metric label="Trade-book P/L" value={formatNumber(tradePnl)} tone={tradePnl >= 0 ? "positive" : "negative"} /><Metric label="Commissions" value={formatNumber(commissionTotal)} /><Metric label="Open trade groups" value={tradePerformance.filter((row) => row.shares > 0).length.toString()} /></div><section className="market-panel"><TradeLedgerTable trades={dashboard.trades} onSelect={(symbol) => void loadSecurity(symbol)} onEdit={editTrade} onRemove={(trade) => void deleteTrade(trade)} /></section></div>;
    if (kind === "risk") return <div className="markets-page"><div className="markets-page-heading"><div><p className="eyebrow">PORTFOLIO</p><h2>Risk</h2><p className="muted-copy">Deterministic exposure checks from current holdings and trade-book open shares; missing quotes stay explicit.</p></div></div><div className="portfolio-summary"><Metric label="Positions" value={dashboard.portfolio.length.toString()} /><Metric label="Market value" value={formatCompact(totalValue)} /><Metric label="Largest position" value={largest ? largest.holding.symbol : "—"} /><Metric label="Largest weight" value={largest && totalValue > 0 ? formatPercent((largest.market_value ?? 0) / totalValue) : "—"} /></div><section className="market-panel"><div className="research-section-title"><AlertTriangle size={16} /><strong>Risk boundaries</strong></div><p className="muted-copy">Concentration is calculated from the local ledger and current sourced quotes. Historical trade returns use average-cost matching for realized P/L; they are not a tax-lot or broker statement.</p><div className="market-stat-grid"><Metric label="Trade-book commissions" value={formatNumber(commissionTotal)} /><Metric label="Open trade groups" value={tradePerformance.filter((row) => row.shares > 0).length.toString()} /><Metric label="Unrealized trade P/L" value={formatNumber(tradePerformance.reduce((sum, row) => sum + (row.unrealizedPnl ?? 0), 0))} /></div></section></div>;
    if (kind === "recommendations") return <div className="markets-page"><div className="markets-page-heading"><div><p className="eyebrow">PORTFOLIO INTELLIGENCE</p><h2>Recommendations</h2><p className="muted-copy">Evidence-led actions for current holdings and trade-book names. Recommendations are research guidance, not orders.</p></div><FreshnessBadge provenance={dashboard.portfolio[0]?.quote.provenance ?? (dashboard.health.tcp_reachable ? { provider: "futu-opend", retrieved_at: dashboard.retrieved_at, freshness: "Recent", realtime: false } : { provider: "markets", retrieved_at: dashboard.retrieved_at, freshness: "Unavailable", realtime: false })} /></div><section className="market-panel recommendation-add-panel"><div className="market-panel-heading"><div><p className="eyebrow">ADD A COMPANY</p><h3>Get a recommendation directly</h3><span className="market-muted-label">Enter any supported public market symbol.</span></div></div><div className="recommendation-add-form"><CompanyAutocomplete value={recommendationInput} onChange={setRecommendationInput} suggestions={matchingCompanySuggestions} onSelect={(suggestion) => void addRecommendationCompany(suggestion.symbol)} onSubmit={() => void addRecommendationCompany()} placeholder="Ticker or company name" ariaLabel="Ticker or company name" /><button className="primary-button" onClick={() => void addRecommendationCompany()} disabled={!recommendationInput.trim()}><Sparkles size={14} /> Get recommendation</button></div></section>{recommendationLoading && <div className="recommendation-loading"><RefreshCw size={15} className="spin-icon" /> Refreshing quotes, history, valuation, research, and live regime…</div>}<section className="recommendation-card-grid">{recommendationSymbols.map((symbol) => { const data = recommendationData[symbol]; const analyzer = data?.analyzer; const decision = data ? recommendationDecision(data, symbol, thesisEvidence) : { action: "WATCH" as const, summary: "Loading sourced inputs…", actions: ["Waiting for quote and history", "No trade action is implied while inputs are loading"] }; const quote = data?.quote; const news = analyzer?.outlook.relevantNews.slice(0, 2) ?? data?.research?.news.slice(0, 2) ?? []; return <article className="recommendation-card" key={symbol}><div className="recommendation-card-head"><div><span className="eyebrow">{heldRecommendationSymbols.has(symbol) ? "LOCAL HOLDING" : "ADDED FOR REVIEW"}</span><h3>{quote?.name ?? symbol}</h3><small>{symbol}{quote?.last !== null && quote?.last !== undefined ? ` · ${quote.currency} ${formatNumber(quote.last)}` : ""}</small></div><div className="recommendation-card-actions"><b className={`recommendation-action recommendation-${decision.action.toLowerCase()}`}>{decision.action}</b><button className="table-action-button" onClick={() => removeRecommendationCompany(symbol)}>Remove</button></div></div>{quote && <FreshnessBadge provenance={quote.provenance} />}{analyzer ? <div className="recommendation-stats"><Metric label="Timing / factor score" value={`${analyzer.score >= 0 ? "+" : ""}${analyzer.score}`} tone={analyzer.score >= 0 ? "positive" : "negative"} /><Metric label="Confidence" value={`${analyzer.confidence}%`} /><Metric label="Valuation" value={data.valuation?.coverage ? data.valuation.detail.split(";")[0] : "Partial"} /></div> : <div className="recommendation-unavailable">{data?.error ?? "Waiting for at least 80 sourced daily candles before producing a directional signal."}</div>}<p className="recommendation-summary">{decision.summary}</p><ThesisSummary symbol={symbol} evidence={thesisEvidence} quote={quote} onOpen={() => openThesis(symbol)} /><div className="recommendation-action-list">{decision.actions.map((action) => <div key={action}><Check size={13} /> <span>{action}</span></div>)}</div>{analyzer && <div className="recommendation-outlook"><div><strong>Short term</strong><p>{analyzer.outlook.shortTerm}</p></div><div><strong>Long term</strong><p>{analyzer.outlook.longTerm}</p></div></div>}{news.length > 0 && <div className="recommendation-news"><strong>Relevant evidence</strong>{news.map((item) => <span key={`${item.source}-${item.title}`}>{item.title} · {item.source}</span>)}</div>}<div className="recommendation-card-footer"><button className="text-button" onClick={() => void loadSecurity(symbol)}>Open full analysis <ChevronRight size={13} /></button><button className="text-button" onClick={() => setView("transactions")}>View transactions <ChevronRight size={13} /></button></div></article>; })}</section>{!recommendationSymbols.length && <section className="market-panel"><div className="market-table-empty">Add a ticker above to create a recommendation card. Held companies will appear here automatically when the portfolio ledger is loaded.</div></section>}</div>;
    return <div className="markets-page"><div className="markets-page-heading"><div><p className="eyebrow">PORTFOLIO</p><h2>Performance</h2><p className="muted-copy">Track reconstructed holdings over time, then compare the time-weighted trade-book return with Hong Kong and US index benchmarks.</p></div><FreshnessBadge provenance={dashboard.portfolio[0]?.quote.provenance ?? (dashboard.health.tcp_reachable ? { provider: "futu-opend", retrieved_at: dashboard.retrieved_at, freshness: "Recent", realtime: false } : { provider: "no-quotes", retrieved_at: dashboard.retrieved_at, freshness: "Unavailable", realtime: false })} /></div><div className="portfolio-summary"><Metric label="Current value" value={formatCompact(totalValue)} /><Metric label="Holding P/L" value={formatNumber(totalPnl)} tone={totalPnl >= 0 ? "positive" : "negative"} /><Metric label="Trade-book P/L" value={formatNumber(tradePnl)} tone={tradePnl >= 0 ? "positive" : "negative"} /><Metric label="All commissions" value={formatNumber(commissionTotal)} /></div><section className="market-panel performance-chart-panel"><div className="market-panel-heading"><div><p className="eyebrow">HOLDINGS OVER TIME</p><h3>Portfolio vs benchmarks</h3><span className="market-muted-label">Portfolio is indexed to 100 at the selected-period start; hover shows the reconstructed holdings value in HKD.</span></div><span className="market-muted-label">{performanceChartData.dates.length ? `${performanceChartData.dates.length} sessions` : "Waiting for history"}</span></div><div className="performance-toolbar"><div className="chart-control-group"><span>Range</span>{([ ["3M", "3M"], ["6M", "6M"], ["1Y", "1Y"], ["2Y", "2Y"], ["ALL", "All"] ] as [PerformanceRange, string][]).map(([value, label]) => <button className={performanceRange === value ? "selected" : ""} key={value} onClick={() => setPerformanceRange(value)}>{label}</button>)}</div><div className="performance-benchmark-picker"><span>Compare</span>{PERFORMANCE_BENCHMARKS.map((benchmark) => <button className={selectedPerformanceBenchmarks.includes(benchmark.symbol) ? "selected" : ""} key={benchmark.symbol} aria-pressed={selectedPerformanceBenchmarks.includes(benchmark.symbol)} title={`${benchmark.label} · ${benchmark.category}`} onClick={() => setSelectedPerformanceBenchmarks((current) => current.includes(benchmark.symbol) ? current.filter((symbol) => symbol !== benchmark.symbol) : [...current, benchmark.symbol])}>{benchmark.label}</button>)}</div></div><PerformanceChart data={performanceChartData} loading={performanceLoading} /></section><section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">TRADE-BOOK PERFORMANCE</p><h3>Realized + unrealized after costs</h3></div><span className="market-muted-label">Average-cost matching · research ledger</span></div><TradePerformanceTable rows={tradePerformance} onSelect={(symbol) => void loadSecurity(symbol)} /></section><section className="market-panel"><div className="market-panel-heading"><div><p className="eyebrow">CURRENT HOLDINGS</p><h3>Latest sourced mark</h3></div><span className="market-muted-label">{dashboard.trading_mode}</span></div><HoldingsTable rows={dashboard.portfolio} onSelect={(symbol) => void loadSecurity(symbol)} /></section></div>;
  }

  function renderUniverse() {
    if (!universe) return <div className="market-loading"><RefreshCw size={18} className={universeLoading ? "spin-icon" : ""} /> Loading Hang Seng constituent quotes…</div>;
    const quotes = universeTab === "hsi" ? universe.hsi : universe.hstech;
    const normalizedSearch = universeSearch.trim().toLowerCase();
    const filteredQuotes = quotes.filter((item) => !normalizedSearch || item.symbol.toLowerCase().includes(normalizedSearch) || item.name.toLowerCase().includes(normalizedSearch));
    const liveCount = quotes.filter((item) => item.provenance.freshness === "Live").length;
    const delayedCount = quotes.filter((item) => item.provenance.freshness !== "Live").length;
    const candidates = universeAnalysisTab === universeTab ? universeCandidates : [];
    const bestBuy = candidates.find((candidate) => candidate.analyzer?.verdict === "BUY");
    const universeContext = JSON.stringify({ index: universeTab === "hsi" ? "HSI" : "Hang Seng TECH", candidates: candidates.slice(0, 6).map((candidate) => ({ symbol: candidate.quote.symbol, name: candidate.quote.name, last: candidate.quote.last, change_pct: candidate.quote.change_pct, industry: candidate.quote.industry, freshness: candidate.quote.provenance.freshness, analyzer: candidate.analyzer, news: candidate.research?.news.slice(0, 3) ?? [] })), thematic: universe.thematic });
      return <div className="markets-page"><div className="markets-page-heading"><div><p className="eyebrow">INDEX UNIVERSE</p><h2>Hang Seng companies</h2><p className="muted-copy">Official HSI and Hang Seng TECH constituents with Futu/OpenD quotes first and labelled public fallbacks when needed.</p></div><button className="secondary-button" onClick={() => void loadUniverse()} disabled={universeLoading}><RefreshCw size={14} className={universeLoading ? "spin-icon" : ""} /> Refresh quotes</button></div><div className="portfolio-summary"><Metric label="HSI constituents" value={universe.hsi.length.toString()} /><Metric label="HSTECH constituents" value={universe.hstech.length.toString()} /><Metric label="Live now" value={liveCount.toString()} tone="positive" /><Metric label="Delayed / other" value={delayedCount.toString()} /></div><section className="market-panel universe-analysis-panel"><div className="market-panel-heading"><div><p className="eyebrow">QUANT UNIVERSE SCAN</p><h3>Best research candidate to buy</h3><span className="market-muted-label">Thematic technology + fundamentals + valuation + commercial proof + live regime · research shortlist only</span></div><button className="primary-button" onClick={() => void analyzeBestUniverse(universeTab)} disabled={universeAnalyzing}>{universeAnalyzing ? <><RefreshCw size={14} className="spin-icon" /> Analyzing {universeTab === "hsi" ? "HSI" : "HSTECH"}…</> : <><Activity size={14} /> Analyze best {universeTab === "hsi" ? "HSI" : "HSTECH"} stock</>}</button></div>{candidates.length ? <><div className="universe-candidate-grid">{candidates.slice(0, 3).map((candidate, index) => { const candidateTone = candidate.analyzer?.verdict === "BUY" ? "positive" : candidate.analyzer?.verdict === "SELL" ? "negative" : undefined; const leadingFactors = candidate.analyzer?.factors.filter((factor) => Math.abs(factor.score) > 0.18).sort((left, right) => Math.abs(right.score) - Math.abs(left.score)).slice(0, 3).map((factor) => `${factor.score > 0 ? "+" : ""}${factor.label}`).join(" · "); return <article className="universe-candidate-card" key={candidate.quote.symbol}><span>#{index + 1} · {candidate.quote.symbol}</span><strong>{candidate.quote.name}</strong><b className={candidateTone ? `market-${candidateTone}` : ""}>{candidate.analyzer?.verdict ?? "DATA"} · score {candidate.score >= 0 ? "+" : ""}{candidate.score.toFixed(0)}</b><small>{formatPercent(candidate.quote.change_pct)} today · {candidate.analyzer ? `${candidate.analyzer.confidence}% confidence · ${candidate.analyzer.dataCoverage * 100 >= 99 ? "full" : "partial"} inputs` : "history unavailable"}</small>{leadingFactors && <small>Drivers: {leadingFactors}</small>}<FreshnessBadge provenance={candidate.quote.provenance} /><button className="text-button" onClick={() => void loadSecurity(candidate.quote.symbol)}>Open analysis <ChevronRight size={13} /></button></article>; })}</div><p className="muted-copy universe-analysis-note">{bestBuy ? `Top BUY candidate: ${bestBuy.quote.symbol} ${bestBuy.quote.name}.` : "No candidate currently clears the BUY threshold; the shortlist is ranked for further research rather than forcing a trade."} The score is not a forecast and does not place orders.</p></> : <div className="market-table-empty">Choose HSI or Hang Seng TECH, then run the scan. It evaluates a diversified pool of technology themes, then validates fundamentals, commercial evidence, valuation, live regime, and later-candle behavior.</div>}</section><section className="market-panel"><div className="universe-toolbar"><div className="watchlist-tabs"><button className={universeTab === "hstech" ? "selected" : ""} onClick={() => { setUniverseTab("hstech"); setUniverseCandidates([]); }}>Hang Seng TECH <small>{universe.hstech.length}</small></button><button className={universeTab === "hsi" ? "selected" : ""} onClick={() => { setUniverseTab("hsi"); setUniverseCandidates([]); }}>HSI <small>{universe.hsi.length}</small></button></div><label className="universe-search"><Search size={14} /><input value={universeSearch} onChange={(event) => setUniverseSearch(event.target.value)} placeholder="Filter ticker or company" /></label></div><UniverseTable quotes={filteredQuotes} onSelect={(symbol) => void loadSecurity(symbol)} onToggle={(symbol) => void toggleWatchlistSymbol(symbol)} watchlistSymbols={selectedWatchlist?.symbols ?? []} /></section><MarketsCopilot target="universe" request={copilotRequest} defaultPrompt={`Review the strongest candidates in the ${universeTab === "hsi" ? "HSI" : "Hang Seng TECH"} universe and explain the leading factor evidence and risks.`} context={universeContext} /><div className="market-source-note universe-source-note"><span>Constituent source</span><strong>Hang Seng Indexes official lists</strong><small>{universe.constituent_source} Retrieved {new Date(universe.retrieved_at).toLocaleTimeString()}.</small><div><a href="https://www.hsi.com.hk/eng/indexes/all-indexes/hsi" target="_blank" rel="noreferrer">HSI source</a><a href="https://origin-www.hsi.com.hk/eng/indexes/all-indexes/hstech" target="_blank" rel="noreferrer">HSTECH source</a></div></div></div>;
  }

  function renderView() {
    if (view === "thesis") return <InvestmentThesis initialSymbol={thesisSymbol} evidence={thesisEvidence} onEvidenceChange={saveThesisEvidence} ledger={thesisPositions} ledgerReady={Boolean(dashboard)} onOpenCompany={(symbol) => void loadSecurity(symbol)} />;
    if (view === "dashboard") return renderDashboard();
    if (view === "companies") return renderCompany();
    if (view === "portfolio") return renderPortfolio();
    if (view === "universe") return renderUniverse();
    if (view === "holdings") return renderHoldings();
    if (view === "performance" || view === "recommendations" || view === "transactions" || view === "risk") return renderPortfolioSurface(view);
    if (["factors", "signals", "screener", "backtests"].includes(view)) return renderQuant(view);
    if (view === "paper" || view === "orders" || view === "history") return renderTrading(view);
    if (view === "news" || view === "filings" || view === "research") return renderResearchView(view);
    if (view === "radar") return <div className="markets-page"><div className="markets-page-heading"><div><p className="eyebrow">MARKET RADAR</p><h2>Events</h2></div></div><section className="market-panel">{dashboard?.radar.length ? <div className="market-event-list">{dashboard.radar.map((event) => <div className="market-event" key={event.id}><span className={`event-severity ${event.severity}`} /><div><strong>{event.title}</strong><p>{event.detail}</p><FreshnessBadge provenance={event.provenance} /></div></div>)}</div> : renderUnavailable("No radar events", "Radar only reports deterministic events from available quotes; it does not invent news or causes.")}</section></div>;
    if (view === "watchlist") return <div className="markets-page"><div className="markets-page-heading"><div><p className="eyebrow">WATCHLISTS</p><h2>{selectedWatchlist?.name ?? "Main"}</h2><p className="muted-copy">Click a row to research it, or remove it directly here.</p></div><button className="secondary-button" onClick={() => void loadDashboard(true)}><RefreshCw size={14} /> Refresh</button></div><div className="watchlist-tabs">{dashboard?.watchlists.map((item) => <button className={item.id === selectedWatchlist?.id ? "selected" : ""} key={item.id} onClick={() => setWatchlistId(item.id)}>{item.name}<small>{item.symbols.length}</small></button>)}</div><section className="market-panel"><QuoteTable quotes={dashboard?.watchlist_quotes.filter((quote) => selectedWatchlist?.symbols.includes(quote.symbol)) ?? []} onSelect={(symbol) => void loadSecurity(symbol)} onRemove={(symbol) => void toggleWatchlistSymbol(symbol)} /></section></div>;
    return renderUnavailable("This Markets surface is ready for a provider", "The surface is separated and source-aware, but no provider response is being substituted. Configure the local providers in Settings before using it.");
  }

  return <div className="markets-workspace"><div className="markets-workspace-head"><div className="markets-brand"><div className="markets-brand-mark"><CandlestickChart size={18} /></div><div><p className="eyebrow">JADX MARKETS</p><strong>Local equity workstation</strong></div></div><div className="markets-search markets-search-autocomplete"><CompanyAutocomplete value={search} onChange={setSearch} suggestions={matchingSearchSuggestions} onSelect={(suggestion) => { setSearch(suggestion.symbol); void loadSecurity(suggestion.symbol); }} onSubmit={() => void loadSecurity(search)} placeholder="Search by market symbol" ariaLabel="Markets company search" /><kbd>↵</kbd>{loading && <RefreshCw size={14} className="spin-icon" />}</div><div className="markets-head-actions"><span className={`markets-connection-dot ${dashboard?.health.tcp_reachable ? "online" : "offline"}`} /><span>{dashboard?.health.connection_state ?? "Checking OpenD"}</span>{dashboard?.fixture_mode && <b className="simulated-badge">SIMULATED</b>}<button className="icon-button quiet" onClick={onOpenSettings} aria-label="Open Markets settings"><SlidersHorizontal size={16} /></button></div></div><LiveMarketDeck quote={quote} macro={macroSnapshot} health={dashboard?.health} /><div className="markets-body"><nav className="markets-nav" aria-label="Markets navigation">{NAV.map((group) => <div key={group.group}><span className="markets-nav-group">{group.group}</span>{group.items.map((item) => { const Icon = item.icon; return <button className={view === item.id ? "selected" : ""} key={item.id} onClick={() => setView(item.id)}><Icon size={14} /><span>{item.label}</span></button>; })}</div>)}<div className="markets-nav-footer"><span className="markets-nav-group">Provider</span><button onClick={onOpenSettings}><Activity size={14} /><span>Futu / OpenD</span><small>{dashboard?.health.tcp_reachable ? "OpenD connected" : dashboard?.health.sdk_available ? "SDK ready" : "Not configured"}</small></button><button onClick={onOpenSettings}><ShieldCheck size={14} /><span>Trading</span><small>{dashboard?.trading_mode ?? "OFF"}</small></button></div></nav><main className="markets-main">{renderView()}</main></div></div>;
}

function QuoteTable({ quotes, onSelect, onRemove }: { quotes: Quote[]; onSelect: (symbol: string) => void; onRemove?: (symbol: string) => void }) {
  if (!quotes.length) return <div className="market-table-empty">No sourced quotes in this watchlist.</div>;
  if (!onRemove) return <div className="market-table"><div className="market-table-row market-table-head"><span>Security</span><span>Last</span><span>Day</span><span>Volume</span><span>Source</span></div>{quotes.map((quote) => <button className="market-table-row" key={quote.symbol} onClick={() => onSelect(quote.symbol)}><span><strong>{quote.symbol}</strong><small>{quote.name}</small></span><span>{formatNumber(quote.last)}</span><span className={(quote.change_pct ?? 0) >= 0 ? "market-positive" : "market-negative"}>{formatPercent(quote.change_pct)}</span><span>{formatCompact(quote.volume)}</span><span><FreshnessBadge provenance={quote.provenance} /></span></button>)}</div>;
  return <div className="market-table watchlist-quote-table"><div className="market-table-row market-table-head"><span>Security</span><span>Last</span><span>Day</span><span>Volume</span><span>Source</span><span>Action</span></div>{quotes.map((quote) => <div className="market-table-row" key={quote.symbol}><span><button className="table-row-link" onClick={() => onSelect(quote.symbol)}><strong>{quote.symbol}</strong><small>{quote.name}</small></button></span><span>{formatNumber(quote.last)}</span><span className={(quote.change_pct ?? 0) >= 0 ? "market-positive" : "market-negative"}>{formatPercent(quote.change_pct)}</span><span>{formatCompact(quote.volume)}</span><span><FreshnessBadge provenance={quote.provenance} /></span><span><button className="table-action-button" onClick={() => onRemove(quote.symbol)}>Remove</button></span></div>)}</div>;
}

function UniverseTable({ quotes, onSelect, onToggle, watchlistSymbols }: { quotes: Quote[]; onSelect: (symbol: string) => void; onToggle: (symbol: string) => void; watchlistSymbols: string[] }) {
  if (!quotes.length) return <div className="market-table-empty">No constituents match this filter.</div>;
  return <div className="market-table universe-table"><div className="market-table-row market-table-head"><span>Security</span><span>Last</span><span>Day</span><span>Volume</span><span>Source</span><span>Watchlist</span></div>{quotes.map((quote) => { const isInWatchlist = watchlistSymbols.includes(quote.symbol); return <div className="market-table-row" key={quote.symbol}><span><button className="table-row-link" onClick={() => onSelect(quote.symbol)}><strong>{quote.symbol}</strong><small>{quote.name}</small></button></span><span>{formatNumber(quote.last)}</span><span className={(quote.change_pct ?? 0) >= 0 ? "market-positive" : "market-negative"}>{formatPercent(quote.change_pct)}</span><span>{formatCompact(quote.volume)}</span><span><FreshnessBadge provenance={quote.provenance} /></span><span><button className={`table-action-button ${isInWatchlist ? "active" : ""}`} onClick={() => onToggle(quote.symbol)}>{isInWatchlist ? "Remove" : "Add"}</button></span></div>; })}</div>;
}

function HoldingsTable({ rows, onSelect }: { rows: PortfolioRow[]; onSelect: (symbol: string) => void }) {
  if (!rows.length) return <div className="market-table-empty">No positions in the local ledger.</div>;
  return <div className="market-table holdings-table"><div className="market-table-row market-table-head"><span>Holding</span><span>Shares</span><span>Price</span><span>Value</span><span>P/L</span><span>Broker</span></div>{rows.map((row) => <button className="market-table-row" key={row.holding.id} onClick={() => onSelect(row.holding.symbol)}><span><strong>{row.holding.symbol}</strong><small>{row.holding.account} · cost {formatNumber(row.holding.average_cost)}</small></span><span>{formatNumber(row.holding.shares, 0)}</span><span>{formatNumber(row.quote.last)}</span><span>{formatNumber(row.market_value)}</span><span className={(row.unrealized_pnl ?? 0) >= 0 ? "market-positive" : "market-negative"}>{formatNumber(row.unrealized_pnl)}<small>{formatPercent(row.unrealized_pnl_pct)}</small></span><span>{row.holding.source}</span></button>)}</div>;
}

function TradeLedgerTable({ trades, onSelect, onEdit, onRemove }: { trades: PortfolioTrade[]; onSelect: (symbol: string) => void; onEdit: (trade: PortfolioTrade) => void; onRemove: (trade: PortfolioTrade) => void }) {
  if (!trades.length) return <div className="market-table-empty">No historical trades recorded yet. Add a dated BUY or SELL to start the audit trail.</div>;
  return <div className="market-table trade-ledger-table"><div className="market-table-row market-table-head"><span>Date</span><span>Security</span><span>Side / quantity</span><span>Execution</span><span>Commission</span><span>Notes</span><span>Action</span></div>{trades.map((trade) => <div className="market-table-row" key={trade.id}><span>{trade.trade_date}</span><span><button className="table-row-link" onClick={() => onSelect(trade.symbol)}><strong>{trade.symbol}</strong><small>{trade.account}</small></button></span><span className={trade.side === "BUY" ? "market-positive" : "market-negative"}>{trade.side} · {formatNumber(trade.quantity, 0)}</span><span>{formatNumber(trade.price)}<small>{formatCompact(trade.quantity * trade.price)} notional</small></span><span>{formatNumber(trade.commission)}</span><span title={trade.notes}>{trade.notes || "—"}</span><span className="trade-ledger-actions"><button className="table-action-button" onClick={() => onEdit(trade)}>Edit</button><button className="table-action-button danger" onClick={() => onRemove(trade)}>Remove</button></span></div>)}</div>;
}

function TradePerformanceTable({ rows, onSelect }: { rows: TradePerformance[]; onSelect: (symbol: string) => void }) {
  if (!rows.length) return <div className="market-table-empty">Record historical trades to see realized P/L, open cost basis, commissions, and mark-to-market performance.</div>;
  return <div className="market-table trade-performance-table"><div className="market-table-row market-table-head"><span>Position</span><span>Shares / avg cost</span><span>Market value</span><span>Realized</span><span>Unrealized</span><span>Costs / return</span></div>{rows.map((row) => <button className="market-table-row" key={`${row.account}-${row.symbol}`} onClick={() => onSelect(row.symbol)}><span><strong>{row.symbol}</strong><small>{row.account} · {row.trades} trades</small></span><span>{formatNumber(row.shares, 0)}<small>{formatNumber(row.averageCost)} avg</small></span><span>{formatNumber(row.marketValue)}</span><span className={row.realizedPnl >= 0 ? "market-positive" : "market-negative"}>{formatNumber(row.realizedPnl)}</span><span className={(row.unrealizedPnl ?? 0) >= 0 ? "market-positive" : "market-negative"}>{formatNumber(row.unrealizedPnl)}</span><span>{formatNumber(row.commissions)} fees<small className={(row.returnPct ?? 0) >= 0 ? "market-positive" : "market-negative"}>{formatPercent(row.returnPct)} total</small></span></button>)}</div>;
}

export default MarketsWorkspace;
