import type { LlmReasoningMode } from "@/types";

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
    defaultModel: "gpt-4.1-mini",
    models: ["gpt-5.5", "gpt-4.1-mini", "gpt-4o-mini", "gpt-4.1"],
  },
  {
    key: "deepseek",
    label: "DeepSeek",
    descKey: "settings.deepseekDesc",
    baseUrl: "https://api.deepseek.com",
    defaultModel: "deepseek-v4-flash",
    models: ["deepseek-v4-flash", "deepseek-v4-pro", "deepseek-chat", "deepseek-reasoner"],
  },
  {
    key: "cerebras",
    label: "Cerebras",
    descKey: "settings.cerebrasDesc",
    baseUrl: "https://api.cerebras.ai",
    defaultModel: "gpt-oss-120b",
    models: ["gpt-oss-120b", "gpt-oss-20b"],
  },
  {
    key: "siliconflow",
    label: "SiliconFlow",
    descKey: "settings.siliconflowDesc",
    baseUrl: "https://api.siliconflow.cn",
    defaultModel: "Qwen/Qwen3-32B",
    models: ["Qwen/Qwen3-32B", "deepseek-ai/DeepSeek-V3", "Qwen/Qwen2.5-7B-Instruct"],
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
