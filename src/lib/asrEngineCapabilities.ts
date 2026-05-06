export type AsrEngineKey = "sensevoice" | "whisper" | "glm-asr" | "alibaba-asr";

export interface AsrEngineCapability {
  execution: "local" | "cloud";
  final: true;
  interim: boolean;
}

export const ASR_ENGINE_CAPABILITIES: Record<AsrEngineKey, AsrEngineCapability> = {
  sensevoice: { execution: "local", final: true, interim: true },
  whisper: { execution: "local", final: true, interim: true },
  "glm-asr": { execution: "cloud", final: true, interim: false },
  "alibaba-asr": { execution: "cloud", final: true, interim: false },
};

export function getAsrEngineCapability(engine: string): AsrEngineCapability | null {
  return Object.prototype.hasOwnProperty.call(ASR_ENGINE_CAPABILITIES, engine)
    ? ASR_ENGINE_CAPABILITIES[engine as AsrEngineKey]
    : null;
}
