export type HkuRequestUsage = {
  model: string;
  last_minute: number;
  last_day: number;
  last_week: number;
  failed_last_day: number;
};

export type HkuQuotaRule = {
  id: string;
  label: string;
  models: string[];
  per_minute?: number;
  per_day?: number;
  per_week?: number;
};

export type HkuQuotaGroup = { id: string; label: string; rules: HkuQuotaRule[] };

const claudeModels = ["claude-haiku-4.5", "claude-sonnet-4.6", "claude-sonnet-5", "claude-opus-4.6"];
const deepseekModels = ["DeepSeek-V3.2", "DeepSeek-V4-Flash", "DeepSeek-V4-Pro"];
const qwenGlmModels = [
  "qwen/qwen3-next-80b-a3b-instruct-maas",
  "qwen/qwen3-next-80b-a3b-thinking-maas",
  "qwen/qwen3-coder-480b-a35b-instruct-maas",
  "qwen/qwen3-235b-a22b-instruct-2507-maas",
  "zai-org/glm-4.7-maas",
];
const geminiTextModels = ["gemini-2.5-flash", "gemini-3-flash-preview", "gemini-3.1-pro-preview", "gemini-3.5-flash"];
const geminiImageModels = ["gemini-2.5-flash-image", "gemini-3.1-flash-image-preview", "gemini-3-pro-image-preview"];
const azureModels = [
  "gpt-4.1-nano", "gpt-4.1-mini", "gpt-4.1", "o4-mini", "gpt-5-nano", "gpt-5-mini", "gpt-5.1", "gpt-5.3-codex", "gpt-5.4", "gpt-5.5",
  "gpt-image-1.5", "gpt-image-2", "text-embedding-ada-002", "text-embedding-3-small",
];

const individual = (groupId: string, model: string, perMinute?: number, perDay?: number, perWeek?: number): HkuQuotaRule => ({
  id: `${groupId}:${model}`,
  label: model,
  models: [model],
  ...(perMinute == null ? {} : { per_minute: perMinute }),
  ...(perDay == null ? {} : { per_day: perDay }),
  ...(perWeek == null ? {} : { per_week: perWeek }),
});

export const HKU_QUOTA_GROUPS: HkuQuotaGroup[] = [
  {
    id: "claude",
    label: "AWS Bedrock Claude",
    rules: claudeModels.map((model) => individual("claude", model, 3, model === "claude-opus-4.6" ? 150 : 300)),
  },
  {
    id: "deepseek",
    label: "Azure DeepSeek",
    rules: [{ id: "deepseek:daily", label: "DeepSeek family · shared daily allowance", models: deepseekModels, per_day: 500 }],
  },
  {
    id: "qwen-glm",
    label: "Vertex Qwen / GLM",
    rules: [
      ...qwenGlmModels.map((model) => individual("qwen-glm", model, 5, 300)),
      { id: "qwen-glm:shared", label: "Qwen / GLM family · shared daily maximum", models: qwenGlmModels, per_day: 500 },
    ],
  },
  {
    id: "azure-openai",
    label: "Azure OpenAI",
    rules: [{ id: "azure-openai:daily", label: "Azure OpenAI family · shared daily maximum", models: azureModels, per_day: 500 }],
  },
  {
    id: "gemini-text",
    label: "Vertex Gemini · text models",
    rules: [
      ...geminiTextModels.map((model) => individual("gemini-text", model, 3, 300)),
      { id: "gemini-text:shared", label: "Gemini text models · shared daily maximum", models: geminiTextModels, per_day: 500 },
    ],
  },
  {
    id: "gemini-image",
    label: "Vertex Gemini · image generation",
    rules: [
      individual("gemini-image", "gemini-2.5-flash-image", 2, undefined, 70),
      individual("gemini-image", "gemini-3.1-flash-image-preview", 2, undefined, 35),
      individual("gemini-image", "gemini-3-pro-image-preview", 2, undefined, 15),
    ],
  },
];

export function observedHkuRequests(rule: HkuQuotaRule, requests: HkuRequestUsage[] = []) {
  const relevant = requests.filter((item) => rule.models.includes(item.model));
  return relevant.reduce((total, item) => ({
    last_minute: total.last_minute + item.last_minute,
    last_day: total.last_day + item.last_day,
    last_week: total.last_week + item.last_week,
    failed_last_day: total.failed_last_day + item.failed_last_day,
  }), { last_minute: 0, last_day: 0, last_week: 0, failed_last_day: 0 });
}
