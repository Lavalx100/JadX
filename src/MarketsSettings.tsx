import { ShieldCheck, X } from "lucide-react";

type MarketsSettingsProps = {
  enabled: boolean;
  backgroundServices: string;
  host: string;
  port: string;
  tradingMode: string;
  fixtureMode: boolean;
  onEnabledChange: (value: boolean) => void;
  onBackgroundServicesChange: (value: string) => void;
  onHostChange: (value: string) => void;
  onHostBlur: () => void;
  onPortChange: (value: string) => void;
  onPortBlur: () => void;
  onTradingModeChange: (value: string) => void;
  onFixtureModeChange: (value: boolean) => void;
  onClose: () => void;
};

export default function MarketsSettings({
  enabled,
  backgroundServices,
  host,
  port,
  tradingMode,
  fixtureMode,
  onEnabledChange,
  onBackgroundServicesChange,
  onHostChange,
  onHostBlur,
  onPortChange,
  onPortBlur,
  onTradingModeChange,
  onFixtureModeChange,
  onClose,
}: MarketsSettingsProps) {
  return <div className="modal-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}><section className="markets-settings-modal material" role="dialog" aria-modal="true" aria-label="JadX Markets settings"><div className="inspector-head"><div><p className="eyebrow">JADX MARKETS</p><h3>Markets settings</h3></div><button className="icon-button quiet" onClick={onClose} aria-label="Close Markets settings"><X size={16} /></button></div><div className="markets-settings-panel standalone"><div className="markets-settings-row"><div><strong>Markets Mode</strong><small>Enable the optional workspace. Normal Assistant mode does not receive market tools or market context.</small></div><select value={String(enabled)} onChange={(event) => onEnabledChange(event.target.value === "true")}><option value="false">Disabled</option><option value="true">Enabled</option></select></div><div className="markets-settings-row"><div><strong>Background market services</strong><small>Independent service policy. Use OFF until the provider is configured.</small></div><select value={backgroundServices} onChange={(event) => onBackgroundServicesChange(event.target.value)}><option>OFF</option><option>MARKET_HOURS</option><option>ALWAYS</option></select></div><div className="markets-settings-fields"><label><span>Futu OpenD host</span><input value={host} onChange={(event) => onHostChange(event.target.value)} onBlur={onHostBlur} /></label><label><span>Futu OpenD port</span><input type="number" value={port} onChange={(event) => onPortChange(event.target.value)} onBlur={onPortBlur} /></label><label><span>Trading mode</span><select value={tradingMode} onChange={(event) => onTradingModeChange(event.target.value)}><option>OFF</option><option>PAPER</option><option>LIVE_WITH_APPROVAL</option></select></label><label><span>Development fixtures</span><select value={String(fixtureMode)} onChange={(event) => onFixtureModeChange(event.target.value === "true")}><option value="false">Disabled</option><option value="true">SIMULATED only</option></select></label></div><div className="markets-settings-note"><ShieldCheck size={14} /><span>{fixtureMode ? "SIMULATED fixture data is enabled and labelled throughout the workspace; it is never presented as live." : "No fixture data is enabled; missing provider data remains UNAVAILABLE."} Live trading is disabled by the current runtime.</span></div></div><div className="markets-settings-checklist"><strong>Provider boundaries</strong><span>Browser/UI never talks directly to OpenD.</span><span>OpenD owns Futu authentication; JadX does not store a brokerage password.</span><span>Quote data carries provider, observed time, retrieval time, and freshness.</span><span>Paper validation is local. No real-money submission endpoint is exposed.</span></div><div className="settings-footer"><div><ShieldCheck size={15} /><span>Local-first · source-aware · trading off by default</span></div><button className="primary-button" onClick={onClose}>Done</button></div></section></div>;
}
