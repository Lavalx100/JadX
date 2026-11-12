export type PersonaSpeechConfig = {
  base_url: string;
  model: string;
  transcription_model?: string;
  transcription_ready?: boolean;
  alignment_model?: string | null;
  word_timing_model?: string | null;
  ref_audio: string;
  ref_text: string;
  sample_rate: number;
};

export type PersonaAvatarMotionResult = {
  framesBase64: string;
  fps: number;
  frameCount: number;
};

export const PERSONA_AUDIO_BASE_URL = "http://127.0.0.1:8008";
export const PERSONA_TRANSCRIPTION_MODEL = "mlx-community/Qwen3-ASR-0.6B-8bit";
export const PERSONA_TTS_STREAMING_INTERVAL_SECONDS = 0.32;
const MAX_ALIGNMENT_AUDIO_BYTES = 6 * 1024 * 1024;

export type PersonaViseme = "open" | "wide" | "round" | "closed" | "neutral";
export type PersonaMouthCue = "sil" | "pp" | "ff" | "th" | "dd" | "kk" | "ch" | "ss" | "nn" | "rr" | "aa" | "e" | "i" | "o" | "u";

type SpeechTimeline = {
  text: string;
  startAt: number | null;
  audioDuration: number;
  expectedDuration: number;
  complete: boolean;
  alignedWords: PersonaWordTiming[] | null;
  faceFrames: Float32Array | null;
  faceFramesStartSeconds: number;
  faceFrameCount: number;
  faceFps: number;
};

export type PersonaWordTiming = { text: string; start: number; end: number };

export function mouthLevelFromRms(rms: number) {
  if (!Number.isFinite(rms)) return 0;
  const normalized = Math.max(0, Math.min(1, (rms - 0.0025) / 0.035));
  return Math.pow(normalized, 0.62);
}

export function mouthRoundnessFromSpectrum(spectrumDb: Float32Array, sampleRate: number, fftSize: number) {
  if (!Number.isFinite(sampleRate) || sampleRate <= 0 || !Number.isFinite(fftSize) || fftSize <= 0) return 0.45;
  const binHz = sampleRate / fftSize;
  let lowEnergy = 0;
  let highEnergy = 0;
  let lowBins = 0;
  let highBins = 0;
  for (let index = 0; index < spectrumDb.length; index += 1) {
    const frequency = index * binHz;
    if (frequency < 300 || frequency > 3200 || !Number.isFinite(spectrumDb[index])) continue;
    const energy = 10 ** (spectrumDb[index] / 10);
    if (frequency < 1200) {
      lowEnergy += energy;
      lowBins += 1;
    } else {
      highEnergy += energy;
      highBins += 1;
    }
  }
  if (!lowBins || !highBins) return 0.45;
  const low = lowEnergy / lowBins;
  const high = highEnergy / highBins;
  const ratio = low / (low + high);
  return Math.max(0, Math.min(1, (ratio - 0.28) / 0.25));
}

export function mouthMotionForAudio(level: number, roundness: number) {
  const opening = Number.isFinite(level) ? Math.max(0, Math.min(1, level)) : 0;
  const rounding = Number.isFinite(roundness) ? Math.max(0, Math.min(1, roundness)) : 0.45;
  return {
    scaleY: opening * 1.05,
    opacity: opening <= 0.04 ? 0 : Math.min(1, (opening - 0.04) / 0.24),
    scaleX: 1.02 - rounding * 0.14,
  };
}

function cleanSpeechText(value: string) {
  return value
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
    .replace(/^\s{0,3}#{1,6}\s+/gm, "")
    .replace(/^\s*[-*+]\s+/gm, "")
    .replace(/[*_~]{1,3}/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

function estimateSpeechDuration(text: string) {
  const letters = Array.from(text).filter((character) => /[\p{L}\p{N}]/u.test(character)).length;
  const pauses = (text.match(/[,;:]/g)?.length ?? 0) * 0.1 + (text.match(/[.!?…]/g)?.length ?? 0) * 0.18;
  return Math.max(0.35, letters / 13.5 + pauses);
}

type MouthCueUnit = { cue: PersonaMouthCue; weight: number };

const LATIN_MOUTH_CUES: Record<string, PersonaMouthCue> = {
  "a": "aa", "á": "aa", "à": "aa", "â": "aa", "ä": "aa", "æ": "aa", "e": "e", "é": "e", "è": "e", "ê": "e", "ë": "e",
  "i": "i", "í": "i", "ì": "i", "î": "i", "ï": "i", "y": "i", "ý": "i", "o": "o", "ó": "o", "ò": "o", "ô": "o", "ö": "o",
  "u": "u", "ú": "u", "ù": "u", "û": "u", "ü": "u", "w": "u", "b": "pp", "m": "pp", "p": "pp", "f": "ff", "v": "ff",
  "t": "dd", "d": "dd", "c": "kk", "k": "kk", "g": "kk", "q": "kk", "h": "kk", "s": "ss", "z": "ss", "x": "ss",
  "j": "ch", "l": "nn", "n": "nn", "r": "rr",
};

const ARABIC_MOUTH_CUES: Record<string, PersonaMouthCue> = {
  "ا": "aa", "آ": "aa", "أ": "aa", "إ": "i", "ى": "aa", "َ": "aa", "ً": "aa", "ي": "i", "ئ": "i", "ِ": "i", "ٍ": "i",
  "و": "u", "ؤ": "u", "ُ": "u", "ٌ": "u", "ب": "pp", "م": "pp", "ف": "ff", "ث": "th", "ذ": "th", "ظ": "th",
  "ت": "dd", "د": "dd", "ط": "dd", "ض": "dd", "ك": "kk", "ق": "kk", "غ": "kk", "خ": "kk", "ح": "kk", "ع": "aa",
  "ج": "ch", "ش": "ch", "س": "ss", "ص": "ss", "ز": "ss", "ن": "nn", "ل": "nn", "ر": "rr",
};

const LATIN_DIGRAPHS: Record<string, PersonaMouthCue[]> = {
  th: ["th"], sh: ["ch"], ch: ["ch"], zh: ["ch"], ph: ["ff"], ng: ["nn"],
  ee: ["e"], ea: ["e"], ie: ["i"], oo: ["u"], ue: ["u"],
  ai: ["aa", "i"], ay: ["aa", "i"], oi: ["o", "i"], oy: ["o", "i"], ou: ["aa", "u"], ow: ["aa", "u"],
};

function mouthCueUnits(text: string): MouthCueUnit[] {
  const characters = Array.from(text.normalize("NFC").toLocaleLowerCase());
  const units: MouthCueUnit[] = [];
  const add = (cue: PersonaMouthCue, weight?: number) => {
    const vowel = cue === "aa" || cue === "e" || cue === "i" || cue === "o" || cue === "u";
    units.push({ cue, weight: weight ?? (cue === "sil" ? 0.42 : vowel ? 1.18 : 0.72) });
  };

  for (let index = 0; index < characters.length; index += 1) {
    const character = characters[index];
    if (/\s/u.test(character)) {
      add("sil", 0.34);
      continue;
    }
    if (/[.!?…؟،,;:]/u.test(character)) {
      add("sil", /[.!?…؟]/u.test(character) ? 0.88 : 0.52);
      continue;
    }
    const pair = `${character}${characters[index + 1] ?? ""}`;
    const pairedCues = LATIN_DIGRAPHS[pair];
    if (pairedCues) {
      pairedCues.forEach((cue) => add(cue, 0.94));
      index += 1;
      continue;
    }
    const cue = ARABIC_MOUTH_CUES[character] ?? LATIN_MOUTH_CUES[character];
    if (cue) add(cue);
    else if (/\p{L}/u.test(character)) add("nn", 0.62);
    // Ignore combining marks, apostrophes, and other non-speech punctuation.
  }
  return units;
}

export function mouthCueAtTextProgress(text: string, progress: number): PersonaMouthCue {
  const units = mouthCueUnits(text);
  if (!units.length) return "sil";
  const totalWeight = units.reduce((total, unit) => total + unit.weight, 0);
  let remaining = Math.max(0, Math.min(0.999, progress)) * totalWeight;
  for (const unit of units) {
    if (remaining < unit.weight) return unit.cue;
    remaining -= unit.weight;
  }
  return units.at(-1)?.cue ?? "sil";
}

function coarseVisemeForCue(cue: PersonaMouthCue): PersonaViseme {
  if (cue === "sil") return "neutral";
  if (cue === "pp") return "closed";
  if (cue === "aa") return "open";
  if (cue === "e" || cue === "i") return "wide";
  if (cue === "o" || cue === "u" || cue === "rr") return "round";
  return "neutral";
}

export function visemeAtTextProgress(text: string, progress: number): PersonaViseme {
  return coarseVisemeForCue(mouthCueAtTextProgress(text, progress));
}

export function parsePersonaWordTimings(payload: unknown, audioDuration: number): PersonaWordTiming[] {
  if (!payload || typeof payload !== "object" || !Number.isFinite(audioDuration) || audioDuration <= 0) return [];
  const record = payload as { items?: unknown; segments?: unknown; words?: unknown };
  const nestedWords = Array.isArray(record.segments)
    ? record.segments.flatMap((segment) => segment && typeof segment === "object" && Array.isArray((segment as { words?: unknown }).words) ? (segment as { words: unknown[] }).words : [])
    : [];
  const items = Array.isArray(record.items) ? record.items : Array.isArray(record.words) ? record.words : nestedWords.length ? nestedWords : Array.isArray(record.segments) ? record.segments : [];
  const timings: PersonaWordTiming[] = [];
  for (const candidate of items) {
    if (!candidate || typeof candidate !== "object") continue;
    const item = candidate as Record<string, unknown>;
    const text = typeof item.text === "string" ? item.text.trim() : typeof item.word === "string" ? item.word.trim() : "";
    const start = Number(item.start_time ?? item.start);
    const end = Number(item.end_time ?? item.end);
    if (!text || !Number.isFinite(start) || !Number.isFinite(end) || end <= start) continue;
    const boundedStart = Math.max(0, Math.min(audioDuration, start));
    const boundedEnd = Math.max(0, Math.min(audioDuration, end));
    if (boundedEnd - boundedStart < 0.012) continue;
    timings.push({ text, start: boundedStart, end: boundedEnd });
    if (timings.length >= 500) break;
  }
  return timings.sort((left, right) => left.start - right.start);
}

export function visemeAtWordTimings(timings: PersonaWordTiming[], elapsedSeconds: number): PersonaViseme {
  if (!Number.isFinite(elapsedSeconds) || elapsedSeconds < 0) return "neutral";
  const active = timings.find((timing) => elapsedSeconds >= timing.start && elapsedSeconds <= timing.end);
  if (!active) return "neutral";
  const progress = (elapsedSeconds - active.start) / Math.max(0.012, active.end - active.start);
  return visemeAtTextProgress(active.text, progress);
}

export function mouthCueAtWordTimings(timings: PersonaWordTiming[], elapsedSeconds: number): PersonaMouthCue {
  if (!Number.isFinite(elapsedSeconds) || elapsedSeconds < 0) return "sil";
  const active = timings.find((timing) => elapsedSeconds >= timing.start && elapsedSeconds <= timing.end);
  if (!active) return "sil";
  const progress = (elapsedSeconds - active.start) / Math.max(0.012, active.end - active.start);
  return mouthCueAtTextProgress(active.text, progress);
}

export function forcedAlignerLanguageForText(text: string): string | null {
  if (!text.trim() || /[\u0600-\u06ff\u0750-\u077f\u08a0-\u08ff]/u.test(text)) return null;
  if (/[\u1100-\u11ff\uac00-\ud7af]/u.test(text)) return "Korean";
  if (/[\u3040-\u30ff]/u.test(text)) return "Japanese";
  if (/[\u3400-\u9fff]/u.test(text)) return "Chinese";
  if (/[\u0400-\u052f]/u.test(text)) return "Russian";
  if (/\p{Script=Latin}/u.test(text)) return "English";
  return null;
}

function appendBytes(left: Uint8Array, right: Uint8Array) {
  if (!left.length) return new Uint8Array(right);
  if (!right.length) return new Uint8Array(left);
  const joined = new Uint8Array(left.length + right.length);
  joined.set(left);
  joined.set(right, left.length);
  return joined;
}

export class PersonaSpeechPlayer {
  onError?: (message: string) => void;
  onMouthMotion?: (level: number, roundness: number, viseme: PersonaViseme, mouthCue: PersonaMouthCue, faceWeights: Float32Array | null) => void;
  onFaceMotionInput?: (audioBase64: string) => Promise<PersonaAvatarMotionResult>;
  onFaceMotionError?: (message: string) => void;
  onPlaybackState?: (speaking: boolean) => void;

  private config: PersonaSpeechConfig | null = null;
  private context: AudioContext | null = null;
  private analyser: AnalyserNode | null = null;
  private waveform: Float32Array<ArrayBuffer> | null = null;
  private spectrum: Float32Array<ArrayBuffer> | null = null;
  private mouthFrame = 0;
  private mouthLevel = 0;
  private mouthRoundness = 0.45;
  private lastMouthFrameAt = 0;
  private speaking = false;
  private enabled = false;
  private mutedForTurn = false;
  private speechTimelines: SpeechTimeline[] = [];
  private pendingText = "";
  private sentenceQueue: string[] = [];
  private controllers = new Set<AbortController>();
  private sources = new Set<AudioBufferSourceNode>();
  private nextStartAt = 0;
  private pumping = false;
  private faceAudioRing = new Float32Array(48_000);
  private faceAudioSamplesWritten = 0;
  private faceMotionGeneration = 0;
  private faceMotionRunning = false;
  private pendingFaceMotion: { audioBase64: string; windowStartSeconds: number; timeline: SpeechTimeline; generation: number } | null = null;
  private faceMotionDisabled = false;
  private faceWeightScratch = new Float32Array(52);

  async enable(config: PersonaSpeechConfig) {
    this.config = config;
    this.enabled = true;
    if (!this.context || this.context.state === "closed") {
      this.context = new AudioContext();
      this.analyser = this.context.createAnalyser();
      this.analyser.fftSize = 1024;
      this.analyser.smoothingTimeConstant = 0.18;
      this.analyser.connect(this.context.destination);
      this.waveform = new Float32Array(this.analyser.fftSize);
      this.spectrum = new Float32Array(this.analyser.frequencyBinCount);
    }
    this.faceMotionDisabled = false;
    await this.context.resume();
  }

  beginTurn() {
    this.mutedForTurn = false;
    this.cancelTurn();
  }

  stopCurrentTurnSpeech() {
    this.mutedForTurn = true;
    this.cancelTurn();
  }

  acceptDelta(delta: string) {
    if (!this.enabled || this.mutedForTurn || !delta) return;
    this.pendingText += delta;

    const boundary = /[.!?…](?:[\"'”’)}\]]*)\s+/g;
    let match: RegExpExecArray | null;
    let consumed = 0;
    while ((match = boundary.exec(this.pendingText)) !== null) {
      const end = boundary.lastIndex;
      const candidate = cleanSpeechText(this.pendingText.slice(consumed, end));
      if (candidate.length >= 16) {
        this.sentenceQueue.push(candidate);
        consumed = end;
      }
    }
    if (consumed) this.pendingText = this.pendingText.slice(consumed);

    if (this.pendingText.length > 220) {
      const splitAt = this.pendingText.lastIndexOf(" ", 180);
      if (splitAt >= 80) {
        const candidate = cleanSpeechText(this.pendingText.slice(0, splitAt));
        if (candidate) this.sentenceQueue.push(candidate);
        this.pendingText = this.pendingText.slice(splitAt + 1);
      }
    }
    void this.pump();
  }

  finishTurn() {
    if (!this.enabled || this.mutedForTurn) return;
    const remainder = cleanSpeechText(this.pendingText);
    this.pendingText = "";
    if (remainder) this.sentenceQueue.push(remainder);
    void this.pump();
  }

  cancelTurn() {
    this.faceMotionGeneration += 1;
    this.pendingFaceMotion = null;
    this.faceAudioRing.fill(0);
    this.faceAudioSamplesWritten = 0;
    this.pendingText = "";
    this.sentenceQueue = [];
    for (const controller of this.controllers) controller.abort();
    this.controllers.clear();
    for (const source of this.sources) {
      try { source.stop(); } catch { /* it may already have ended */ }
      source.disconnect();
    }
    this.sources.clear();
    this.speechTimelines = [];
    this.nextStartAt = this.context?.currentTime ?? 0;
    this.stopMouthMotion();
  }

  async disable() {
    this.enabled = false;
    this.cancelTurn();
    this.config = null;
    if (this.context && this.context.state !== "closed") await this.context.close();
    this.context = null;
    this.analyser = null;
    this.waveform = null;
    this.spectrum = null;
  }

  private async pump() {
    if (this.pumping || !this.enabled || !this.config) return;
    this.pumping = true;
    try {
      while (this.enabled && this.config && this.sentenceQueue.length) {
        const sentence = this.sentenceQueue.shift()!;
        await this.speak(sentence, this.config);
      }
    } finally {
      this.pumping = false;
      if (this.enabled && this.sentenceQueue.length) void this.pump();
    }
  }

  private async speak(text: string, config: PersonaSpeechConfig) {
    const controller = new AbortController();
    this.controllers.add(controller);
    const timeline: SpeechTimeline = {
      text,
      startAt: null,
      audioDuration: 0,
      expectedDuration: estimateSpeechDuration(text),
      complete: false,
      alignedWords: null,
      faceFrames: null,
      faceFramesStartSeconds: 0,
      faceFrameCount: 0,
      faceFps: 0,
    };
    this.faceAudioRing.fill(0);
    this.faceAudioSamplesWritten = 0;
    this.speechTimelines.push(timeline);
    try {
      const response = await fetch(`${config.base_url}/v1/audio/speech`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          model: config.model,
          input: text,
          ref_audio: config.ref_audio,
          ref_text: config.ref_text,
          stream: true,
          streaming_interval: PERSONA_TTS_STREAMING_INTERVAL_SECONDS,
          response_format: "pcm",
        }),
        signal: controller.signal,
      });
      if (!response.ok) {
        const details = (await response.text()).slice(0, 240);
        throw new Error(details || `Local speech server returned ${response.status}`);
      }
      if (!response.body) throw new Error("Local speech server returned no audio stream");

      const reader = response.body.getReader();
      let pending = new Uint8Array();
      let carry = new Uint8Array();
      const alignmentChunks: Uint8Array[] = [];
      let alignmentBytes = 0;
      const forcedLanguage = forcedAlignerLanguageForText(text);
      const alignerModel = config.alignment_model && forcedLanguage ? config.alignment_model : null;
      const timingModel = alignerModel ?? config.word_timing_model ?? null;
      // Keep playback buffers aligned with Qwen's emitted chunk cadence so a
      // fraction of each chunk is not held back and played late.
      const scheduleBytes = Math.floor(config.sample_rate * PERSONA_TTS_STREAMING_INTERVAL_SECONDS) * 2;
      while (true) {
        const { done, value } = await reader.read();
        if (controller.signal.aborted) break;
        if (value?.length) {
          const combined = appendBytes(carry, value);
          const alignedLength = combined.length - (combined.length % 2);
          carry = combined.slice(alignedLength);
          if (timingModel && alignmentBytes + alignedLength <= MAX_ALIGNMENT_AUDIO_BYTES) {
            alignmentChunks.push(combined.slice(0, alignedLength));
            alignmentBytes += alignedLength;
          } else if (timingModel) {
            alignmentChunks.length = 0;
            alignmentBytes = MAX_ALIGNMENT_AUDIO_BYTES + 1;
          }
          pending = appendBytes(pending, combined.slice(0, alignedLength));
          while (pending.length >= scheduleBytes) {
            this.schedulePcm(pending.slice(0, scheduleBytes), config.sample_rate, timeline);
            pending = pending.slice(scheduleBytes);
          }
        }
        if (done) break;
      }
      if (pending.length >= 2 && !controller.signal.aborted) this.schedulePcm(pending, config.sample_rate, timeline);
      if (!controller.signal.aborted) {
        timeline.complete = true;
        if (timingModel && alignmentBytes > 0 && alignmentBytes <= MAX_ALIGNMENT_AUDIO_BYTES) {
          void this.alignTimeline(timeline, text, alignmentChunks, alignmentBytes, config, timingModel, alignerModel !== null, forcedLanguage);
        }
      }
    } catch (cause) {
      if (!controller.signal.aborted) {
        const message = cause instanceof Error ? cause.message : String(cause);
        this.onError?.(`Local voice stopped: ${message}`);
        this.cancelTurn();
      }
    } finally {
      this.controllers.delete(controller);
    }
  }

  private async alignTimeline(
    timeline: SpeechTimeline,
    text: string,
    chunks: Uint8Array[],
    byteLength: number,
    config: PersonaSpeechConfig,
    timingModel: string,
    usesForcedAlignment: boolean,
    language: string | null,
  ) {
    const controller = new AbortController();
    this.controllers.add(controller);
    const timeout = window.setTimeout(() => controller.abort(), 15_000);
    try {
      const wavBuffer = new ArrayBuffer(44 + byteLength);
      const wav = new DataView(wavBuffer);
      const writeTag = (offset: number, value: string) => {
        for (let index = 0; index < value.length; index += 1) wav.setUint8(offset + index, value.charCodeAt(index));
      };
      writeTag(0, "RIFF");
      wav.setUint32(4, 36 + byteLength, true);
      writeTag(8, "WAVE");
      writeTag(12, "fmt ");
      wav.setUint32(16, 16, true);
      wav.setUint16(20, 1, true);
      wav.setUint16(22, 1, true);
      wav.setUint32(24, config.sample_rate, true);
      wav.setUint32(28, config.sample_rate * 2, true);
      wav.setUint16(32, 2, true);
      wav.setUint16(34, 16, true);
      writeTag(36, "data");
      wav.setUint32(40, byteLength, true);
      const samples = new Uint8Array(wavBuffer, 44);
      let cursor = 0;
      for (const chunk of chunks) {
        samples.set(chunk, cursor);
        cursor += chunk.length;
      }
      chunks.length = 0;

      const body = new FormData();
      body.set("file", new Blob([wavBuffer], { type: "audio/wav" }), "speech.wav");
      body.set("model", timingModel);
      if (usesForcedAlignment) {
        body.set("text", text);
        if (language) body.set("language", language);
        body.set("response_format", "ndjson");
      } else {
        body.set("word_timestamps", "true");
        body.set("response_format", "verbose_json");
      }
      const response = await fetch(`${config.base_url}/v1/audio/transcriptions`, {
        method: "POST",
        body,
        signal: controller.signal,
      });
      if (!response.ok) return;
      const responseText = await response.text();
      const payloads = usesForcedAlignment
        ? responseText.split(/\r?\n/).filter(Boolean).flatMap((line) => { try { return [JSON.parse(line)]; } catch { return []; } })
        : (() => { try { return [JSON.parse(responseText)]; } catch { return []; } })();
      for (const payload of payloads) {
        const timings = parsePersonaWordTimings(payload, timeline.audioDuration);
        if (timings.length) {
          timeline.alignedWords = timings;
          return;
        }
      }
    } catch { /* optional local alignment falls back to text pacing */ }
    finally {
      window.clearTimeout(timeout);
      this.controllers.delete(controller);
    }
  }

  private schedulePcm(pcm: Uint8Array, sampleRate: number, timeline: SpeechTimeline) {
    const context = this.context;
    if (!context || context.state === "closed" || pcm.length < 2) return;
    const count = Math.floor(pcm.length / 2);
    const buffer = context.createBuffer(1, count, sampleRate);
    const channel = buffer.getChannelData(0);
    const view = new DataView(pcm.buffer, pcm.byteOffset, count * 2);
    for (let index = 0; index < count; index += 1) channel[index] = view.getInt16(index * 2, true) / 32768;
    this.updateFaceMotionAudio(channel, sampleRate, timeline);

    const source = context.createBufferSource();
    source.buffer = buffer;
    if (this.analyser) source.connect(this.analyser);
    else source.connect(context.destination);
    const startAt = Math.max(context.currentTime + 0.025, this.nextStartAt);
    this.nextStartAt = startAt + buffer.duration;
    timeline.startAt ??= startAt;
    timeline.audioDuration += buffer.duration;
    this.sources.add(source);
    this.setSpeaking(true);
    this.animateMouth();
    source.onended = () => {
      source.disconnect();
      this.sources.delete(source);
      if (this.sources.size === 0) this.stopMouthMotion();
    };
    source.start(startAt);
  }

  private setSpeaking(speaking: boolean) {
    if (this.speaking === speaking) return;
    this.speaking = speaking;
    this.onPlaybackState?.(speaking);
  }

  private stopMouthMotion() {
    if (this.mouthFrame) window.cancelAnimationFrame(this.mouthFrame);
    this.mouthFrame = 0;
    this.mouthLevel = 0;
    this.mouthRoundness = 0.45;
    this.lastMouthFrameAt = 0;
    this.speechTimelines = [];
    this.onMouthMotion?.(0, this.mouthRoundness, "neutral", "sil", null);
    this.setSpeaking(false);
  }

  private updateFaceMotionAudio(audio: Float32Array, sampleRate: number, timeline: SpeechTimeline) {
    if (!this.onFaceMotionInput || this.faceMotionDisabled || !Number.isFinite(sampleRate) || sampleRate <= 0 || audio.length === 0) return;
    const outputCount = Math.max(1, Math.round(audio.length * 16_000 / sampleRate));
    for (let index = 0; index < outputCount; index += 1) {
      const sourcePosition = index * sampleRate / 16_000;
      const left = Math.min(audio.length - 1, Math.floor(sourcePosition));
      const right = Math.min(audio.length - 1, left + 1);
      const fraction = sourcePosition - left;
      const sample = audio[left] + (audio[right] - audio[left]) * fraction;
      this.faceAudioRing[this.faceAudioSamplesWritten % this.faceAudioRing.length] = sample;
      this.faceAudioSamplesWritten += 1;
    }

    const windowSamples = new Float32Array(this.faceAudioRing.length);
    const firstSample = this.faceAudioSamplesWritten - windowSamples.length;
    for (let index = 0; index < windowSamples.length; index += 1) {
      const globalIndex = firstSample + index;
      if (globalIndex >= 0) windowSamples[index] = this.faceAudioRing[globalIndex % this.faceAudioRing.length];
    }
    const bytes = new Uint8Array(windowSamples.length * 4);
    const data = new DataView(bytes.buffer);
    for (let index = 0; index < windowSamples.length; index += 1) data.setFloat32(index * 4, windowSamples[index], true);
    let audioBase64 = "";
    const chunkSize = 16_384;
    for (let index = 0; index < bytes.length; index += chunkSize) {
      audioBase64 += btoa(String.fromCharCode(...bytes.subarray(index, Math.min(bytes.length, index + chunkSize))));
    }
    this.pendingFaceMotion = {
      audioBase64,
      windowStartSeconds: firstSample / 16_000,
      timeline,
      generation: this.faceMotionGeneration,
    };
    void this.pumpFaceMotion();
  }

  private async pumpFaceMotion() {
    if (this.faceMotionRunning || !this.onFaceMotionInput || this.faceMotionDisabled) return;
    this.faceMotionRunning = true;
    try {
      while (this.pendingFaceMotion && this.enabled && !this.faceMotionDisabled) {
        const request = this.pendingFaceMotion;
        this.pendingFaceMotion = null;
        if (request.generation !== this.faceMotionGeneration) continue;
        const result = await this.onFaceMotionInput(request.audioBase64);
        if (request.generation !== this.faceMotionGeneration) continue;
        const bytes = window.atob(result.framesBase64);
        const expectedBytes = result.frameCount * 52 * 4;
        if (result.fps <= 0 || result.frameCount <= 0 || bytes.length !== expectedBytes) {
          throw new Error("The local face model returned an invalid frame sequence.");
        }
        const frames = new Float32Array(result.frameCount * 52);
        const frameBytes = new Uint8Array(bytes.length);
        for (let index = 0; index < bytes.length; index += 1) frameBytes[index] = bytes.charCodeAt(index);
        const view = new DataView(frameBytes.buffer);
        for (let index = 0; index < frames.length; index += 1) {
          const value = view.getFloat32(index * 4, true);
          frames[index] = Number.isFinite(value) ? Math.max(0, Math.min(1, value)) : 0;
        }
        request.timeline.faceFrames = frames;
        request.timeline.faceFramesStartSeconds = request.windowStartSeconds;
        request.timeline.faceFrameCount = result.frameCount;
        request.timeline.faceFps = result.fps;
      }
    } catch (cause) {
      this.faceMotionDisabled = true;
      this.pendingFaceMotion = null;
      this.onFaceMotionError?.(cause instanceof Error ? cause.message : String(cause));
    } finally {
      this.faceMotionRunning = false;
      if (this.pendingFaceMotion && this.enabled && !this.faceMotionDisabled) void this.pumpFaceMotion();
    }
  }

  private faceWeightsAt(timeline: SpeechTimeline | null | undefined, elapsedSeconds: number) {
    const frames = timeline?.faceFrames;
    if (!timeline || !frames || timeline.faceFps <= 0 || timeline.faceFrameCount <= 0) return null;
    const position = (elapsedSeconds - timeline.faceFramesStartSeconds) * timeline.faceFps;
    if (position < 0 || position > timeline.faceFrameCount - 1) return null;
    const before = Math.floor(position);
    const after = Math.min(timeline.faceFrameCount - 1, before + 1);
    const blend = position - before;
    for (let index = 0; index < this.faceWeightScratch.length; index += 1) {
      const start = frames[before * 52 + index];
      const end = frames[after * 52 + index];
      this.faceWeightScratch[index] = start + (end - start) * blend;
    }
    return this.faceWeightScratch;
  }

  private animateMouth() {
    if (this.mouthFrame || !this.analyser || !this.waveform) return;
    const tick = (timestamp: number) => {
      this.mouthFrame = 0;
      const analyser = this.analyser;
      const waveform = this.waveform;
      const spectrum = this.spectrum;
      if (!analyser || !waveform || !spectrum || this.sources.size === 0) {
        this.stopMouthMotion();
        return;
      }
      analyser.getFloatTimeDomainData(waveform);
      analyser.getFloatFrequencyData(spectrum);
      let energy = 0;
      for (const sample of waveform) energy += sample * sample;
      const rms = Math.sqrt(energy / waveform.length);
      const target = mouthLevelFromRms(rms);
      const deltaSeconds = this.lastMouthFrameAt ? Math.max(0.001, (timestamp - this.lastMouthFrameAt) / 1000) : 1 / 60;
      this.lastMouthFrameAt = timestamp;
      const timeConstant = target > this.mouthLevel ? 0.045 : 0.095;
      const blend = 1 - Math.exp(-deltaSeconds / timeConstant);
      this.mouthLevel += (target - this.mouthLevel) * blend;
      const targetRoundness = mouthRoundnessFromSpectrum(spectrum, this.context?.sampleRate ?? 0, analyser.fftSize);
      const roundnessBlend = 1 - Math.exp(-deltaSeconds / 0.075);
      this.mouthRoundness += (targetRoundness - this.mouthRoundness) * roundnessBlend;
      const now = this.context?.currentTime ?? 0;
      this.speechTimelines = this.speechTimelines.filter((timeline) =>
        !timeline.complete || timeline.startAt === null || now < timeline.startAt + timeline.audioDuration,
      );
      const activeTimeline = this.speechTimelines.find((timeline) => {
        if (timeline.startAt === null || now < timeline.startAt) return false;
        const duration = timeline.complete
          ? timeline.audioDuration
          : Math.max(timeline.expectedDuration, timeline.audioDuration);
        return now < timeline.startAt + duration;
      });
      const duration = activeTimeline
        ? (activeTimeline.complete ? activeTimeline.audioDuration : Math.max(activeTimeline.expectedDuration, activeTimeline.audioDuration))
        : 1;
      const progress = activeTimeline?.startAt === null || !activeTimeline
        ? 0
        : Math.max(0, Math.min(0.999, (now - activeTimeline.startAt) / Math.max(duration, 0.01)));
      const viseme = activeTimeline
        ? activeTimeline.alignedWords
          ? visemeAtWordTimings(activeTimeline.alignedWords, now - (activeTimeline.startAt ?? now))
          : visemeAtTextProgress(activeTimeline.text, progress)
        : "neutral";
      const mouthCue = activeTimeline
        ? activeTimeline.alignedWords
          ? mouthCueAtWordTimings(activeTimeline.alignedWords, now - (activeTimeline.startAt ?? now))
          : mouthCueAtTextProgress(activeTimeline.text, progress)
        : "sil";
      const elapsed = activeTimeline?.startAt === null || !activeTimeline ? 0 : now - activeTimeline.startAt;
      this.onMouthMotion?.(this.mouthLevel, this.mouthRoundness, viseme, mouthCue, this.faceWeightsAt(activeTimeline, elapsed));
      this.mouthFrame = window.requestAnimationFrame(tick);
    };
    this.mouthFrame = window.requestAnimationFrame(tick);
  }
}
