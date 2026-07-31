export type AsrEngineKey =
  | "qwen3-asr-0.6b"
  | "qwen3-asr-1.7b"
  | "glm-asr"
  | "alibaba-asr";

interface AsrEngineCapabilityBase {
  final: true;
  interim: boolean;
}

export type AsrEngineCapability = AsrEngineCapabilityBase & (
  | { execution: "local"; downloadSize: string }
  | { execution: "cloud"; downloadSize: null }
);

export const ASR_ENGINE_CAPABILITIES: Record<AsrEngineKey, AsrEngineCapability> = {
  "qwen3-asr-0.6b": { execution: "local", final: true, interim: true, downloadSize: "850 MB" },
  "qwen3-asr-1.7b": { execution: "local", final: true, interim: true, downloadSize: "2.19 GB" },
  "glm-asr": { execution: "cloud", final: true, interim: false, downloadSize: null },
  "alibaba-asr": { execution: "cloud", final: true, interim: false, downloadSize: null },
};

export function getAsrEngineCapability(engine: string): AsrEngineCapability | null {
  return Object.prototype.hasOwnProperty.call(ASR_ENGINE_CAPABILITIES, engine)
    ? ASR_ENGINE_CAPABILITIES[engine as AsrEngineKey]
    : null;
}

export function formatAsrEngineDescription(engine: string, description: string): string {
  const downloadSize = getAsrEngineCapability(engine)?.downloadSize;
  return downloadSize ? `${downloadSize} · ${description}` : description;
}
