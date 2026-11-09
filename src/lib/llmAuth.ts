import type { OpenaiAuthMode, XaiAuthMode } from "@/types";

export function hasLlmAuth({
  provider,
  apiKey,
  openaiAuthMode,
  openaiOauthReady,
  xaiAuthMode,
  grokOauthReady,
}: {
  provider: string;
  apiKey: string;
  openaiAuthMode: OpenaiAuthMode;
  openaiOauthReady: boolean;
  xaiAuthMode: XaiAuthMode;
  grokOauthReady: boolean;
}): boolean {
  if (provider === "openai" && openaiAuthMode === "oauth") return openaiOauthReady;
  if (provider === "xai" && xaiAuthMode === "oauth") return grokOauthReady;
  return Boolean(apiKey.trim());
}
