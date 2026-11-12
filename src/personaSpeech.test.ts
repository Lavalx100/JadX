import { afterEach, describe, expect, it, vi } from "vitest";
import { forcedAlignerLanguageForText, mouthLevelFromRms, mouthMotionForAudio, mouthRoundnessFromSpectrum, parsePersonaWordTimings, PersonaSpeechPlayer, PERSONA_TTS_STREAMING_INTERVAL_SECONDS, visemeAtTextProgress, visemeAtWordTimings } from "./personaSpeech";

describe("mouthLevelFromRms", () => {
  it("keeps silence closed", () => {
    expect(mouthLevelFromRms(0)).toBe(0);
    expect(mouthLevelFromRms(0.0025)).toBe(0);
  });

  it("opens visibly across normal speech levels and clamps loud peaks", () => {
    expect(mouthLevelFromRms(0.03)).toBeGreaterThan(0.5);
    expect(mouthLevelFromRms(0.06)).toBe(1);
    expect(mouthLevelFromRms(0.5)).toBe(1);
  });

  it("treats invalid energy as silence", () => {
    expect(mouthLevelFromRms(Number.NaN)).toBe(0);
    expect(mouthLevelFromRms(Number.POSITIVE_INFINITY)).toBe(0);
  });
});

describe("mouthRoundnessFromSpectrum", () => {
  it("moves between wider and rounder shapes using local speech frequencies", () => {
    const wide = new Float32Array(256).fill(-35);
    const round = new Float32Array(256).fill(-35);
    const binHz = 8_000 / 512;
    for (let index = 0; index < wide.length; index += 1) {
      const frequency = index * binHz;
      if (frequency >= 300 && frequency < 1_200) wide[index] = -45;
      if (frequency >= 1_200 && frequency <= 3_200) wide[index] = -5;
      if (frequency >= 300 && frequency < 1_200) round[index] = -5;
      if (frequency >= 1_200 && frequency <= 3_200) round[index] = -45;
    }
    expect(mouthRoundnessFromSpectrum(wide, 8_000, 512)).toBe(0);
    expect(mouthRoundnessFromSpectrum(round, 8_000, 512)).toBe(1);
    expect(mouthRoundnessFromSpectrum(new Float32Array(256).fill(Number.NEGATIVE_INFINITY), 8_000, 512)).toBe(0.45);
  });
});

describe("mouthMotionForAudio", () => {
  it("keeps the mouth closed at rest and varies width gently for rounded sounds", () => {
    expect(mouthMotionForAudio(0, 0.45).opacity).toBe(0);
    expect(mouthMotionForAudio(0, 0.45).scaleY).toBe(0);
    expect(mouthMotionForAudio(1, 0).scaleY).toBe(1.05);
    expect(mouthMotionForAudio(1, 1).scaleX).toBeCloseTo(0.88);
    expect(mouthMotionForAudio(Number.NaN, Number.NaN).opacity).toBe(0);
  });
});

describe("visemeAtTextProgress", () => {
  it("changes mouth cues with spoken vowels and lip-closure consonants", () => {
    expect(visemeAtTextProgress("ah", 0)).toBe("open");
    expect(visemeAtTextProgress("ee", 0.5)).toBe("wide");
    expect(visemeAtTextProgress("oo", 0.5)).toBe("round");
    expect(visemeAtTextProgress("mom", 0)).toBe("closed");
  });

  it("recognizes common Arabic long vowels and stays neutral for punctuation", () => {
    expect(visemeAtTextProgress("ا", 0)).toBe("open");
    expect(visemeAtTextProgress("ي", 0)).toBe("wide");
    expect(visemeAtTextProgress("و", 0)).toBe("round");
    expect(visemeAtTextProgress("؟", 0)).toBe("neutral");
  });
});

describe("forced-aligned viseme timing", () => {
  it("parses and bounds word timestamps from MLX-Audio aligner output", () => {
    expect(parsePersonaWordTimings({ items: [
      { text: "later", start_time: 0.5, end_time: 1.4 },
      { text: "first", start_time: -0.1, end_time: 0.4 },
      { text: "broken", start_time: 0.9, end_time: 0.7 },
    ] }, 1)).toEqual([
      { text: "first", start: 0, end: 0.4 },
      { text: "later", start: 0.5, end: 1 },
    ]);
    expect(parsePersonaWordTimings({ segments: [{ words: [{ word: "hello", start: 0.1, end: 0.3 }] }] }, 1)).toEqual([
      { text: "hello", start: 0.1, end: 0.3 },
    ]);
  });

  it("uses the aligned word span for its mouth cue and leaves pauses neutral", () => {
    const timings = [
      { text: "mom", start: 0.1, end: 0.4 },
      { text: "apple", start: 0.6, end: 1.1 },
    ];
    expect(visemeAtWordTimings(timings, 0.1)).toBe("closed");
    expect(visemeAtWordTimings(timings, 0.62)).toBe("open");
    expect(visemeAtWordTimings(timings, 0.5)).toBe("neutral");
  });

  it("avoids forced alignment for unsupported Arabic text", () => {
    expect(forcedAlignerLanguageForText("Hello there.")).toBe("English");
    expect(forcedAlignerLanguageForText("مرحبا")).toBeNull();
  });
});

describe("PersonaSpeechPlayer audio-reactive mouth", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("opens from streamed playback audio and closes when playback ends", async () => {
    const frameCallbacks = new Map<number, FrameRequestCallback>();
    const sources: Array<{ onended: (() => void) | null; buffer: { duration: number } | null }> = [];
    const motion: number[] = [];
    const speaking: boolean[] = [];
    let nextFrame = 1;

    vi.stubGlobal("window", {
      requestAnimationFrame: (callback: FrameRequestCallback) => {
        const id = nextFrame++;
        frameCallbacks.set(id, callback);
        return id;
      },
      cancelAnimationFrame: (id: number) => frameCallbacks.delete(id),
    });

    class FakeAudioContext {
      state = "running";
      currentTime = 0;
      destination = {};
      close = async () => { this.state = "closed"; };
      resume = async () => {};
      createAnalyser() {
        const fftSize = 512;
        return {
          fftSize,
          frequencyBinCount: fftSize / 2,
          smoothingTimeConstant: 0,
          connect: () => {},
          getFloatTimeDomainData: (samples: Float32Array) => samples.fill(0.22),
          getFloatFrequencyData: (samples: Float32Array) => {
            samples.fill(-35);
            for (let index = 0; index < samples.length; index += 1) {
              const frequency = index * sampleRate / fftSize;
              if (frequency >= 300 && frequency < 1_200) samples[index] = -5;
              if (frequency >= 1_200 && frequency <= 3_200) samples[index] = -45;
            }
          },
        };
      }
      createBuffer(_channels: number, length: number, sampleRate: number) {
        const samples = new Float32Array(length);
        return { duration: length / sampleRate, getChannelData: () => samples };
      }
      createBufferSource() {
        const source: { buffer: { duration: number } | null; onended: (() => void) | null; connect: () => void; disconnect: () => void; start: () => void; stop: () => void } = {
          buffer: null,
          onended: null as (() => void) | null,
          connect: () => {},
          disconnect: () => {},
          start: () => {},
          stop: () => {},
        };
        sources.push(source);
        return source;
      }
    }
    vi.stubGlobal("AudioContext", FakeAudioContext);

    const sampleRate = 8_000;
    const pcm = new Uint8Array((sampleRate * PERSONA_TTS_STREAMING_INTERVAL_SECONDS + 80) * 2);
    const view = new DataView(pcm.buffer);
    for (let offset = 0; offset < pcm.byteLength; offset += 2) view.setInt16(offset, 6_000, true);
    const fetchMock = vi.fn(async (_url: RequestInfo | URL, _options?: RequestInit) => new Response(pcm, { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    const player = new PersonaSpeechPlayer();
    player.onMouthMotion = (level) => motion.push(level);
    player.onPlaybackState = (value) => speaking.push(value);
    await player.enable({ base_url: "http://127.0.0.1:8008", model: "local-test", ref_audio: "local-only.wav", ref_text: "local transcript", sample_rate: sampleRate });
    player.acceptDelta("This is a local speech test.");
    player.finishTurn();
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(sources.length).toBe(2);
    expect(sources.reduce((duration, source) => duration + (source.buffer?.duration ?? 0), 0)).toBeCloseTo(pcm.byteLength / 2 / sampleRate);
    const requestBody = JSON.parse(String(fetchMock.mock.calls[0]?.[1]?.body));
    expect(requestBody.streaming_interval).toBe(PERSONA_TTS_STREAMING_INTERVAL_SECONDS);
    expect(speaking).toContain(true);
    for (const timestamp of [16, 32, 48, 64]) {
      const frame = frameCallbacks.values().next().value as FrameRequestCallback | undefined;
      frame?.(timestamp);
    }
    expect(motion.at(-1) ?? 0).toBeGreaterThan(0.5);

    for (const source of sources) source.onended?.();
    expect(motion.at(-1)).toBe(0);
    expect(speaking.at(-1)).toBe(false);
    await player.disable();
  });

  it("sends short generated speech to the optional local word aligner", async () => {
    const frameCallbacks = new Map<number, FrameRequestCallback>();
    let nextFrame = 1;
    vi.stubGlobal("window", {
      requestAnimationFrame: (callback: FrameRequestCallback) => {
        const id = nextFrame++;
        frameCallbacks.set(id, callback);
        return id;
      },
      cancelAnimationFrame: (id: number) => frameCallbacks.delete(id),
      setTimeout,
      clearTimeout,
    });
    class FakeAudioContext {
      state = "running";
      currentTime = 0;
      destination = {};
      close = async () => { this.state = "closed"; };
      resume = async () => {};
      createAnalyser() {
        return { fftSize: 32, frequencyBinCount: 16, smoothingTimeConstant: 0, connect: () => {}, getFloatTimeDomainData: (samples: Float32Array) => samples.fill(0), getFloatFrequencyData: (samples: Float32Array) => samples.fill(-80) };
      }
      createBuffer(_channels: number, length: number, sampleRate: number) {
        const samples = new Float32Array(length);
        return { duration: length / sampleRate, getChannelData: () => samples };
      }
      createBufferSource() { return { buffer: null as { duration: number } | null, onended: null as (() => void) | null, connect: () => {}, disconnect: () => {}, start: () => {}, stop: () => {} }; }
    }
    vi.stubGlobal("AudioContext", FakeAudioContext);

    const sampleRate = 8_000;
    const pcm = new Uint8Array((sampleRate * PERSONA_TTS_STREAMING_INTERVAL_SECONDS + 80) * 2);
    const fetchMock = vi.fn(async (url: RequestInfo | URL, options?: RequestInit) => {
      if (String(url).endsWith("/v1/audio/speech")) return new Response(pcm, { status: 200 });
      const requestBody = options?.body as FormData;
      if (requestBody.get("model") === "mlx-community/Qwen3-ForcedAligner-0.6B-8bit") {
        return new Response(`${JSON.stringify({ items: [{ text: "The", start_time: 0.01, end_time: 0.12 }, { text: "word", start_time: 0.14, end_time: 0.28 }] })}\n`, { status: 200 });
      }
      return new Response(JSON.stringify({ text: "The spoken line.", segments: [{ words: [{ word: "The", start: 0.01, end: 0.12 }, { word: "spoken", start: 0.14, end: 0.25 }] }] }), { status: 200 });
    });
    vi.stubGlobal("fetch", fetchMock);

    const player = new PersonaSpeechPlayer();
    await player.enable({ base_url: "http://127.0.0.1:8008", model: "local-test", alignment_model: "mlx-community/Qwen3-ForcedAligner-0.6B-8bit", ref_audio: "local-only.wav", ref_text: "local transcript", sample_rate: sampleRate });
    player.beginTurn();
    player.acceptDelta("The word-alignment path is active. ");
    player.finishTurn();
    await new Promise((resolve) => setTimeout(resolve, 0));

    const alignmentCall = fetchMock.mock.calls.find(([url]) => String(url).endsWith("/v1/audio/transcriptions"));
    expect(alignmentCall).toBeDefined();
    const body = alignmentCall?.[1]?.body as FormData;
    expect(body.get("model")).toBe("mlx-community/Qwen3-ForcedAligner-0.6B-8bit");
    expect(body.get("text")).toBe("The word-alignment path is active.");
    expect(body.get("language")).toBe("English");
    expect((body.get("file") as File).size).toBe(pcm.byteLength + 44);
    await player.disable();

    const whisperFallback = new PersonaSpeechPlayer();
    await whisperFallback.enable({ base_url: "http://127.0.0.1:8008", model: "local-test", word_timing_model: "mlx-community/whisper-large-v3-turbo-asr-fp16", ref_audio: "local-only.wav", ref_text: "local transcript", sample_rate: sampleRate });
    whisperFallback.beginTurn();
    whisperFallback.acceptDelta("The word-alignment path is active. ");
    whisperFallback.finishTurn();
    await new Promise((resolve) => setTimeout(resolve, 0));
    const transcriptionCalls = fetchMock.mock.calls.filter(([url]) => String(url).endsWith("/v1/audio/transcriptions"));
    expect(transcriptionCalls).toHaveLength(2);
    const whisperBody = transcriptionCalls[1]?.[1]?.body as FormData;
    expect(whisperBody.get("model")).toBe("mlx-community/whisper-large-v3-turbo-asr-fp16");
    expect(whisperBody.get("word_timestamps")).toBe("true");
    expect(whisperBody.get("response_format")).toBe("verbose_json");
    await whisperFallback.disable();
  });

  it("stops current speech and ignores later deltas until the next turn", async () => {
    class FakeAudioContext {
      state = "running";
      currentTime = 0;
      destination = {};
      close = async () => { this.state = "closed"; };
      resume = async () => {};
      createAnalyser() { return { fftSize: 32, frequencyBinCount: 16, connect: () => {} }; }
    }
    vi.stubGlobal("AudioContext", FakeAudioContext);
    const fetchMock = vi.fn((_url: RequestInfo | URL, options?: RequestInit) => new Promise<Response>((_resolve, reject) => {
      options?.signal?.addEventListener("abort", () => reject(new DOMException("Aborted", "AbortError")), { once: true });
    }));
    vi.stubGlobal("fetch", fetchMock);

    const player = new PersonaSpeechPlayer();
    await player.enable({ base_url: "http://127.0.0.1:8008", model: "local-test", ref_audio: "local-only.wav", ref_text: "local transcript", sample_rate: 8_000 });
    player.beginTurn();
    player.acceptDelta("This first sentence is long enough. ");
    expect(fetchMock).toHaveBeenCalledTimes(1);
    player.stopCurrentTurnSpeech();
    player.acceptDelta("This later sentence should stay silent. ");
    player.finishTurn();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(fetchMock).toHaveBeenCalledTimes(1);

    player.beginTurn();
    player.acceptDelta("A fresh turn should speak again. ");
    expect(fetchMock).toHaveBeenCalledTimes(2);
    player.stopCurrentTurnSpeech();
    await player.disable();
  });
});
