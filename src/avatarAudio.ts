export function maxWindowedRms(chunks: readonly Float32Array[], sampleRate: number, windowMs = 200) {
  if (!Number.isFinite(sampleRate) || sampleRate <= 0 || !Number.isFinite(windowMs) || windowMs <= 0) return 0;
  const windowSamples = Math.max(1, Math.round(sampleRate * windowMs / 1000));
  let sumSquares = 0;
  let count = 0;
  let maximum = 0;
  for (const chunk of chunks) {
    for (const value of chunk) {
      const sample = Number.isFinite(value) ? value : 0;
      sumSquares += sample * sample;
      count += 1;
      if (count === windowSamples) {
        maximum = Math.max(maximum, Math.sqrt(sumSquares / count));
        sumSquares = 0;
        count = 0;
      }
    }
  }
  if (count > 0) maximum = Math.max(maximum, Math.sqrt(sumSquares / count));
  return maximum;
}

export function encodeMonoPcm16Wav(
  chunks: readonly Float32Array[],
  sourceSampleRate: number,
  targetSampleRate = 16_000,
) {
  if (!Number.isFinite(sourceSampleRate) || sourceSampleRate <= 0) {
    throw new RangeError("The microphone sample rate is invalid.");
  }
  if (!Number.isFinite(targetSampleRate) || targetSampleRate <= 0) {
    throw new RangeError("The transcription sample rate is invalid.");
  }

  const sourceLength = chunks.reduce((length, chunk) => length + chunk.length, 0);
  if (sourceLength === 0) throw new Error("No microphone audio was captured.");

  const source = new Float32Array(sourceLength);
  let offset = 0;
  for (const chunk of chunks) {
    source.set(chunk, offset);
    offset += chunk.length;
  }

  const outputLength = Math.max(1, Math.floor(sourceLength * targetSampleRate / sourceSampleRate));
  const wav = new ArrayBuffer(44 + outputLength * 2);
  const view = new DataView(wav);
  const writeTag = (position: number, value: string) => {
    for (let index = 0; index < value.length; index += 1) view.setUint8(position + index, value.charCodeAt(index));
  };

  writeTag(0, "RIFF");
  view.setUint32(4, 36 + outputLength * 2, true);
  writeTag(8, "WAVE");
  writeTag(12, "fmt ");
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, targetSampleRate, true);
  view.setUint32(28, targetSampleRate * 2, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  writeTag(36, "data");
  view.setUint32(40, outputLength * 2, true);

  for (let index = 0; index < outputLength; index += 1) {
    const sourcePosition = index * sourceSampleRate / targetSampleRate;
    const leftIndex = Math.min(sourceLength - 1, Math.floor(sourcePosition));
    const rightIndex = Math.min(sourceLength - 1, leftIndex + 1);
    const fraction = sourcePosition - leftIndex;
    const sample = source[leftIndex] + (source[rightIndex] - source[leftIndex]) * fraction;
    const clamped = Math.max(-1, Math.min(1, sample));
    view.setInt16(44 + index * 2, clamped < 0 ? clamped * 32768 : clamped * 32767, true);
  }

  return new Blob([wav], { type: "audio/wav" });
}
