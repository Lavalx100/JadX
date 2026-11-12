import { invoke } from "@tauri-apps/api/core";
import {
  Activity, AlertTriangle, Check, ChevronRight, Cloud, Cpu,
  Copy, ExternalLink, FolderOpen, KeyRound, LockKeyhole, Mail, Plus, RefreshCw,
  Search, Settings2, ShieldAlert, ShieldCheck, SlidersHorizontal, X,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { defaultModelPricing, DEEPSEEK_PRICING_CHECKED, DEEPSEEK_PRICING_SOURCE, modelPricing, pricingKey, type ModelPricing, type PricingMap } from "./modelPricing";

type ProviderId = "deepseek" | "openrouter" | "openai" | "custom" | "hku";
type ProviderConfig = { baseUrl: string; model: string; supportsVision: boolean; maxContextLength: number };
type Workspace = { id: string; name: string; root_path: string };
type Model = { key: string; display_name?: string; max_context_length?: number; capabilities?: { vision?: boolean } };
type SettingsSection = "general" | "providers" | "performance" | "security" | "workspaces" | "email";

const defaults: Record<ProviderId, ProviderConfig> = {
  deepseek: { baseUrl: "https://api.deepseek.com", model: "deepseek-flash", supportsVision: false, maxContextLength: 1_000_000 },
  openrouter: { baseUrl: "https://openrouter.ai/api/v1", model: "", supportsVision: false, maxContextLength: 131_072 },
  openai: { baseUrl: "https://api.openai.com/v1", model: "gpt-4.1-mini", supportsVision: true, maxContextLength: 128_000 },
  custom: { baseUrl: "https://api.example.com/v1", model: "", supportsVision: false, maxContextLength: 32_768 },
  hku: { baseUrl: "https://api.hku.hk", model: "qwen/qwen3-next-80b-a3b-instruct-maas", supportsVision: false, maxContextLength: 32_768 },
};
const providers: { id: ProviderId; name: string; detail: string }[] = [
  { id: "deepseek", name: "DeepSeek", detail: "Official DeepSeek API" },
  { id: "openrouter", name: "OpenRouter", detail: "Multi-provider model catalog" },
  { id: "openai", name: "OpenAI", detail: "Official OpenAI API" },
  { id: "custom", name: "Custom compatible API", detail: "Any OpenAI Chat Completions endpoint" },
  { id: "hku", name: "HKU GenAI", detail: "DeepSeek, Claude, Azure OpenAI, Gemini, Qwen/GLM & image models" },
];
const sections: { id: SettingsSection; label: string; detail: string; icon: typeof Settings2 }[] = [
  { id: "general", label: "General", detail: "Model behavior", icon: SlidersHorizontal },
  { id: "providers", label: "Providers", detail: "Keys and endpoints", icon: Cloud },
  { id: "performance", label: "Performance", detail: "Context and resources", icon: Cpu },
  { id: "security", label: "Security", detail: "Execution permissions", icon: ShieldCheck },
  { id: "workspaces", label: "Workspaces", detail: "Projects and folders", icon: FolderOpen },
  { id: "email", label: "Email listener", detail: "Outlook verification", icon: Mail },
];

type Props = {
  onClose: () => void;
  selectedProvider: string;
  selectedProviderModel?: string;
  onProviderChange: (id: string, model: string, vision: boolean, maxContext: number) => void;
  profile: string; onProfile: (value: string) => void;
  thinkingMode: "Off" | "On"; onThinkingMode: (value: "Off" | "On") => void;
  reasoningEffort: string; onReasoningEffort: (value: string) => void;
  contextLength: number; contextMaximum: number; onContextLength: (value: number) => void;
  contextMode: "auto" | "custom"; onAutomaticContext: () => void;
  resourcePolicy: string; onResourcePolicy: (value: "Adaptive" | "Performance" | "Cool") => void;
  executionMode: string; onExecutionMode: (value: "Safe" | "ApproveForMe" | "DeveloperUnrestricted") => void;
  runtimeIdentity?: { display_name?: string; key?: string; provider?: string; context_length?: number; max_context_length?: number };
  activeModel?: Model;
  systemPrompt: string; onSystemPrompt: (value: string) => void; onSaveSetting: (key: string, value: unknown) => void;
  usagePricing: PricingMap; onUsagePricingChange: (value: PricingMap) => void;
  workspaces: Workspace[]; activeWorkspaceId: string | null;
  onSelectWorkspace: (id: string) => void; onRefreshWorkspace: () => void; onAddWorkspace: () => void;
  environment?: { security?: { os_isolation_available?: boolean } };
  outlookStatus?: { connected: boolean; account?: { email?: string } | null; permission: string; listener_enabled: boolean } | null;
  outlookClientId: string; onOutlookClientId: (value: string) => void;
  outlookTenant: string; onOutlookTenant: (value: string) => void;
  outlookEmailHint: string; onOutlookEmailHint: (value: string) => void;
  outlookBrowser: string; onOutlookBrowser: (value: "Google Chrome" | "Safari") => void;
  outlookConnecting: boolean; onConnectOutlook: () => void; onToggleOutlook: (enabled: boolean) => void; onDisconnectOutlook: () => void;
  onError: (message: string) => void;
};

function fieldLabel(title: string, hint?: string) {
  return <span className="settings-field-title">{title}{hint && <small>{hint}</small>}</span>;
}

export default function SettingsOverlay(props: Props) {
  const [section, setSection] = useState<SettingsSection>("general");
  const [configs, setConfigs] = useState<Record<ProviderId, ProviderConfig>>(defaults);
  const [editingProvider, setEditingProvider] = useState<ProviderId>(
    (providers.some((provider) => provider.id === props.selectedProvider) ? props.selectedProvider : "deepseek") as ProviderId,
  );
  const [keyDraft, setKeyDraft] = useState("");
  const [keySaved, setKeySaved] = useState<Record<string, boolean>>({});
  const [models, setModels] = useState<Model[]>([]);
  const [providerBusy, setProviderBusy] = useState(false);
  const [providerMessage, setProviderMessage] = useState("");
  const [externalAcknowledged, setExternalAcknowledged] = useState(false);
  const [embeddingInput, setEmbeddingInput] = useState("");
  const [embeddingResult, setEmbeddingResult] = useState<{ model: string; embedding: number[]; usage: { input_tokens?: number } } | null>(null);
  const [embeddingBusy, setEmbeddingBusy] = useState(false);
  const [settingsSearch, setSettingsSearch] = useState("");
  const [contextDraft, setContextDraft] = useState(props.contextLength);
  const [pricingDraft, setPricingDraft] = useState<{ inputPerMillion: string; cachedInputPerMillion: string; outputPerMillion: string } | null>(null);

  useEffect(() => setContextDraft(props.contextLength), [props.contextLength]);
  const activePricingKey = pricingKey(editingProvider, configs[editingProvider].model);
  const activePricing = modelPricing(editingProvider, configs[editingProvider].model, props.usagePricing);
  useEffect(() => setPricingDraft(activePricing ? {
    inputPerMillion: String(activePricing.inputPerMillion),
    cachedInputPerMillion: String(activePricing.cachedInputPerMillion),
    outputPerMillion: String(activePricing.outputPerMillion),
  } : null), [activePricingKey, props.usagePricing]);

  useEffect(() => {
    const load = async () => {
      try {
        const stored = await invoke<Record<string, unknown>>("get_settings");
        const next = { ...defaults };
        for (const provider of providers) {
          const saved = stored[`external_provider_${provider.id}`] as { base_url?: string; model?: string; supports_vision?: boolean; max_context_length?: number } | undefined;
          if (saved) next[provider.id] = {
            baseUrl: saved.base_url ?? next[provider.id].baseUrl,
            model: saved.model ?? next[provider.id].model,
            supportsVision: saved.supports_vision ?? next[provider.id].supportsVision,
            maxContextLength: saved.max_context_length ?? next[provider.id].maxContextLength,
          };
        }
        setConfigs(next);
        setExternalAcknowledged(stored.external_provider_acknowledged === true);
        const statuses = await Promise.all(providers.map(async ({ id }) => {
          try { return [id, await invoke<boolean>("provider_api_key_status", { providerId: id })] as const; }
          catch { return [id, false] as const; }
        }));
        setKeySaved(Object.fromEntries(statuses));
      } catch (error) {
        props.onError(String(error));
      }
    };
    void load();
  // Provider settings are a snapshot when the overlay opens.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const activeConfig = configs[editingProvider];
  const hkuThinkingSupported = Boolean(props.selectedProviderModel && (
    props.selectedProviderModel.startsWith("DeepSeek-")
    || props.selectedProviderModel.includes("thinking")
    || props.selectedProviderModel.startsWith("zai-org/")
    || props.selectedProviderModel === "o4-mini"
    || props.selectedProviderModel.startsWith("gpt-5")
    || (props.selectedProviderModel.startsWith("gemini-") && !props.selectedProviderModel.includes("image"))
  ));
  const activeProviderLabel = useMemo(
    () => editingProvider === "custom" ? "Custom OpenAI-compatible" : providers.find((item) => item.id === editingProvider)?.name ?? "Provider",
    [editingProvider],
  );

  function updateConfig<K extends keyof ProviderConfig>(key: K, value: ProviderConfig[K]) {
    setConfigs((current) => ({ ...current, [editingProvider]: { ...current[editingProvider], [key]: value } }));
  }

  function savePricing(value: ModelPricing) {
    const next = { ...props.usagePricing, [activePricingKey]: value };
    props.onUsagePricingChange(next);
    props.onSaveSetting("usage_model_pricing", next);
  }

  function updatePricing(key: keyof ModelPricing, value: string) {
    setPricingDraft((current) => ({ ...(current ?? { inputPerMillion: "", cachedInputPerMillion: "", outputPerMillion: "" }), [key]: value }));
  }

  const pricingDraftValid = Boolean(pricingDraft && Object.values(pricingDraft).every((value) => value.trim() !== "" && Number.isFinite(Number(value)) && Number(value) >= 0));

  async function saveProviderConfig() {
    await invoke("save_external_provider_config", {
      providerId: editingProvider,
      baseUrl: activeConfig.baseUrl,
      model: activeConfig.model,
      supportsVision: activeConfig.supportsVision,
      maxContextLength: activeConfig.maxContextLength,
    });
  }

  async function saveKeyIfEntered() {
    if (!keyDraft.trim()) return;
    await invoke("set_provider_api_key", { providerId: editingProvider, value: keyDraft.trim() });
    setKeyDraft("");
    setKeySaved((current) => ({ ...current, [editingProvider]: true }));
  }

  async function testProvider() {
    setProviderBusy(true);
    setProviderMessage("");
    try {
      await saveProviderConfig();
      await saveKeyIfEntered();
      const hasKey = keyDraft.trim() || keySaved[editingProvider];
      if (!hasKey && editingProvider !== "hku") throw new Error("Add an API key first. It is stored in macOS Keychain.");
      const result = await invoke<Model[]>("list_external_provider_models", { providerId: editingProvider });
      setModels(result);
      setProviderMessage(editingProvider === "hku"
        ? `Loaded ${result.length} documented HKU models. No prompt was sent; HKU credentials are checked on your first chat.`
        : `Connected · ${result.length} model${result.length === 1 ? "" : "s"} available`);
      if (!activeConfig.model && result[0]) updateConfig("model", result[0].key);
    } catch (error) {
      setProviderMessage(String(error));
    } finally {
      setProviderBusy(false);
    }
  }

  async function createHkuEmbedding() {
    if (!externalAcknowledged) {
      setProviderMessage("Confirm the external data-sharing notice before sending embedding input to HKU.");
      return;
    }
    setEmbeddingBusy(true);
    setProviderMessage("");
    setEmbeddingResult(null);
    try {
      await saveProviderConfig();
      await saveKeyIfEntered();
      const result = await invoke<{ model: string; embedding: number[]; usage: { input_tokens?: number } }>("hku_embed_text", {
        input: embeddingInput,
        acknowledgeExternal: externalAcknowledged,
      });
      setEmbeddingResult(result);
      setProviderMessage(`Embedding created · ${result.embedding.length.toLocaleString()} dimensions · ${result.usage.input_tokens?.toLocaleString() ?? "token usage not reported"} input tokens.`);
    } catch (error) {
      setProviderMessage(String(error));
    } finally {
      setEmbeddingBusy(false);
    }
  }

  async function activateProvider() {
    setProviderBusy(true);
    setProviderMessage("");
    try {
      await saveProviderConfig();
      await saveKeyIfEntered();
      if (!externalAcknowledged) throw new Error("Confirm the data-sharing notice before activating an external provider.");
      await invoke("activate_inference_provider", { providerId: editingProvider, acknowledgeExternal: true });
      props.onProviderChange(editingProvider, activeConfig.model.trim(), activeConfig.supportsVision, activeConfig.maxContextLength);
      setProviderMessage(`${activeProviderLabel} is now active.`);
    } catch (error) {
      setProviderMessage(String(error));
    } finally {
      setProviderBusy(false);
    }
  }

  async function activateLocal() {
    try {
      await invoke("activate_inference_provider", { providerId: "local", acknowledgeExternal: false });
      props.onProviderChange("local", "", false, props.contextMaximum);
      setProviderMessage("LM Studio is now active. Local conversations stay on this Mac.");
    } catch (error) { props.onError(String(error)); }
  }

  async function clearKey() {
    try {
      await invoke("clear_provider_api_key", { providerId: editingProvider });
      setKeySaved((current) => ({ ...current, [editingProvider]: false }));
      setProviderMessage("API key removed from Keychain.");
    } catch (error) { setProviderMessage(String(error)); }
  }

  const visibleSections = sections.filter((item) => `${item.label} ${item.detail}`.toLowerCase().includes(settingsSearch.toLowerCase()));
  const selectClass = "settings-control";

  return <div className="modal-backdrop settings-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget) props.onClose(); }}>
    <section className="settings-window material" role="dialog" aria-modal="true" aria-label="JadX Settings">
      <aside className="settings-rail">
        <div className="settings-window-title"><div className="settings-app-glyph">JX</div><div><span>JadX</span><small>Settings</small></div><button className="icon-button quiet settings-close" onClick={props.onClose} aria-label="Close settings"><X size={16} /></button></div>
        <label className="settings-search"><Search size={14} /><input aria-label="Search settings" value={settingsSearch} onChange={(event) => setSettingsSearch(event.target.value)} placeholder="Search" /><kbd>⌘F</kbd></label>
        <nav className="settings-nav" aria-label="Settings sections">{visibleSections.map((item) => { const Icon = item.icon; return <button key={item.id} className={section === item.id ? "active" : ""} onClick={() => setSection(item.id)}><Icon size={16} /><span>{item.label}<small>{item.detail}</small></span>{section === item.id && <ChevronRight size={13} />}</button>; })}</nav>
        <div className="settings-rail-footer"><span className="settings-local-dot" /> Local-first by default</div>
      </aside>
      <div className="settings-main">
        <header className="settings-page-head"><div><p className="eyebrow">JADX PREFERENCES</p><h2>{sections.find((item) => item.id === section)?.label ?? "Settings"}</h2><p>{sections.find((item) => item.id === section)?.detail ?? "Configure JadX"}</p></div><button className="icon-button quiet settings-main-close" onClick={props.onClose} aria-label="Close settings"><X size={16} /></button></header>
        <div className="settings-scroll">
          {section === "general" && <>
            <section className="settings-group"><div className="settings-group-head"><div><h3>Assistant behavior</h3><p>Choose how JadX responds across conversations.</p></div><Settings2 size={16} /></div>
              <div className="settings-row"><div>{fieldLabel("Response profile", "Controls default speed and reasoning preferences.")}</div><select className={selectClass} value={props.profile} onChange={(event) => props.onProfile(event.target.value)}><option>FAST</option><option>BALANCED</option><option>DEEP</option><option>AGENT</option><option>CUSTOM</option></select></div>
              <div className="settings-row"><div>{fieldLabel("Thinking", props.selectedProvider === "hku" && !hkuThinkingSupported ? "The selected HKU deployment does not document adjustable thinking controls." : "Thinking settings are mapped to the selected model's supported fields.")}</div><select className={selectClass} value={props.thinkingMode} disabled={props.selectedProvider === "hku" && !hkuThinkingSupported} onChange={(event) => props.onThinkingMode(event.target.value as "Off" | "On")}><option>Off</option><option>On</option></select></div>
              <div className="settings-row"><div>{fieldLabel("Reasoning effort", props.selectedProvider === "hku" && props.selectedProviderModel?.startsWith("DeepSeek-") ? "HKU DeepSeek maps Low–High to high and XHigh to max; unsupported fields are omitted." : "Supported providers receive the closest supported effort; unavailable values are omitted.")}</div><select className={selectClass} value={props.reasoningEffort} disabled={props.thinkingMode === "Off" || props.selectedProvider === "deepseek" || (props.selectedProvider === "hku" && !hkuThinkingSupported)} onChange={(event) => props.onReasoningEffort(event.target.value)}><option>Low</option><option>Medium</option><option>High</option><option>XHigh</option></select></div>
            </section>
            <section className="settings-group"><div className="settings-group-head"><div><h3>Runtime identity</h3><p>This exact identity is supplied to the model with each request.</p></div><Activity size={16} /></div><div className="settings-identity"><strong>{props.runtimeIdentity?.display_name ?? props.activeModel?.display_name ?? "LM Studio model not discovered"}</strong><code>{props.runtimeIdentity?.key ?? props.activeModel?.key ?? "—"}</code><span>{props.runtimeIdentity?.provider ?? "LM Studio (local)"}</span></div></section>
            <section className="settings-group"><div className="settings-group-head"><div><h3>System instructions</h3><p>Saved locally and included in future prompts.</p></div></div><textarea className="settings-prompt" value={props.systemPrompt} onChange={(event) => props.onSystemPrompt(event.target.value)} onBlur={() => props.onSaveSetting("system_prompt", props.systemPrompt)} rows={7} /></section>
          </>}

          {section === "providers" && <>
            <section className="settings-group provider-active-card"><div className="provider-card-top"><span className="settings-provider-icon"><Cloud size={18} /></span><div><h3>Active inference provider</h3><p>{props.selectedProvider === "local" ? "LM Studio · requests stay on this Mac" : `${providers.find((item) => item.id === props.selectedProvider)?.name ?? "External provider"} · cloud requests enabled`}</p></div><span className={`provider-state ${props.selectedProvider === "local" ? "local" : "cloud"}`}><i />{props.selectedProvider === "local" ? "LOCAL" : "ACTIVE"}</span></div>
              <div className="provider-switch-row"><span>LM Studio on this Mac</span><button className={`secondary-button compact-button ${props.selectedProvider === "local" ? "selected" : ""}`} onClick={() => void activateLocal()}>{props.selectedProvider === "local" ? <><Check size={13} /> Active</> : "Use local"}</button></div>
            </section>
            <section className="settings-group"><div className="settings-group-head"><div><h3>Configure an external API</h3><p>OpenAI-compatible streaming, provider-specific request handling, and Keychain-backed credentials.</p></div><Cloud size={16} /></div>
              <div className="settings-provider-cards">{providers.map((item) => <button key={item.id} className={`settings-provider-card ${editingProvider === item.id ? "chosen" : ""}`} onClick={() => { setEditingProvider(item.id); setModels([]); setProviderMessage(""); setKeyDraft(""); }}><span className="provider-monogram">{item.name.slice(0, 1)}</span><span><strong>{item.name}</strong><small>{item.detail}</small></span>{props.selectedProvider === item.id && <Check size={15} />}</button>)}</div>
              <div className="provider-form">
                <label>{fieldLabel("API base URL", editingProvider === "hku" ? "Fixed to HKU's official gateway; keys are sent in its api-key header." : "HTTPS required; a local loopback HTTP endpoint is also supported.")}<input className={selectClass} value={activeConfig.baseUrl} readOnly={editingProvider === "hku"} onChange={(event) => updateConfig("baseUrl", event.target.value)} placeholder="https://api.example.com/v1" autoCapitalize="off" autoCorrect="off" /></label>
                <label>{fieldLabel("Model ID", editingProvider === "hku" ? "Select from the HKU catalog: chat, Responses, Gemini multimodal, or image generation deployments are routed by model." : "Use the provider's exact model identifier.")}<input className={selectClass} value={activeConfig.model} onChange={(event) => { const modelId = event.target.value; updateConfig("model", modelId); const discovered = models.find((item) => item.key === modelId); if (discovered?.max_context_length) updateConfig("maxContextLength", discovered.max_context_length); if (discovered) updateConfig("supportsVision", Boolean(discovered.capabilities?.vision)); }} list={`provider-models-${editingProvider}`} placeholder={editingProvider === "openrouter" ? "vendor/model-name" : editingProvider === "hku" ? "gpt-5-mini or gemini-2.5-flash-image" : "model-id"} autoCapitalize="off" autoCorrect="off" /><datalist id={`provider-models-${editingProvider}`}>{models.map((model) => <option key={model.key} value={model.key}>{model.display_name}</option>)}</datalist></label>
                <label>{fieldLabel(editingProvider === "hku" ? "HKU subscription key" : "API key", keySaved[editingProvider] ? "A key is saved in macOS Keychain. Enter a new value to replace it." : editingProvider === "hku" ? "Stored in macOS Keychain; sent only to api.hku.hk in the api-key header." : "Stored in macOS Keychain; never written to JadX settings or logs.")}<input className={selectClass} type="password" autoComplete="new-password" value={keyDraft} onChange={(event) => setKeyDraft(event.target.value)} placeholder={keySaved[editingProvider] ? "Saved key · leave blank to keep" : "Paste API key"} /></label>
                <label>{fieldLabel("Model context limit", editingProvider === "hku" ? "HKU does not publish the limits in this catalog; 32K is a conservative editable estimate." : "Used by the context meter; confirm the actual limit for this model.")}<input className={selectClass} type="number" min={4096} max={1_000_000} step={1024} value={activeConfig.maxContextLength} onChange={(event) => updateConfig("maxContextLength", Number(event.target.value))} /></label>
                <label className="settings-check-row"><input type="checkbox" checked={activeConfig.supportsVision} disabled={editingProvider === "hku"} onChange={(event) => updateConfig("supportsVision", event.target.checked)} /><span><strong>This model accepts image input</strong><small>{editingProvider === "hku" ? "HKU Gemini and selected Azure OpenAI chat deployments accept images. Image-generation models are separately identified in the catalog." : "Enable only when the selected model/API supports image inputs."}</small></span></label>
              </div>
              <div className="provider-pricing-card">
                <div className="provider-pricing-head"><div><strong>Estimated API rates</strong><small>USD per 1 million tokens · applies to {activeConfig.model || "the selected model"}</small></div><span>{editingProvider === "deepseek" ? "DeepSeek" : editingProvider === "openrouter" ? "OpenRouter" : activeProviderLabel}</span></div>
                {editingProvider === "deepseek" && defaultModelPricing(editingProvider, activeConfig.model) && <div className="provider-pricing-presets"><button className="secondary-button compact-button" onClick={() => { const rate = defaultModelPricing(editingProvider, activeConfig.model, "offPeak"); if (rate) savePricing(rate); }}>Use off-peak rate</button><button className="secondary-button compact-button" onClick={() => { const rate = defaultModelPricing(editingProvider, activeConfig.model, "peak"); if (rate) savePricing(rate); }}>Use peak rate</button></div>}
                <div className="provider-pricing-fields">
                  <label>{fieldLabel("Uncached input", "$/1M tokens")}<input className={selectClass} type="number" min="0" step="any" value={pricingDraft?.inputPerMillion ?? ""} onChange={(event) => updatePricing("inputPerMillion", event.target.value)} placeholder="Enter rate" /></label>
                  <label>{fieldLabel("Cached input", "$/1M tokens")}<input className={selectClass} type="number" min="0" step="any" value={pricingDraft?.cachedInputPerMillion ?? ""} onChange={(event) => updatePricing("cachedInputPerMillion", event.target.value)} placeholder="Enter rate" /></label>
                  <label>{fieldLabel("Output", "$/1M tokens")}<input className={selectClass} type="number" min="0" step="any" value={pricingDraft?.outputPerMillion ?? ""} onChange={(event) => updatePricing("outputPerMillion", event.target.value)} placeholder="Enter rate" /></label>
                </div>
                <div className="provider-pricing-foot"><span>{editingProvider === "deepseek" ? <>DeepSeek rates checked {DEEPSEEK_PRICING_CHECKED}. The selected tier is applied to all tracked requests; peak windows are 01:00–04:00 and 06:00–10:00 UTC, weekdays except Chinese public holidays. <a href={DEEPSEEK_PRICING_SOURCE} target="_blank" rel="noreferrer">Official price list</a></> : editingProvider === "openrouter" ? <>OpenRouter prices vary by model; copy the current model rates from its <a href="https://openrouter.ai/models" target="_blank" rel="noreferrer">model catalog</a>.</> : "Set rates from the provider’s current price list; JadX does not guess unknown prices."}</span><button className="primary-button compact-button" disabled={!activeConfig.model.trim() || !pricingDraftValid} onClick={() => pricingDraft && pricingDraftValid && savePricing({ inputPerMillion: Number(pricingDraft.inputPerMillion), cachedInputPerMillion: Number(pricingDraft.cachedInputPerMillion), outputPerMillion: Number(pricingDraft.outputPerMillion) })}>Save rates</button></div>
              </div>
              <div className="provider-data-notice"><AlertTriangle size={17} /><div><strong>External data sharing</strong><p>When active, JadX sends prompts, conversation context, enabled tool schemas/results, and attached images to {activeProviderLabel}. Workspace files are sent only when the model reads them with a tool. Browser screenshots and Outlook verification results may also become tool results in the chat. The provider's privacy, retention, and billing terms apply. Local is always available as a separate choice.</p><label><input type="checkbox" checked={externalAcknowledged} onChange={(event) => setExternalAcknowledged(event.target.checked)} /><span>I understand and authorize sending this chat's required data to the selected external provider.</span></label></div></div>
              <div className="provider-actions"><button className="secondary-button" disabled={providerBusy} onClick={() => void testProvider()}>{providerBusy ? <><RefreshCw size={14} className="spin-icon" /> Connecting…</> : <><RefreshCw size={14} /> {editingProvider === "hku" ? "Load documented models" : "Test & list models"}</>}</button><button className="primary-button" disabled={providerBusy || !externalAcknowledged || !activeConfig.model.trim()} onClick={() => void activateProvider()}><Check size={14} /> Activate {activeProviderLabel}</button></div>
              {editingProvider === "hku" && activeConfig.model.startsWith("text-embedding-") && <div className="embedding-test"><div><strong>Embedding deployment</strong><small>Send a short sample to HKU and return its vector. Input text is not stored by JadX.</small></div><textarea value={embeddingInput} onChange={(event) => setEmbeddingInput(event.target.value)} maxLength={32000} rows={3} placeholder="Text to embed…" /><div className="embedding-actions"><button className="secondary-button compact-button" disabled={!embeddingResult} onClick={() => void navigator.clipboard.writeText(JSON.stringify(embeddingResult?.embedding ?? [])).catch(() => setProviderMessage("Clipboard access is unavailable."))}><Copy size={13} /> Copy vector</button><button className="primary-button compact-button" disabled={embeddingBusy || !embeddingInput.trim() || !externalAcknowledged} onClick={() => void createHkuEmbedding()}>{embeddingBusy ? <><RefreshCw size={13} className="spin-icon" /> Creating…</> : "Create embedding"}</button></div></div>}
              {providerMessage && <div className={`provider-message ${providerMessage.toLowerCase().includes("connected") || providerMessage.toLowerCase().includes("active") ? "success" : ""}`} role="status">{providerMessage}</div>}
              <div className="provider-key-actions">{keySaved[editingProvider] && <button className="text-button danger-text" onClick={() => void clearKey()}><KeyRound size={13} /> Remove saved API key</button>}<span>Keys are isolated per provider in the macOS Keychain.</span></div>
            </section>
          </>}

          {section === "performance" && <>
            <section className="settings-group"><div className="settings-group-head"><div><h3>Context window</h3><p>{props.selectedProvider === "local" ? "Local Auto uses the largest context recommended for this Mac’s available memory and model." : "External Auto uses the selected model’s configured maximum. Actual cloud billing depends on the prompt and project files read."}</p></div><Cpu size={16} /></div><div className="settings-row"><div>{fieldLabel("Context sizing", props.selectedProvider === "local" ? "Auto is memory-aware; a size selection is saved for LM Studio only." : "Auto follows this external model’s configured context limit; each provider keeps its own setting.")}</div><button className={`secondary-button compact-button ${props.contextMode === "auto" ? "selected" : ""}`} onClick={props.onAutomaticContext}>{props.contextMode === "auto" ? "Auto · active" : "Use Auto"}</button></div><div className="settings-row"><div>{fieldLabel("Context size", "Numeric choices are capped to the selected model limit.")}</div><select className={selectClass} value={props.contextLength} onChange={(event) => props.onContextLength(Number(event.target.value))}>{[8192,12288,16384,24576,32768,49152,65536].filter((size) => size <= Math.max(props.contextMaximum, 16384)).map((size) => <option key={size} value={size}>{Math.round(size / 1024)}K{size === 16384 ? " · recommended" : ""}</option>)}</select></div><div className="settings-row"><div>{fieldLabel("Custom token count", `Maximum ${props.contextMaximum.toLocaleString()} tokens`)}</div><input className={selectClass} type="number" min={4096} max={props.contextMaximum} step={1024} value={contextDraft} onChange={(event) => setContextDraft(Number(event.target.value))} onBlur={() => props.onContextLength(contextDraft)} /></div></section>
            <section className="settings-group"><div className="settings-group-head"><div><h3>Resource policy</h3><p>JadX controls request budgets and background work, not the provider's internal allocator.</p></div><Activity size={16} /></div><div className="settings-row"><div>{fieldLabel("Runtime policy")}</div><select className={selectClass} value={props.resourcePolicy} onChange={(event) => props.onResourcePolicy(event.target.value as "Adaptive" | "Performance" | "Cool")}><option value="Adaptive">Adaptive · balanced responsiveness</option><option value="Performance">Performance · prioritize throughput</option><option value="Cool">Cool · reduce background load</option></select></div><div className="settings-note"><Check size={14} /> One generation at a time · pressure-aware budgets · background compaction is deferred under load.</div></section>
          </>}

          {section === "security" && <>
            <section className={`settings-group security-mode-card ${props.executionMode === "DeveloperUnrestricted" ? "danger" : ""}`}>
              <div className="settings-group-head"><div><h3>Agent permissions</h3><p>Safe mode is the recommended default.</p></div>{props.executionMode === "DeveloperUnrestricted" ? <ShieldAlert size={17} /> : <ShieldCheck size={17} />}</div>
              <select className={selectClass} value={props.executionMode} onChange={(event) => props.onExecutionMode(event.target.value as "Safe" | "ApproveForMe" | "DeveloperUnrestricted")}>
                <option value="Safe">Safe mode · approve each sensitive action</option>
                <option value="ApproveForMe">Approve for me · auto-approve workspace edits</option>
                <option value="DeveloperUnrestricted">Full access · auto-approve edits and shell/Python</option>
              </select>
              <div className={`security-callout ${props.executionMode === "DeveloperUnrestricted" ? "danger" : ""}`}>
                {props.executionMode === "DeveloperUnrestricted" ? <ShieldAlert size={15} /> : <LockKeyhole size={15} />}
                <span>{props.executionMode === "DeveloperUnrestricted"
                  ? "Workspace edits and model-generated shell/Python run without per-action approval. Shell and Python run as your macOS user without Safe-mode command blocking and can access files outside the workspace or use the network. Dedicated browser, web, and sensitive actions still ask."
                  : props.executionMode === "ApproveForMe"
                    ? `Workspace file edits are applied automatically with recovery snapshots. Allowlisted IDE verification checks run in the workspace sandbox; other shell, Python, external browser, network, and sensitive actions still ask. OS isolation is ${props.environment?.security?.os_isolation_available ? "available" : "unavailable"}.`
                    : `Safe mode asks before every workspace write, shell/Python request, and external action. OS isolation is ${props.environment?.security?.os_isolation_available ? "available" : "unavailable"}.`}</span>
              </div>
            </section>
            <section className="settings-group"><div className="settings-group-head"><div><h3>Privacy boundary</h3><p>Provider choice never changes by itself. External providers require an explicit activation in the Providers section.</p></div><LockKeyhole size={16} /></div><div className="settings-note"><ShieldCheck size={14} /> API secrets stay in Keychain; logs and diagnostics redact credentials.</div></section>
          </>}

          {section === "workspaces" && <section className="settings-group"><div className="settings-group-head"><div><h3>Approved project folders</h3><p>Filesystem tools are limited to directories you explicitly add.</p></div><FolderOpen size={16} /></div><div className="settings-workspace-list">{props.workspaces.map((workspace) => <button key={workspace.id} className={`settings-workspace-row ${props.activeWorkspaceId === workspace.id ? "active" : ""}`} onClick={() => props.onSelectWorkspace(workspace.id)}><FolderOpen size={15} /><span><strong>{workspace.name}</strong><small>{workspace.root_path}</small></span>{props.activeWorkspaceId === workspace.id && <Check size={14} />}</button>)}{props.workspaces.length === 0 && <p className="settings-empty">No approved folders yet. Choose a project folder to enable workspace tools.</p>}</div><div className="settings-inline-actions"><button className="secondary-button" onClick={props.onRefreshWorkspace} disabled={!props.activeWorkspaceId}><RefreshCw size={13} /> Refresh metadata</button><button className="secondary-button" onClick={props.onAddWorkspace}><Plus size={14} /> Add folder</button></div><div className="settings-note">The metadata cache stores paths, sizes, timestamps, and Git status—not file contents.</div></section>}

          {section === "email" && <section className="settings-group"><div className="settings-group-head"><div><h3>Outlook verification listener</h3><p>Read-only, narrow mailbox access for verification-code tasks.</p></div><Mail size={16} /></div><div className="settings-outlook-status"><Mail size={16} /><span>{props.outlookStatus?.connected ? `Connected · ${props.outlookStatus.account?.email ?? "Microsoft account"}` : "Not connected"}</span>{props.outlookStatus?.connected && <span className="provider-state local"><i /> {props.outlookStatus.permission}</span>}</div><div className="settings-form-grid"><label>{fieldLabel("Microsoft public client ID")}<input className={selectClass} value={props.outlookClientId} onChange={(event) => props.onOutlookClientId(event.target.value)} onBlur={() => props.onSaveSetting("outlook_client_id", props.outlookClientId.trim())} placeholder="Entra app registration ID" /></label><label>{fieldLabel("Tenant")}<input className={selectClass} value={props.outlookTenant} onChange={(event) => props.onOutlookTenant(event.target.value)} onBlur={() => props.onSaveSetting("outlook_tenant", props.outlookTenant.trim() || "common")} placeholder="common" /></label><label>{fieldLabel("Account hint (optional)")}<input className={selectClass} value={props.outlookEmailHint} onChange={(event) => props.onOutlookEmailHint(event.target.value)} onBlur={() => props.onSaveSetting("outlook_email_hint", props.outlookEmailHint.trim())} placeholder="you@outlook.com" /></label><label>{fieldLabel("Open sign-in in")}<select className={selectClass} value={props.outlookBrowser} onChange={(event) => props.onOutlookBrowser(event.target.value as "Google Chrome" | "Safari")}><option>Google Chrome</option><option>Safari</option></select></label></div><div className="settings-inline-actions"><button className="secondary-button" onClick={props.onConnectOutlook} disabled={props.outlookConnecting}>{props.outlookConnecting ? <><RefreshCw size={13} className="spin-icon" /> Waiting for sign-in…</> : <><ExternalLink size={13} /> Connect Outlook</>}</button>{props.outlookStatus?.connected && <><button className="secondary-button" onClick={() => props.onToggleOutlook(!props.outlookStatus?.listener_enabled)}>{props.outlookStatus.listener_enabled ? "Pause listener" : "Enable listener"}</button><button className="text-button danger-text" onClick={props.onDisconnectOutlook}>Disconnect</button></>}</div><div className="settings-note">Mail.Read only. JadX polls the selected Inbox only while a task is waiting for a matching code; it does not send, delete, or mark messages read.</div></section>}
        </div>
        <footer className="settings-window-footer"><span><ShieldCheck size={14} /> {props.selectedProvider === "local" ? "Local provider" : `${providers.find((item) => item.id === props.selectedProvider)?.name ?? "External"} selected`} · {props.resourcePolicy} resources · {props.executionMode === "DeveloperUnrestricted" ? "full access" : props.executionMode === "ApproveForMe" ? "approve for me" : "safe mode"}</span><button className="primary-button" onClick={props.onClose}>Done</button></footer>
      </div>
    </section>
  </div>;
}
