import { describe, expect, it } from "vitest";
import { automaticContextLength, providerContextSettingKeys, resolveProviderContext, supportsMaximumReasoning } from "./providerRuntime";

describe("provider-aware IDE runtime policy", () => {
  it("uses memory-aware context for local models and the selected model maximum for cloud", () => {
    expect(automaticContextLength("local", 24_576, 131_072)).toBe(24_576);
    expect(automaticContextLength("deepseek", 24_576, 1_000_000)).toBe(1_000_000);
  });

  it("keeps custom context sizes independent by provider and clamps to each model", () => {
    const local = providerContextSettingKeys("local");
    const deepseek = providerContextSettingKeys("deepseek");
    const settings = {
      [local.mode]: "custom", [local.length]: 32_768,
      [deepseek.mode]: "custom", [deepseek.length]: 262_144,
    };
    expect(resolveProviderContext(settings, "local", 16_384, 65_536)).toEqual({ mode: "custom", length: 32_768 });
    expect(resolveProviderContext(settings, "deepseek", 16_384, 128_000)).toEqual({ mode: "custom", length: 128_000 });
  });

  it("reinterprets legacy auto context for the active provider", () => {
    expect(resolveProviderContext({ context_mode: "auto", context_length: 16_384 }, "openai", 16_384, 128_000))
      .toEqual({ mode: "auto", length: 128_000 });
  });

  it("raises reasoning only for recognized model capability", () => {
    expect(supportsMaximumReasoning("local", "model", true)).toBe(true);
    expect(supportsMaximumReasoning("local", "model", false)).toBe(false);
    expect(supportsMaximumReasoning("deepseek", "deepseek-flash")).toBe(true);
    expect(supportsMaximumReasoning("openai", "gpt-4.1-mini")).toBe(false);
    expect(supportsMaximumReasoning("hku", "gpt-5-mini")).toBe(true);
  });
});
