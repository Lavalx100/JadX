export type ModelPricing = {
  inputPerMillion: number;
  cachedInputPerMillion: number;
  outputPerMillion: number;
};

export type PricingMap = Record<string, ModelPricing>;
export type CostEstimate = { amount: number; complete: boolean } | null;

export const DEEPSEEK_PRICING_SOURCE = "https://api-docs.deepseek.com/quick_start/pricing/";
export const DEEPSEEK_PRICING_CHECKED = "2026-10-24";

const deepseekRates = {
  flash: {
    offPeak: { inputPerMillion: 0.15, cachedInputPerMillion: 0.003, outputPerMillion: 0.60 },
    peak: { inputPerMillion: 0.30, cachedInputPerMillion: 0.006, outputPerMillion: 1.20 },
  },
  pro: {
    offPeak: { inputPerMillion: 0.66, cachedInputPerMillion: 0.022, outputPerMillion: 1.98 },
    peak: { inputPerMillion: 1.32, cachedInputPerMillion: 0.044, outputPerMillion: 3.96 },
  },
} satisfies Record<string, Record<string, ModelPricing>>;

export function pricingKey(providerId: string, model: string) {
  return `${providerId}:${model.trim()}`;
}

export function defaultModelPricing(providerId: string, model: string, tier: "offPeak" | "peak" = "offPeak"): ModelPricing | null {
  if (providerId !== "deepseek") return null;
  const normalized = model.trim().toLowerCase();
  if (normalized === "deepseek-flash" || normalized === "deepseek-v4-flash") return deepseekRates.flash[tier];
  if (normalized === "deepseek-v4-pro") return deepseekRates.pro[tier];
  return null;
}

export function normalizePricingMap(value: unknown): PricingMap {
  if (!value || typeof value !== "object" || Array.isArray(value)) return {};
  const result: PricingMap = {};
  for (const [key, candidate] of Object.entries(value)) {
    if (!candidate || typeof candidate !== "object") continue;
    const rate = candidate as Record<string, unknown>;
    const input = rate.inputPerMillion;
    const cached = rate.cachedInputPerMillion;
    const output = rate.outputPerMillion;
    if ([input, cached, output].every((item) => typeof item === "number" && Number.isFinite(item) && item >= 0)) {
      result[key] = { inputPerMillion: input as number, cachedInputPerMillion: cached as number, outputPerMillion: output as number };
    }
  }
  return result;
}

export function modelPricing(providerId: string, model: string, pricing: PricingMap): ModelPricing | null {
  return pricing[pricingKey(providerId, model)] ?? defaultModelPricing(providerId, model);
}

export function estimateUsageCost(
  providerId: string,
  model: string,
  usage: {
    input_tokens?: number | null;
    cached_input_tokens?: number | null;
    cache_miss_tokens?: number | null;
    output_tokens?: number | null;
    requests?: number;
    input_metrics_reported_requests?: number;
    output_metrics_reported_requests?: number;
    cache_metrics_reported_requests?: number;
  },
  pricing: PricingMap,
): CostEstimate {
  if (providerId === "local") return null;
  const rate = modelPricing(providerId, model, pricing);
  if (!rate) return null;

  let amount = 0;
  let measured = false;
  const cached = usage.cached_input_tokens ?? 0;
  const input = usage.input_tokens ?? (usage.cache_miss_tokens != null || usage.cached_input_tokens != null
    ? (usage.cache_miss_tokens ?? 0) + cached
    : null);
  if (input != null) {
    amount += Math.max(0, input - cached) * rate.inputPerMillion / 1_000_000;
    amount += cached * rate.cachedInputPerMillion / 1_000_000;
    measured = true;
  }
  if (usage.output_tokens != null) {
    amount += usage.output_tokens * rate.outputPerMillion / 1_000_000;
    measured = true;
  }
  if (!measured) return null;
  const complete = usage.requests == null || (
    (usage.input_metrics_reported_requests ?? 0) >= usage.requests
    && (usage.output_metrics_reported_requests ?? 0) >= usage.requests
    && (rate.cachedInputPerMillion === rate.inputPerMillion
      || (usage.cache_metrics_reported_requests ?? 0) >= usage.requests)
  );
  return { amount, complete };
}

export function formatUsd(amount: number) {
  const digits = amount > 0 && amount < 0.01 ? 6 : 2;
  return new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", minimumFractionDigits: digits, maximumFractionDigits: digits }).format(amount);
}
