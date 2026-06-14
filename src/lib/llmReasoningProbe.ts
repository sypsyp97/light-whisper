import type { ApiFormat, CustomProvider } from "@/types";

export interface LlmReasoningProbeProviderDefaults {
  baseUrl: string;
  model: string;
  apiFormat?: ApiFormat;
}

export interface LlmReasoningProbeTarget {
  provider: string;
  baseUrl: string;
  model: string;
  apiFormat: ApiFormat;
}

export function resolveLlmReasoningProbeTarget({
  provider,
  customBaseUrl,
  customModel,
  customProviders,
  defaults,
}: {
  provider: string;
  customBaseUrl?: string | null;
  customModel?: string | null;
  customProviders: CustomProvider[];
  defaults: LlmReasoningProbeProviderDefaults;
}): LlmReasoningProbeTarget {
  const customProvider = customProviders.find((item) => item.id === provider);
  const baseUrl = customBaseUrl?.trim() || customProvider?.base_url || defaults.baseUrl;
  const model = customModel?.trim() || customProvider?.model.trim() || defaults.model;

  return {
    provider,
    baseUrl,
    model,
    apiFormat: customProvider?.api_format ?? defaults.apiFormat ?? "openai_compat",
  };
}

export function resolveAssistantLlmReasoningProbeTarget({
  assistantUseSeparateModel,
  polishTarget,
  llmProvider,
  effectiveAssistantProvider,
  assistantModel,
  customBaseUrl,
  customModel,
  customProviders,
  defaults,
}: {
  assistantUseSeparateModel: boolean;
  polishTarget: LlmReasoningProbeTarget;
  llmProvider: string;
  effectiveAssistantProvider: string;
  assistantModel?: string | null;
  customBaseUrl?: string | null;
  customModel?: string | null;
  customProviders: CustomProvider[];
  defaults: LlmReasoningProbeProviderDefaults;
}): LlmReasoningProbeTarget {
  if (!assistantUseSeparateModel) {
    return polishTarget;
  }

  const sharesPolishProvider = effectiveAssistantProvider === llmProvider;
  return resolveLlmReasoningProbeTarget({
    provider: effectiveAssistantProvider,
    customBaseUrl: sharesPolishProvider ? customBaseUrl : null,
    customModel: sharesPolishProvider
      ? assistantModel?.trim() || customModel
      : assistantModel,
    customProviders,
    defaults,
  });
}
