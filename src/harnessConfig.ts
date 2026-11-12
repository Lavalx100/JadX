export const CONTEXT_CANDIDATES = [8192, 12288, 16384, 24576, 32768, 49152, 65536] as const;

export function clampContext(requested: number, maximum = 262144) {
  const safeMaximum = Math.max(4096, maximum);
  return Math.max(4096, Math.min(safeMaximum, Math.round(requested / 1024) * 1024));
}

export function profileDefaults(profile: string) {
  if (profile === "FAST") return { thinkingMode: "Off", reasoningEffort: "Low" };
  if (profile === "DEEP") return { thinkingMode: "On", reasoningEffort: "Medium" };
  if (profile === "AGENT") return { thinkingMode: "On", reasoningEffort: "Medium" };
  return { thinkingMode: "On", reasoningEffort: "Low" };
}
