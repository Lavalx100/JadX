import { describe, expect, it } from "vitest";
import { hkuToolLimitation } from "./providerCapabilities";

describe("HKU model tool capabilities", () => {
  it.each([
    "qwen/qwen3-next-80b-a3b-instruct-maas",
    "qwen/qwen3-coder-480b-a35b-instruct-maas",
    "zai-org/glm-4.7-maas",
    "DeepSeek-V4-Flash",
    "gpt-5-mini",
    "gpt-5.3-codex",
  ])("allows JadX tools for supported chat model %s", (model) => {
    expect(hkuToolLimitation(model)).toBeNull();
  });

  it.each([
    ["claude-opus-4.6", "text-only Converse"],
    ["gemini-3.1-pro-preview", "URL-context"],
    ["gpt-image-2", "image-generation"],
    ["text-embedding-3-small", "embedding"],
    ["unknown-deployment", "not verified"],
  ])("explains why %s cannot use tools", (model, reason) => {
    expect(hkuToolLimitation(model)).toContain(reason);
  });
});
