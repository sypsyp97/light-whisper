/**
 * Write a WAV file header into a DataView.
 * Produces a standard 16-bit mono PCM WAV at the given sample rate.
 */
function writeWavHeader(
  view: DataView,
  numSamples: number,
  sampleRate: number
): void {
  const numChannels = 1;
  const bitsPerSample = 16;
  const byteRate = sampleRate * numChannels * (bitsPerSample / 8);
  const blockAlign = numChannels * (bitsPerSample / 8);
  const dataSize = numSamples * numChannels * (bitsPerSample / 8);

  // "RIFF" chunk descriptor
  writeString(view, 0, "RIFF");
  view.setUint32(4, 36 + dataSize, true);
  writeString(view, 8, "WAVE");

  // "fmt " sub-chunk
  writeString(view, 12, "fmt ");
  view.setUint32(16, 16, true); // sub-chunk size (PCM = 16)
  view.setUint16(20, 1, true); // audio format (PCM = 1)
  view.setUint16(22, numChannels, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, byteRate, true);
  view.setUint16(32, blockAlign, true);
  view.setUint16(34, bitsPerSample, true);

  // "data" sub-chunk
  writeString(view, 36, "data");
  view.setUint32(40, dataSize, true);
}

function writeString(view: DataView, offset: number, str: string): void {
  for (let i = 0; i < str.length; i++) {
    view.setUint8(offset + i, str.charCodeAt(i));
  }
}

/**
 * Convert an AudioBuffer (any sample rate) to a 16-bit mono WAV
 * ArrayBuffer re-sampled to the target sample rate.
 */
function audioBufferToWav(buffer: AudioBuffer, targetSampleRate = 16000): ArrayBuffer {
  // Down-mix to mono (average all channels)
  const numChannels = buffer.numberOfChannels;
  let channelData: Float32Array;

  if (numChannels <= 1) {
    channelData = buffer.getChannelData(0);
  } else {
    const mixed = new Float32Array(buffer.length);
    for (let ch = 0; ch < numChannels; ch++) {
      const data = buffer.getChannelData(ch);
      for (let i = 0; i < data.length; i++) {
        mixed[i] += data[i];
      }
    }
    for (let i = 0; i < mixed.length; i++) {
      mixed[i] /= numChannels;
    }
    channelData = mixed;
  }
  const sourceSampleRate = buffer.sampleRate;

  // Resample if necessary
  let samples: Float32Array;
  if (sourceSampleRate === targetSampleRate) {
    samples = channelData;
  } else {
    const ratio = sourceSampleRate / targetSampleRate;
    const newLength = Math.round(channelData.length / ratio);
    samples = new Float32Array(newLength);
    for (let i = 0; i < newLength; i++) {
      const srcIndex = i * ratio;
      const low = Math.floor(srcIndex);
      const high = Math.min(low + 1, channelData.length - 1);
      const frac = srcIndex - low;
      samples[i] = channelData[low] * (1 - frac) + channelData[high] * frac;
    }
  }

  const numSamples = samples.length;
  const headerSize = 44;
  const wavBuffer = new ArrayBuffer(headerSize + numSamples * 2);
  const view = new DataView(wavBuffer);

  writeWavHeader(view, numSamples, targetSampleRate);

  // Write PCM samples (clamp to int16 range)
  let offset = headerSize;
  for (let i = 0; i < numSamples; i++) {
    const s = Math.max(-1, Math.min(1, samples[i]));
    const val = s < 0 ? s * 0x8000 : s * 0x7fff;
    view.setInt16(offset, val, true);
    offset += 2;
  }

  return wavBuffer;
}

/**
 * Convert a WebM Blob captured by MediaRecorder into a WAV ArrayBuffer
 * by decoding through the Web Audio API and re-encoding as 16 kHz mono PCM.
 */
export async function convertToWav(blob: Blob): Promise<ArrayBuffer> {
  const arrayBuffer = await blob.arrayBuffer();
  let audioCtx: AudioContext;
  try {
    audioCtx = new AudioContext({ sampleRate: 16000 });
  } catch {
    // Fallback to default sample rate if 16k is unsupported
    audioCtx = new AudioContext();
  }

  try {
    const audioBuffer = await audioCtx.decodeAudioData(arrayBuffer);
    return audioBufferToWav(audioBuffer, 16000);
  } finally {
    await audioCtx.close();
  }
}
