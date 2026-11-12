import { describe, expect, it } from "vitest";
import { encodeMonoPcm16Wav, maxWindowedRms } from "./avatarAudio";

describe("maxWindowedRms", () => {
  it("rejects silence but detects short speech even when surrounded by pauses", () => {
    expect(maxWindowedRms([new Float32Array(2_000)], 1_000)).toBe(0);
    expect(maxWindowedRms([new Float32Array(1_000), new Float32Array(1_000).fill(0.02), new Float32Array(1_000)], 1_000)).toBeCloseTo(0.02);
  });

  it("treats invalid samples and sample rates safely", () => {
    expect(maxWindowedRms([new Float32Array([Number.NaN, Number.POSITIVE_INFINITY])], 16_000)).toBe(0);
    expect(maxWindowedRms([new Float32Array([0.5])], 0)).toBe(0);
  });
});

describe("encodeMonoPcm16Wav", () => {
  it("writes mono 16-bit WAV audio at the requested sample rate", async () => {
    const blob = encodeMonoPcm16Wav([new Float32Array([0, 0.5, -0.5, 1, -1])], 16_000);
    const bytes = await blob.arrayBuffer();
    const view = new DataView(bytes);

    expect(blob.type).toBe("audio/wav");
    expect(bytes.byteLength).toBe(54);
    expect(view.getUint32(24, true)).toBe(16_000);
    expect(view.getUint16(22, true)).toBe(1);
    expect(view.getUint16(34, true)).toBe(16);
    expect(view.getUint32(40, true)).toBe(10);
    expect(view.getInt16(46, true)).toBe(16_383);
    expect(view.getInt16(48, true)).toBe(-16_384);
  });

  it("resamples input without saving or retaining audio outside the returned blob", async () => {
    const wav = encodeMonoPcm16Wav([new Float32Array([0, 0.25, 0.5, 0.75])], 32_000, 16_000);
    const view = new DataView(await wav.arrayBuffer());

    expect(view.getUint32(24, true)).toBe(16_000);
    expect(view.getUint32(40, true)).toBe(4);
    expect(view.getInt16(44, true)).toBe(0);
    expect(view.getInt16(46, true)).toBe(16_383);
  });

  it("rejects an empty recording or invalid sample rate", () => {
    expect(() => encodeMonoPcm16Wav([], 48_000)).toThrow("No microphone audio");
    expect(() => encodeMonoPcm16Wav([new Float32Array([0])], 0)).toThrow("sample rate is invalid");
  });
});
