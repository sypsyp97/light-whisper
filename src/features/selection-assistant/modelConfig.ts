import type { LlmProviderConfig, LlmReasoningMode } from "@/types";
import { isFixedPresetProvider } from "@/lib/llmModelOptions";

export function resolveSelectionModelConfig(config: LlmProviderConfig) {
  const polishReasoning = config.polish_reasoning_mode
    ?? config.reasoning_mode
    ?? "provider_default";
  const provider = config.selection_provider?.trim();
  const model = config.selection_model?.trim();
  const providerExists = Boolean(
    provider
    && (isFixedPresetProvider(provider) || provider === "custom"
      || config.custom_providers?.some((candidate) => candidate.id === provider)),
  );

  if (!config.selection_use_separate_model || !providerExists || !model) {
    return {
      provider: config.active,
      model: undefined,
      reasoningMode: polishReasoning as LlmReasoningMode,
      followsPolish: true,
    };
  }

  return {
    provider: provider!,
    model,
    reasoningMode: config.selection_reasoning_mode ?? polishReasoning,
    followsPolish: false,
  };
}
