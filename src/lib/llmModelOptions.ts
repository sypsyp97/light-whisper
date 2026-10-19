import type { AiModelInfo, LlmReasoningMode } from "@/types";

export function resolveModelId(value: string, models: readonly AiModelInfo[]): string {
  const normalized = value.trim();
  const keyword = normalized.toLowerCase();
  return models.find((model) => model.id.toLowerCase() === keyword)?.id
    ?? models.find((model) => model.displayName?.trim().toLowerCase() === keyword)?.id
    ?? normalized;
}

export interface LlmProviderOption {
  key: string;
  label: string;
  descKey: string;
  baseUrl: string;
  defaultModel: string;
  models: readonly string[];
}

export const llmProviderOptions: ReadonlyArray<LlmProviderOption> = [
  {
    key: "openai",
    label: "OpenAI",
    descKey: "settings.openaiDesc",
    baseUrl: "https://api.openai.com",
    defaultModel: "gpt-6-luna",
    models: [],
  },
  {
    key: "xai",
    label: "xAI Grok",
    descKey: "settings.xaiDesc",
    baseUrl: "https://api.x.ai",
    defaultModel: "grok-4.7",
    models: ["grok-4.7", "grok-4.6", "grok-4.5", "grok-build-0.1", "grok-composer-2.5-fast"],
  },
  {
    key: "deepseek",
    label: "DeepSeek",
    descKey: "settings.deepseekDesc",
    baseUrl: "https://api.deepseek.com",
    defaultModel: "deepseek-flash",
    models: ["deepseek-flash", "deepseek-v4-flash", "deepseek-v4-pro"],
  },
  {
    key: "cerebras",
    label: "Cerebras",
    descKey: "settings.cerebrasDesc",
    baseUrl: "https://api.cerebras.ai",
    defaultModel: "qwen-3.8-27b",
    models: ["qwen-3.8-27b", "gpt-oss-120b"],
  },
  {
    key: "siliconflow",
    label: "SiliconFlow",
    descKey: "settings.siliconflowDesc",
    baseUrl: "https://api.siliconflow.cn",
    defaultModel: "Qwen/Qwen3.8-27B",
    models: ["Qwen/Qwen3.8-27B", "deepseek-ai/DeepSeek-V4.1-Flash", "deepseek-ai/DeepSeek-V4-Pro"],
  },
  {
    key: "openrouter",
    label: "OpenRouter",
    descKey: "settings.openrouterDesc",
    baseUrl: "https://openrouter.ai/api",
    defaultModel: "~deepseek/deepseek-flash-latest",
    models: ["~deepseek/deepseek-flash-latest"],
  },
];

export const reasoningModeOptions: ReadonlyArray<{
  key: LlmReasoningMode;
  labelKey: string;
  descKey: string;
}> = [
  { key: "provider_default", labelKey: "settings.reasoningDefault", descKey: "settings.reasoningDefaultDesc" },
  { key: "off", labelKey: "settings.reasoningOff", descKey: "settings.reasoningOffDesc" },
  { key: "light", labelKey: "settings.reasoningLight", descKey: "settings.reasoningLightDesc" },
  { key: "balanced", labelKey: "settings.reasoningBalanced", descKey: "settings.reasoningBalancedDesc" },
  { key: "deep", labelKey: "settings.reasoningDeep", descKey: "settings.reasoningDeepDesc" },
];

export function findLlmPreset(key: string): LlmProviderOption {
  return llmProviderOptions.find((option) => option.key === key) ?? llmProviderOptions[0];
}

export function isFixedPresetProvider(key: string): boolean {
  return llmProviderOptions.some((option) => option.key === key);
}

export function resolveLlmBaseUrl(key: string, customBaseUrl?: string | null): string {
  const preset = findLlmPreset(key);
  return isFixedPresetProvider(key)
    ? preset.baseUrl
    : customBaseUrl?.trim() || preset.baseUrl;
}

export function resolveLlmModel(key: string, customModel?: string | null): string {
  const preset = findLlmPreset(key);
  return customModel?.trim() || preset.defaultModel;
}

export function findReasoningModeOption(mode: LlmReasoningMode) {
  return reasoningModeOptions.find((option) => option.key === mode) ?? reasoningModeOptions[0];
}
