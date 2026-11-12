export type ContextMode = "auto" | "custom";

export function providerContextSettingKeys(providerId: string) {
  const safeId = providerId.trim().toLowerCase().replace(/[^a-z0-9_-]/g, "_") || "local";
  return {
    length: `context_length_${safeId}`,
    mode: `context_mode_${safeId}`,
  };
}

export function automaticContextLength(providerId: string, localRecommended: number, modelMaximum: number) {
  const maximum = Number.isFinite(modelMaximum) ? Math.max(4096, Math.floor(modelMaximum)) : 16_384;
  const candidate = providerId === "local" ? localRecommended : maximum;
  return Math.min(maximum, Math.max(4096, Number.isFinite(candidate) ? Math.floor(candidate) : 16_384));
}

export function resolveProviderContext(
  settings: Record<string, unknown>,
  providerId: string,
  localRecommended: number,
  modelMaximum: number,
) {
  const keys = providerContextSettingKeys(providerId);
  const savedMode = settings[keys.mode] ?? settings.context_mode;
  const mode: ContextMode = savedMode === "custom" ? "custom" : "auto";
  const automatic = automaticContextLength(providerId, localRecommended, modelMaximum);
  const savedLength = Number(settings[keys.length] ?? settings.context_length ?? automatic);
  const length = mode === "auto"
    ? automatic
    : Math.min(Math.max(4096, Number.isFinite(savedLength) ? Math.floor(savedLength) : automatic), Math.max(4096, modelMaximum));
  return { mode, length };
}

/** Conservative capability check: only raise IDE reasoning when the selected model is known to accept it. */
export function supportsMaximumReasoning(providerId: string, model: string, localCapability = false) {
  if (providerId === "local") return localCapability;
  const key = model.trim().toLowerCase();
  if (providerId === "deepseek") return true;
  if (providerId === "hku") return key.startsWith("deepseek-") || key.includes("thinking") || key.startsWith("zai-org/") || key === "o4-mini" || key.startsWith("gpt-5") || (key.startsWith("gemini-") && !key.includes("image"));
  if (providerId === "openai") return /^(gpt-5|o[134](?:-|$))/.test(key) || key.includes("reasoning");
  return /deepseek|reasoning|thinking|(?:^|\/)o[134](?:-|$)|gpt-5|gemini-2\.[5-9]|qwen3.*(?:think|reason)|glm-4\.[7-9]/.test(key);
}
