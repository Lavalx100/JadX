import { lazy, Suspense, useEffect, useRef, useState, type ReactNode, type RefObject } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { AudioLines, CircleStop, Mic, Plus, RotateCw, Send, Settings2, ShieldCheck, Volume2, X } from "lucide-react";
import { encodeMonoPcm16Wav, maxWindowedRms } from "./avatarAudio";

const AvatarGlbView = lazy(() => import("./AvatarGlbView"));

type MicState = "idle" | "recording" | "transcribing";

export type PersonaAvatarProfile = { id: string; name: string; lipSyncReady: boolean };

type CaptureSession = {
  stream: MediaStream;
  context: AudioContext;
  source: MediaStreamAudioSourceNode;
  processor: ScriptProcessorNode;
  mute: GainNode;
  chunks: Float32Array[];
  sampleCount: number;
  sampleRate: number;
  startedAt: number;
};

type AvatarWorkspaceProps = {
  image: string;
  modelDataUrl: string;
  modelHint: string;
  faces: PersonaAvatarProfile[];
  activeFaceId: string;
  faceBusy: boolean;
  faceNotice: string;
  stageRef: RefObject<HTMLDivElement | null>;
  speaking: boolean;
  generating: boolean;
  voiceReady: boolean;
  wordTimingReady: boolean;
  transcriptionReady: boolean;
  voiceBusy: boolean;
  voiceStatus: string;
  providerName: string;
  latestUserMessage: string;
  latestAssistantMessage: string;
  securityPrompts: ReactNode;
  hasSecurityPrompt: boolean;
  draft: string;
  onDraftChange: (value: string) => void;
  onSend: (text?: string) => void | Promise<void>;
  onTranscribe: (wav: Blob) => Promise<string>;
  onRetryVoice: () => void;
  onReplay: () => void;
  onStopSpeech: () => void;
  onSelectFace: (faceId: string) => void | Promise<void>;
  onPickFace: () => void | Promise<void>;
  onImportFaces: (paths: string[]) => void | Promise<void>;
};

const MAX_RECORDING_SECONDS = 90;

function releaseCapture(session: CaptureSession) {
  session.processor.onaudioprocess = null;
  try { session.source.disconnect(); } catch { /* already disconnected */ }
  try { session.processor.disconnect(); } catch { /* already disconnected */ }
  try { session.mute.disconnect(); } catch { /* already disconnected */ }
  session.stream.getTracks().forEach((track) => track.stop());
  if (session.context.state !== "closed") void session.context.close();
}

function formatRecordingTime(seconds: number) {
  const whole = Math.max(0, Math.floor(seconds));
  return `${String(Math.floor(whole / 60)).padStart(2, "0")}:${String(whole % 60).padStart(2, "0")}`;
}

export default function AvatarWorkspace({
  image,
  modelDataUrl,
  modelHint,
  faces,
  activeFaceId,
  faceBusy,
  faceNotice,
  stageRef,
  speaking,
  generating,
  voiceReady,
  wordTimingReady,
  transcriptionReady,
  voiceBusy,
  voiceStatus,
  providerName,
  latestUserMessage,
  latestAssistantMessage,
  securityPrompts,
  hasSecurityPrompt,
  draft,
  onDraftChange,
  onSend,
  onTranscribe,
  onRetryVoice,
  onReplay,
  onStopSpeech,
  onSelectFace,
  onPickFace,
  onImportFaces,
}: AvatarWorkspaceProps) {
  const [micState, setMicState] = useState<MicState>("idle");
  const [micError, setMicError] = useState("");
  const [micSettingsNeeded, setMicSettingsNeeded] = useState(false);
  const [recordingSeconds, setRecordingSeconds] = useState(0);
  const [dragActive, setDragActive] = useState(false);
  const [dropNotice, setDropNotice] = useState("");
  const captureRef = useRef<CaptureSession | null>(null);
  const importFacesRef = useRef(onImportFaces);
  importFacesRef.current = onImportFaces;

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void getCurrentWindow().onDragDropEvent(({ payload }) => {
      if (payload.type === "enter" || payload.type === "over") {
        setDragActive(true);
      } else if (payload.type === "leave") {
        setDragActive(false);
      } else if (payload.type === "drop") {
        setDragActive(false);
        const glbPaths = payload.paths.filter((path) => path.toLowerCase().endsWith(".glb"));
        if (glbPaths.length === 0) {
          setDropNotice("Drop a .glb 3D face model here.");
          return;
        }
        setDropNotice("");
        void importFacesRef.current(glbPaths);
      }
    }).then((stopListening) => {
      if (disposed) stopListening();
      else unlisten = stopListening;
    }).catch((cause) => {
      setDropNotice(`File drop is unavailable: ${cause instanceof Error ? cause.message : String(cause)}`);
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  function stopVoice() {
    onStopSpeech();
  }

  async function finishRecording(send: boolean) {
    const capture = captureRef.current;
    if (!capture) return;
    captureRef.current = null;
    releaseCapture(capture);
    setRecordingSeconds(0);

    if (!send) {
      capture.chunks.length = 0;
      setMicState("idle");
      setMicError("");
      return;
    }

    setMicState("transcribing");
    setMicError("");
    try {
      if (maxWindowedRms(capture.chunks, capture.sampleRate) < 0.006) {
        throw new Error("I didn't catch any speech. Try speaking a little closer to the microphone.");
      }
      const wav = encodeMonoPcm16Wav(capture.chunks, capture.sampleRate);
      capture.chunks.length = 0;
      const transcript = (await onTranscribe(wav)).trim();
      if (!transcript) throw new Error("I didn't catch any speech. Try speaking a little closer to the microphone.");
      await onSend(transcript);
    } catch (cause) {
      setMicError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      capture.chunks.length = 0;
      setMicState("idle");
    }
  }

  async function startRecording() {
    if (micState !== "idle" || !transcriptionReady || generating) return;
    setMicError("");
    setMicSettingsNeeded(false);
    if (!navigator.mediaDevices?.getUserMedia || typeof AudioContext === "undefined") {
      setMicError("Microphone capture is unavailable in this JadX window.");
      return;
    }

    let stream: MediaStream | null = null;
    let context: AudioContext | null = null;
    let capture: CaptureSession | null = null;
    try {
      stream = await navigator.mediaDevices.getUserMedia({
        audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true, autoGainControl: true },
      });
      context = new AudioContext({ latencyHint: "interactive" });
      await context.resume();
      const source = context.createMediaStreamSource(stream);
      const processor = context.createScriptProcessor(4096, 1, 1);
      const mute = context.createGain();
      mute.gain.value = 0;
      const session: CaptureSession = {
        stream,
        context,
        source,
        processor,
        mute,
        chunks: [],
        sampleCount: 0,
        sampleRate: context.sampleRate,
        startedAt: performance.now(),
      };
      capture = session;
      captureRef.current = session;
      processor.onaudioprocess = (event) => {
        if (captureRef.current !== session) return;
        const samples = new Float32Array(event.inputBuffer.getChannelData(0));
        session.chunks.push(samples);
        session.sampleCount += samples.length;
        if (session.sampleCount >= session.sampleRate * MAX_RECORDING_SECONDS) {
          void finishRecording(true);
        }
      };
      source.connect(processor);
      processor.connect(mute);
      mute.connect(context.destination);
      setRecordingSeconds(0);
      setMicState("recording");
    } catch (cause) {
      const activeCapture = captureRef.current;
      if (activeCapture === capture && activeCapture) {
        captureRef.current = null;
        releaseCapture(activeCapture);
      } else {
        stream?.getTracks().forEach((track) => track.stop());
        if (context && context.state !== "closed") void context.close();
      }
      const errorName = cause instanceof Error ? cause.name : "";
      if (errorName === "NotAllowedError" || errorName === "SecurityError") {
        setMicError("JadX cannot access the microphone. Check that JadX is enabled in System Settings → Privacy & Security → Microphone.");
        setMicSettingsNeeded(true);
      } else if (errorName === "NotFoundError") {
        setMicError("No microphone is available. Connect or enable a microphone, then try again.");
      } else if (errorName === "NotReadableError") {
        setMicError("The microphone is busy or unavailable. Close other apps using it, then try again.");
      } else {
        setMicError("Could not access the microphone. Check that it is connected and available to JadX.");
      }
    }
  }

  useEffect(() => {
    if (micState !== "recording") return;
    const timer = window.setInterval(() => {
      const capture = captureRef.current;
      if (capture) setRecordingSeconds((performance.now() - capture.startedAt) / 1000);
    }, 200);
    return () => window.clearInterval(timer);
  }, [micState]);

  useEffect(() => () => {
    const capture = captureRef.current;
    captureRef.current = null;
    if (capture) releaseCapture(capture);
  }, []);

  const status = speaking
    ? "Speaking"
    : generating
      ? "Thinking"
      : voiceBusy
        ? "Preparing local voice"
        : voiceStatus.startsWith("Voice setup failed")
          ? transcriptionReady ? "Mic ready · voice unavailable" : "Voice unavailable"
          : "Ready for you";
  const activeFace = faces.find((face) => face.id === activeFaceId);
  const faceSetupHint = activeFace && !activeFace.lipSyncReady
    ? `${activeFace.name} has no jaw or lip morph targets, so its mouth cannot animate yet. Re-export this face with ARKit blendshapes or visemes.`
    : "Drop a .glb anywhere in this view. JadX moves it into ~/JadX/avatar/faces.";

  return (
    <section className={`avatar-workspace ${dragActive ? "avatar-drop-active" : ""}`} aria-label="Avatar mode">
      <div className="avatar-ambient" aria-hidden="true">
        <div className="avatar-grid" />
        <div className="avatar-ambient-glow avatar-ambient-glow-left" />
        <div className="avatar-ambient-glow avatar-ambient-glow-right" />
        <div className="avatar-crosshair avatar-crosshair-top" />
        <div className="avatar-crosshair avatar-crosshair-bottom" />
      </div>

      <div className="avatar-layout">
        <div className={`avatar-portrait-column ${speaking ? "speaking" : ""}`}>
          <div className="avatar-orbit avatar-orbit-outer" aria-hidden="true" />
          <div className="avatar-orbit avatar-orbit-inner" aria-hidden="true" />
          <div className={`persona-avatar-visual avatar-portrait-stage ${modelDataUrl ? "avatar-glb-stage" : ""}`} ref={stageRef}>
            {modelDataUrl
              ? <Suspense fallback={<div className="avatar-glb-status">LOADING LOCAL 3D ENGINE</div>}><AvatarGlbView modelDataUrl={modelDataUrl} stageRef={stageRef} /></Suspense>
              : image && <img src={image} alt="Local avatar" draggable={false} />}
            <span className="persona-avatar-scan" aria-hidden="true" />
            <span className="avatar-light-sweep" aria-hidden="true" />
          </div>
          <div className="avatar-portrait-base" aria-hidden="true"><span /><span /><span /></div>
          <div className="avatar-portrait-label"><span>{modelDataUrl ? "LOCAL 3D AVATAR" : "FLOATING AVATAR"}</span><i />{status.toUpperCase()}</div>
        </div>

        <aside className="avatar-presence-panel">
          <div className="avatar-presence-kicker"><span className={`avatar-live-indicator ${speaking ? "active" : ""}`} /> JADX · ASSISTANT</div>
          <h1>JadX<span>.</span></h1>
          <p className="avatar-presence-subtitle">Your local assistant, here with you.</p>
          <div className="avatar-face-controls">
            <label className="avatar-face-select-wrap">
              <span>FACE MODEL</span>
              <select
                aria-label="Active face model"
                value={activeFaceId}
                disabled={faceBusy || faces.length === 0}
                onChange={(event) => void onSelectFace(event.target.value)}
              >
                {faces.length === 0 ? <option value="">No 3D face added</option> : faces.map((face) => <option key={face.id} value={face.id}>{face.name}{face.lipSyncReady ? " · lips ready" : " · no lip shapes"}</option>)}
              </select>
            </label>
            <button type="button" className="avatar-add-face-button" onClick={() => void onPickFace()} disabled={faceBusy}>
              <Plus size={14} /> {faceBusy ? "Adding…" : "Add face"}
            </button>
          </div>
          <p className="avatar-model-setup-note">
            {faceNotice || dropNotice || (modelHint && !modelDataUrl ? modelHint : faceSetupHint)}
          </p>
          {hasSecurityPrompt && <div className="avatar-security-prompts" aria-label="Assistant approvals and security prompts" aria-live="assertive">{securityPrompts}</div>}
          <div className="avatar-status-card" aria-live="polite">
            <div className="avatar-status-orb"><AudioLines size={19} /></div>
            <div><strong>{status}</strong><span>{voiceReady ? wordTimingReady ? "Qwen voice · local word-timed lipsync" : "Qwen voice · approximate lipsync" : transcriptionReady ? "Local mic ready · reply audio unavailable" : voiceStatus || "Connecting to on-device voice"}</span></div>
            {voiceReady && <button type="button" onClick={onReplay} disabled={!latestAssistantMessage || speaking || generating} aria-label="Replay the last reply" title="Replay last reply"><Volume2 size={16} /></button>}
            {voiceReady && <button type="button" className="avatar-stop-voice-button" onClick={stopVoice} disabled={!speaking && !generating} aria-label="Stop spoken reply" title="Stop this reply's voice"><CircleStop size={15} /><span>Stop</span></button>}
            {!voiceReady && !voiceBusy && <button type="button" onClick={onRetryVoice} aria-label="Retry local voice setup" title="Retry local voice setup"><RotateCw size={15} /></button>}
          </div>
          {!modelDataUrl && faces.length === 0 && <p className="avatar-model-setup-note">Add a 3D model to enable facial animation.</p>}

          <div className="avatar-recent-response" aria-live="polite">
            <div className="avatar-message-label">{generating ? "JADX IS THINKING" : speaking ? "JADX IS SPEAKING" : "LATEST RESPONSE"}</div>
            {latestUserMessage && <div className="avatar-latest-prompt">YOU · {latestUserMessage}</div>}
            <p>{latestAssistantMessage || "Start a conversation by speaking or typing below."}</p>
          </div>

          <div className={`avatar-waveform ${speaking ? "active" : ""}`} aria-label={speaking ? "JadX is speaking" : "Voice is ready"}>
            {Array.from({ length: 25 }, (_, index) => <i key={index} style={{ "--wave-index": index } as React.CSSProperties} />)}
          </div>
          <div className="avatar-route-note"><ShieldCheck size={13} /><span>Mic audio is transcribed on this Mac. Chat text goes to <b>{providerName}</b>.</span></div>
        </aside>
      </div>

      <form className="avatar-composer" onSubmit={(event) => { event.preventDefault(); void onSend(); }}>
        <div className="avatar-input-state" aria-live="polite">
          {micState === "recording" ? <><span className="avatar-recording-dot" /> LISTENING · {formatRecordingTime(recordingSeconds)}</> : micState === "transcribing" ? <><AudioLines size={12} className="avatar-transcribing-icon" /> TRANSCRIBING LOCALLY</> : generating ? <><span className="avatar-thinking-dot" /> WAITING FOR JADX</> : transcriptionReady ? <><span className="avatar-ready-dot" /> ON-DEVICE SPEECH READY</> : <><span className="avatar-thinking-dot" /> STARTING LOCAL SPEECH SERVICE</>}
        </div>
        {micError && <div className="avatar-mic-error" role="status"><span>{micError}</span>{micSettingsNeeded && <button type="button" className="avatar-mic-settings" onClick={() => void invoke("open_microphone_settings").catch(() => setMicError("Could not open System Settings. Go to System Settings → Privacy & Security → Microphone and allow JadX."))}><Settings2 size={12} /> Open Microphone Settings</button>}</div>}
        <div className="avatar-composer-row">
          <textarea
            value={draft}
            onChange={(event) => onDraftChange(event.target.value)}
            onKeyDown={(event) => { if (event.key === "Enter" && !event.shiftKey) { event.preventDefault(); if (draft.trim() && !generating) void onSend(); } }}
            placeholder="Talk or type to JadX…"
            aria-label="Message JadX"
            rows={1}
            disabled={micState === "recording" || micState === "transcribing"}
          />
          {micState === "recording" ? <>
            <button type="button" className="avatar-mic-cancel" onClick={() => void finishRecording(false)} aria-label="Discard recording" title="Discard recording"><X size={16} /></button>
            <button type="button" className="avatar-record-button recording" onClick={() => void finishRecording(true)} aria-label="Stop recording and send" title="Stop recording and send"><CircleStop size={17} /><span>Finish</span></button>
          </> : <button type="button" className={`avatar-record-button ${micState === "transcribing" ? "transcribing" : ""}`} disabled={!transcriptionReady || generating || micState === "transcribing"} onClick={() => void startRecording()} aria-label="Start local voice input" title={transcriptionReady ? "Speak to JadX; audio stays on this Mac" : "Preparing local speech service"}>
            {micState === "transcribing" ? <><AudioLines size={17} /><span>Listening</span></> : <><Mic size={17} /><span>Speak</span></>}
          </button>}
          <button type="submit" className="avatar-send-button" disabled={!draft.trim() || generating || micState !== "idle"} aria-label="Send message" title="Send message"><Send size={16} /></button>
        </div>
        <div className="avatar-composer-footer"><span>ENTER TO SEND <b>·</b> SHIFT+ENTER FOR A NEW LINE</span><span>LOCAL QWEN ASR · QWEN VOICE</span></div>
      </form>
      {dragActive && <div className="avatar-drop-overlay" aria-live="polite"><div><Plus size={23} /><strong>Drop your 3D face model</strong><span>GLB files are copied into JadX’s local face library.</span></div></div>}
    </section>
  );
}
