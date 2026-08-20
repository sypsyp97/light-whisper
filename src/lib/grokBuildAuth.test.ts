import { describe, expect, it } from "vitest";

import type { XaiAuthMode } from "@/types";
import {
  GROK_BUILD_INFERENCE_BASE_URL,
  GROK_BUILD_OAUTH_PREFIX,
  XAI_API_KEY_INFERENCE_BASE_URL,
  decodeGrokBuildOauthAccessToken,
  effectiveXaiAuthMode,
  encodeGrokBuildOauthAccessToken,
  isGrokBuildOauthOriginAuth,
  shouldShowGrokBuildAuth,
  shouldUseGrokBuildOauth,
} from "./grokBuildAuth";

describe("GROK_BUILD_OAUTH_PREFIX and inference URLs", () => {
  it("uses the Grok Build origin-auth prefix", () => {
    expect(GROK_BUILD_OAUTH_PREFIX).toBe("grok-build-oauth:");
  });

  it("points OAuth inference at the Grok CLI proxy and API-key inference at api.x.ai", () => {
    expect(GROK_BUILD_INFERENCE_BASE_URL).toBe("https://cli-chat-proxy.grok.com");
    expect(XAI_API_KEY_INFERENCE_BASE_URL).toBe("https://api.x.ai");
  });
});

describe("effectiveXaiAuthMode", () => {
  it("returns the stored mode when the user has explicitly chosen api_key", () => {
    expect(effectiveXaiAuthMode({ storedMode: "api_key", loggedIn: true })).toBe("api_key");
    expect(effectiveXaiAuthMode({ storedMode: "api_key", loggedIn: false })).toBe("api_key");
  });

  it("returns the stored mode when the user has explicitly chosen oauth", () => {
    expect(effectiveXaiAuthMode({ storedMode: "oauth", loggedIn: true })).toBe("oauth");
    expect(effectiveXaiAuthMode({ storedMode: "oauth", loggedIn: false })).toBe("oauth");
  });

  it("defaults to oauth when no stored mode is set and a Grok Build session is logged in", () => {
    expect(effectiveXaiAuthMode({ storedMode: null, loggedIn: true })).toBe("oauth");
    expect(effectiveXaiAuthMode({ storedMode: undefined, loggedIn: true })).toBe("oauth");
  });

  it("defaults to api_key when no stored mode is set and the user is logged out", () => {
    expect(effectiveXaiAuthMode({ storedMode: null, loggedIn: false })).toBe("api_key");
    expect(effectiveXaiAuthMode({ storedMode: undefined, loggedIn: false })).toBe("api_key");
  });
});

describe("shouldShowGrokBuildAuth", () => {
  it("is true only for the xai provider", () => {
    expect(shouldShowGrokBuildAuth("xai")).toBe(true);
  });

  it("is false for OpenAI, other presets, custom providers, and casing variants", () => {
    expect(shouldShowGrokBuildAuth("openai")).toBe(false);
    expect(shouldShowGrokBuildAuth("deepseek")).toBe(false);
    expect(shouldShowGrokBuildAuth("cerebras")).toBe(false);
    expect(shouldShowGrokBuildAuth("custom-provider")).toBe(false);
    expect(shouldShowGrokBuildAuth("XAI")).toBe(false);
    expect(shouldShowGrokBuildAuth("Xai")).toBe(false);
    expect(shouldShowGrokBuildAuth("")).toBe(false);
  });
});

describe("shouldUseGrokBuildOauth", () => {
  it("is true only when the provider is xai, auth mode is oauth, and the user is logged in", () => {
    expect(shouldUseGrokBuildOauth({
      provider: "xai",
      authMode: "oauth",
      loggedIn: true,
    })).toBe(true);
  });

  it("is false when the provider is not xai even if oauth is selected and logged in", () => {
    expect(shouldUseGrokBuildOauth({
      provider: "openai",
      authMode: "oauth",
      loggedIn: true,
    })).toBe(false);
    expect(shouldUseGrokBuildOauth({
      provider: "deepseek",
      authMode: "oauth",
      loggedIn: true,
    })).toBe(false);
  });

  it("is false in api_key mode even when a Grok Build session is logged in", () => {
    expect(shouldUseGrokBuildOauth({
      provider: "xai",
      authMode: "api_key",
      loggedIn: true,
    })).toBe(false);
  });

  it("is false in oauth mode when the user is logged out", () => {
    expect(shouldUseGrokBuildOauth({
      provider: "xai",
      authMode: "oauth",
      loggedIn: false,
    })).toBe(false);
  });
});

describe("encodeGrokBuildOauthAccessToken", () => {
  it("prefixes a trimmed access token", () => {
    expect(encodeGrokBuildOauthAccessToken("access-token")).toBe(
      "grok-build-oauth:access-token",
    );
    expect(encodeGrokBuildOauthAccessToken("  access-token  ")).toBe(
      "grok-build-oauth:access-token",
    );
  });

  it("returns null for empty or whitespace-only tokens", () => {
    expect(encodeGrokBuildOauthAccessToken("")).toBeNull();
    expect(encodeGrokBuildOauthAccessToken("   ")).toBeNull();
    expect(encodeGrokBuildOauthAccessToken("\n\t")).toBeNull();
  });

  it("keeps colons inside the token payload", () => {
    expect(encodeGrokBuildOauthAccessToken("abc:def")).toBe("grok-build-oauth:abc:def");
  });
});

describe("decodeGrokBuildOauthAccessToken", () => {
  it("strips the Grok Build prefix from a valid origin-auth value", () => {
    expect(decodeGrokBuildOauthAccessToken("grok-build-oauth:access-token")).toBe(
      "access-token",
    );
  });

  it("trims the outer input before decoding", () => {
    expect(decodeGrokBuildOauthAccessToken("  grok-build-oauth:access-token  ")).toBe(
      "access-token",
    );
  });

  it("returns null when the prefix is missing or wrong", () => {
    expect(decodeGrokBuildOauthAccessToken("access-token")).toBeNull();
    expect(decodeGrokBuildOauthAccessToken("openai-codex-oauth-api-key:sk-test")).toBeNull();
    expect(decodeGrokBuildOauthAccessToken("openai-codex-chatgpt:token")).toBeNull();
    expect(decodeGrokBuildOauthAccessToken("not-grok-build-oauth:token")).toBeNull();
    expect(decodeGrokBuildOauthAccessToken("grok-build-oauth")).toBeNull();
  });

  it("returns null when the prefix is present but the payload is empty or whitespace", () => {
    expect(decodeGrokBuildOauthAccessToken("grok-build-oauth:")).toBeNull();
    expect(decodeGrokBuildOauthAccessToken("grok-build-oauth:   ")).toBeNull();
    expect(decodeGrokBuildOauthAccessToken("   grok-build-oauth:   ")).toBeNull();
  });

  it("round-trips a non-empty token through encode then decode", () => {
    const encoded = encodeGrokBuildOauthAccessToken("  runtime-token  ");
    expect(encoded).toBe("grok-build-oauth:runtime-token");
    expect(decodeGrokBuildOauthAccessToken(encoded as string)).toBe("runtime-token");
  });
});

describe("isGrokBuildOauthOriginAuth", () => {
  it("is true only for decodable Grok Build origin-auth values", () => {
    expect(isGrokBuildOauthOriginAuth("grok-build-oauth:access-token")).toBe(true);
    expect(isGrokBuildOauthOriginAuth("  grok-build-oauth:access-token  ")).toBe(true);
  });

  it("is false for empty, whitespace, API keys, OpenAI Codex prefixes, and empty payloads", () => {
    expect(isGrokBuildOauthOriginAuth("")).toBe(false);
    expect(isGrokBuildOauthOriginAuth("   ")).toBe(false);
    expect(isGrokBuildOauthOriginAuth("xai-plain-api-key")).toBe(false);
    expect(isGrokBuildOauthOriginAuth("openai-codex-oauth-api-key:sk-test")).toBe(false);
    expect(isGrokBuildOauthOriginAuth("grok-build-oauth:")).toBe(false);
    expect(isGrokBuildOauthOriginAuth("grok-build-oauth:   ")).toBe(false);
  });
});

describe("XaiAuthMode", () => {
  it("is the dual-auth union used by resolvers and IPC", () => {
    const apiKey: XaiAuthMode = "api_key";
    const oauth: XaiAuthMode = "oauth";
    expect(apiKey).toBe("api_key");
    expect(oauth).toBe("oauth");
  });
});
