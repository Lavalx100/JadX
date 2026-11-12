/** Returns a user-facing explanation when an HKU deployment cannot accept JadX function tools. */
export function hkuToolLimitation(model: string | null | undefined): string | null {
  const key = model?.trim().toLowerCase() ?? "";
  if (!key) return null;
  if (key.startsWith("claude-")) {
    return "This HKU Claude deployment uses a text-only Converse offering and does not accept JadX function tools.";
  }
  if (key.startsWith("gemini-")) {
    return "This HKU Gemini ITS deployment only permits its provider-side URL-context tool; JadX tools are not accepted on this route.";
  }
  if (key.startsWith("gpt-image-")) {
    return "This HKU image-generation deployment generates images but does not accept chat messages or JadX function tools.";
  }
  if (key.startsWith("text-embedding-")) {
    return "This HKU embedding deployment produces embeddings, not chat responses, and cannot use JadX function tools.";
  }
  if (
    key.startsWith("deepseek-") ||
    key.startsWith("qwen/") ||
    key.startsWith("zai-org/") ||
    key.startsWith("gpt-4.1") ||
    key.startsWith("gpt-5") ||
    key === "o4-mini"
  ) return null;
  return "JadX has not verified function-tool support for this HKU deployment. Select a listed Qwen/GLM, DeepSeek, or Azure chat model.";
}
