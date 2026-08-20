import type { XaiAuthMode } from "@/types";

export const GROK_BUILD_OAUTH_PREFIX = "grok-build-oauth:";
export const GROK_BUILD_INFERENCE_BASE_URL = "https://cli-chat-proxy.grok.com";
export const XAI_API_KEY_INFERENCE_BASE_URL = "https://api.x.ai";

export function effectiveXaiAuthMode({
  storedMode,
  loggedIn,
}: {
  storedMode?: XaiAuthMode | null;
  loggedIn: boolean;
}): XaiAuthMode {
  if (storedMode === "api_key" || storedMode === "oauth") {
    return storedMode;
  }
  return loggedIn ? "oauth" : "api_key";
}

export function shouldShowGrokBuildAuth(provider: string): boolean {
  return provider === "xai";
}

export function shouldUseGrokBuildOauth({
  provider,
  authMode,
  loggedIn,
}: {
  provider: string;
  authMode: XaiAuthMode;
  loggedIn: boolean;
}): boolean {
  return provider === "xai" && authMode === "oauth" && loggedIn;
}

export function encodeGrokBuildOauthAccessToken(accessToken: string): string | null {
  const token = accessToken.trim();
  if (!token) {
    return null;
  }
  return `${GROK_BUILD_OAUTH_PREFIX}${token}`;
}

export function decodeGrokBuildOauthAccessToken(input: string): string | null {
  const payload = input.trim().startsWith(GROK_BUILD_OAUTH_PREFIX)
    ? input.trim().slice(GROK_BUILD_OAUTH_PREFIX.length)
    : null;
  if (payload === null) {
    return null;
  }
  const token = payload.trim();
  return token ? token : null;
}

export function isGrokBuildOauthOriginAuth(input: string): boolean {
  return decodeGrokBuildOauthAccessToken(input) !== null;
}
