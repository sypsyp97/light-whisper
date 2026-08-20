import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

import en from "@/i18n/en";
import zh from "@/i18n/zh";

function readRepo(relativePath: string): string {
  return readFileSync(resolve(process.cwd(), relativePath), "utf8");
}

const settingsPage = readFileSync(resolve(process.cwd(), "src/pages/SettingsPage.tsx"), "utf8");
const typesSource = readFileSync(resolve(process.cwd(), "src/types/index.ts"), "utf8");
const tauriSource = readFileSync(resolve(process.cwd(), "src/api/tauri.ts"), "utf8");
const userProfileSource = readFileSync(
  resolve(process.cwd(), "src-tauri/src/state/user_profile.rs"),
  "utf8",
);
const llmProviderSource = readFileSync(
  resolve(process.cwd(), "src-tauri/src/services/llm_provider.rs"),
  "utf8",
);
const llmClientSource = readFileSync(
  resolve(process.cwd(), "src-tauri/src/services/llm_client.rs"),
  "utf8",
);
const codexOauthSource = readFileSync(
  resolve(process.cwd(), "src-tauri/src/services/codex_oauth_service.rs"),
  "utf8",
);
const commandsModSource = readFileSync(
  resolve(process.cwd(), "src-tauri/src/commands/mod.rs"),
  "utf8",
);
const libSource = readFileSync(resolve(process.cwd(), "src-tauri/src/lib.rs"), "utf8");
const aiPolishSource = readFileSync(
  resolve(process.cwd(), "src-tauri/src/commands/ai_polish.rs"),
  "utf8",
);
const profileCommandSource = readFileSync(
  resolve(process.cwd(), "src-tauri/src/commands/profile.rs"),
  "utf8",
);
const appStateSource = readFileSync(
  resolve(process.cwd(), "src-tauri/src/state/app_state.rs"),
  "utf8",
);
const servicesModSource = readFileSync(
  resolve(process.cwd(), "src-tauri/src/services/mod.rs"),
  "utf8",
);

const GROK_I18N_KEYS = [
  "xaiDesc",
  "xaiAuthModeLabel",
  "xaiAuthModeApiKey",
  "xaiAuthModeOauth",
  "xaiAuthModeApiKeyHint",
  "xaiAuthModeOauthHint",
  "grokBuildOauthLabel",
  "grokBuildOauthHint",
  "grokBuildOauthConnectedHint",
  "grokBuildOauthLogin",
  "grokBuildOauthReauth",
  "grokBuildOauthLogout",
  "grokBuildOauthWorking",
  "grokBuildOauthDeviceCodeLogin",
  "grokBuildOauthDeviceCodeReady",
  "grokBuildOauthDeviceCodeContinue",
  "fillApiKeyOrGrokLogin",
  "apiKeyOrGrokLoginMissing",
] as const;

function sliceBetween(source: string, startMarker: string, endMarker: string): string {
  const start = source.indexOf(startMarker);
  expect(start).toBeGreaterThanOrEqual(0);
  const end = source.indexOf(endMarker, start + startMarker.length);
  expect(end).toBeGreaterThan(start);
  return source.slice(start, end);
}

describe("SettingsPage Grok Build dual-auth contract", () => {
  it("imports Grok Build helpers and IPC wrappers", () => {
    expect(settingsPage).toContain('from "@/lib/grokBuildAuth"');
    expect(settingsPage).toContain("effectiveXaiAuthMode");
    expect(settingsPage).toContain("shouldShowGrokBuildAuth");
    expect(settingsPage).toContain("shouldUseGrokBuildOauth");
    expect(settingsPage).toContain("getGrokBuildOauthStatus");
    expect(settingsPage).toContain("loginGrokBuildOauth");
    expect(settingsPage).toContain("startGrokBuildOauthDeviceCode");
    expect(settingsPage).toContain("completeGrokBuildOauthDeviceCode");
    expect(settingsPage).toContain("logoutGrokBuildOauth");
  });

  it("places the xAI auth-mode toggle above the polish API key and the OAuth block below it", () => {
    const polishAuthSlice = sliceBetween(
      settingsPage,
      't("settings.baseUrlFixedHint")',
      't("settings.modelLabel")',
    );

    expect(polishAuthSlice).toContain("shouldShowGrokBuildAuth(llmProvider)");
    expect(polishAuthSlice).toContain("renderXaiAuthModeToggle");
    expect(polishAuthSlice).toContain("renderOpenaiAuthModeToggle");
    expect(polishAuthSlice).toContain("SecretInput");
    expect(polishAuthSlice).toContain('renderGrokBuildOauthBlock("polish")');
    expect(polishAuthSlice).toContain('renderOpenaiCodexOauthBlock("polish")');

    expect(polishAuthSlice.indexOf("renderXaiAuthModeToggle")).toBeLessThan(
      polishAuthSlice.indexOf('t("settings.apiKey")'),
    );
    expect(polishAuthSlice.indexOf('t("settings.apiKey")')).toBeLessThan(
      polishAuthSlice.indexOf('renderGrokBuildOauthBlock("polish")'),
    );
  });

  it("places the xAI auth-mode toggle above the assistant API key when the assistant provider differs", () => {
    const assistantKeySlice = sliceBetween(settingsPage, "助手独立 API Key", "助手模型选择器");

    expect(assistantKeySlice).toContain("shouldShowGrokBuildAuth(effectiveAssistantProvider)");
    expect(assistantKeySlice).toContain("renderXaiAuthModeToggle");
    expect(assistantKeySlice.indexOf("renderXaiAuthModeToggle")).toBeLessThan(
      assistantKeySlice.indexOf("SecretInput"),
    );
  });

  it("renders a Grok Build OAuth block in the assistant provider picker path", () => {
    expect(settingsPage).toContain('renderGrokBuildOauthBlock("assistant")');
  });

  it("gates Grok UI with shouldShowGrokBuildAuth instead of showing it for OpenAI", () => {
    expect(settingsPage).toMatch(
      /shouldShowGrokBuildAuth\(llmProvider\)[\s\S]{0,200}renderXaiAuthModeToggle/,
    );
    expect(settingsPage).toMatch(
      /llmProvider === "openai"[\s\S]{0,80}renderOpenaiAuthModeToggle/,
    );
    expect(settingsPage).not.toMatch(
      /shouldShowGrokBuildAuth\("openai"\)/,
    );
  });

  it("treats oauth plus a logged-in Grok session as credentials even when the API key field is empty", () => {
    expect(settingsPage).toContain("shouldUseGrokBuildOauth");
    expect(settingsPage).toContain("effectiveXaiAuthMode");
    expect(settingsPage).toMatch(/polishHasAuth[\s\S]{0,1200}(shouldUseGrokBuildOauth|effectiveXaiAuthMode)/);
    expect(settingsPage).toContain('t("settings.fillApiKeyOrGrokLogin")');
    expect(settingsPage).toContain('t("settings.apiKeyOrGrokLoginMissing")');
  });

  it("persists xaiAuthMode through llmConfigSave / setLlmProviderConfig", () => {
    const saveCallback = sliceBetween(
      settingsPage,
      "const llmConfigSave = useDebouncedCallback",
      "computeOnlineAsrKeyringUser",
    );
    expect(saveCallback).toContain("setLlmProviderConfig");
    expect(saveCallback).toContain("openaiAuthMode");
    expect(saveCallback).toContain("xaiAuthMode");
    expect(settingsPage).toContain("handleXaiAuthModeChange");
    expect(settingsPage).toMatch(/p\.llm_provider\.xai_auth_mode/);
  });

  it("refreshes Grok Build status on settings load and refreshes models after login", () => {
    expect(settingsPage).toContain("refreshGrokBuildOauthStatus");
    expect(settingsPage).toContain("getGrokBuildOauthStatus");
    expect(settingsPage).toMatch(
      /refreshProfile\(\)\.then\([\s\S]*refreshGrokBuildOauthStatus/,
    );
    expect(settingsPage).toContain("finalizeGrokBuildOauthLogin");
    expect(settingsPage).toMatch(
      /finalizeGrokBuildOauthLogin[\s\S]*refreshAiModels\(true\)/,
    );
  });

  it("forwards xaiAuthMode when fetching models for the xai provider", () => {
    expect(settingsPage).toMatch(
      /listAiModels\([\s\S]*llmProvider === "xai" \? effectiveXaiAuthMode/,
    );
    expect(settingsPage).toMatch(
      /listAiModels\([\s\S]*effectiveAssistantProvider === "xai" \? effectiveXaiAuthMode/,
    );
    expect(settingsPage).toMatch(
      /listAiModels\([\s\S]*llmProvider === "openai" \? effectiveOpenaiAuthMode/,
    );
  });

  it("exposes browser login, device-code login, reauth, logout, verification URL, user code, and continue", () => {
    const grokBlock = sliceBetween(
      settingsPage,
      "const renderGrokBuildOauthBlock",
      "const allProviderOptions",
    );
    expect(grokBlock).toContain('t("settings.grokBuildOauthLogin")');
    expect(grokBlock).toContain('t("settings.grokBuildOauthReauth")');
    expect(grokBlock).toContain('t("settings.grokBuildOauthLogout")');
    expect(grokBlock).toContain('t("settings.grokBuildOauthDeviceCodeLogin")');
    expect(grokBlock).toContain('t("settings.grokBuildOauthDeviceCodeContinue")');
    expect(grokBlock).toContain("verificationUrl");
    expect(grokBlock).toContain("userCode");
    expect(grokBlock).toContain("handleGrokBuildOauthLogin");
    expect(grokBlock).toContain("handleGrokBuildOauthDeviceCodeStart");
    expect(grokBlock).toContain("handleGrokBuildOauthDeviceCodeComplete");
    expect(grokBlock).toContain("handleGrokBuildOauthLogout");
  });

  it("keeps the existing OpenAI Codex OAuth controls", () => {
    expect(settingsPage).toContain("renderOpenaiAuthModeToggle");
    expect(settingsPage).toContain("renderOpenaiCodexOauthBlock");
    expect(settingsPage).toContain("getOpenaiCodexOauthStatus");
    expect(settingsPage).toContain("loginOpenaiCodexOauth");
  });
});

describe("Grok Build TypeScript IPC types", () => {
  it("declares XaiAuthMode and persists it on LlmProviderConfig", () => {
    expect(typesSource).toContain('export type XaiAuthMode = "api_key" | "oauth"');
    expect(typesSource).toContain("xai_auth_mode?: XaiAuthMode | null");
  });

  it("declares camelCase Grok Build status and device-code challenge types", () => {
    expect(typesSource).toContain("export interface GrokBuildOauthStatus");
    expect(typesSource).toContain("export interface GrokBuildOauthDeviceCodeChallenge");
    expect(typesSource).toMatch(/interface GrokBuildOauthStatus \{[\s\S]*loggedIn: boolean/);
    expect(typesSource).toMatch(
      /interface GrokBuildOauthDeviceCodeChallenge \{[\s\S]*verificationUrl: string;[\s\S]*userCode: string;[\s\S]*deviceCode: string;[\s\S]*intervalSecs: number/,
    );
  });
});

describe("Grok Build i18n keys", () => {
  it("defines every required settings key in English and Chinese", () => {
    for (const key of GROK_I18N_KEYS) {
      const enValue = (en.settings as Record<string, unknown>)[key];
      const zhValue = (zh.settings as Record<string, unknown>)[key];
      expect(enValue, `en.settings.${key}`).toEqual(expect.any(String));
      expect(zhValue, `zh.settings.${key}`).toEqual(expect.any(String));
      expect(String(enValue).trim().length, `en.settings.${key} should be non-empty`).toBeGreaterThan(0);
      expect(String(zhValue).trim().length, `zh.settings.${key} should be non-empty`).toBeGreaterThan(0);
    }
  });
});

describe("Grok Build backend wiring contract", () => {
  it("registers the Grok Build OAuth service, commands, and generate_handler entries", () => {
    expect(servicesModSource).toContain("pub mod grok_build_oauth_service");
    expect(commandsModSource).toContain("pub mod grok_build_oauth");
    expect(libSource).toContain("commands::grok_build_oauth::get_grok_build_oauth_status");
    expect(libSource).toContain("commands::grok_build_oauth::login_grok_build_oauth");
    expect(libSource).toContain("commands::grok_build_oauth::start_grok_build_oauth_device_code");
    expect(libSource).toContain("commands::grok_build_oauth::complete_grok_build_oauth_device_code");
    expect(libSource).toContain("commands::grok_build_oauth::logout_grok_build_oauth");
    expect(libSource).toContain("grok_build_oauth_service");
  });

  it("uses the Grok CLI public-client identity, keyring slots, and session meta file", () => {
    const grokOauthSource = readRepo("src-tauri/src/services/grok_build_oauth_service.rs");
    const grokCommandsSource = readRepo("src-tauri/src/commands/grok_build_oauth.rs");
    expect(grokOauthSource).toContain("b1a00492-073a-47ea-816f-4c329264a828");
    expect(grokOauthSource).toContain("https://auth.x.ai");
    expect(grokOauthSource).toContain("https://auth.x.ai/oauth2/authorize");
    expect(grokOauthSource).toContain("https://auth.x.ai/oauth2/token");
    expect(grokOauthSource).toContain("https://auth.x.ai/oauth2/device/code");
    expect(grokOauthSource).toContain("openid profile email offline_access grok-cli:access api:access");
    expect(grokOauthSource).toContain("http://127.0.0.1:56121/callback");
    expect(grokOauthSource).toContain("light-whisper");
    expect(grokOauthSource).toContain("grok-build-oauth");
    expect(grokOauthSource).toContain("grok-build-oauth-refresh-token");
    expect(grokOauthSource).toContain("grok_build_oauth_session.json");
    expect(grokCommandsSource).toContain("login_grok_build_oauth");
    expect(grokCommandsSource).toContain("start_grok_build_oauth_device_code");
    expect(grokCommandsSource).toContain("complete_grok_build_oauth_device_code");
    expect(grokCommandsSource).toContain("logout_grok_build_oauth");
    expect(grokCommandsSource).toContain("get_grok_build_oauth_status");
  });

  it("uses Grok CLI identity headers and proxy URLs for OAuth origin auth", () => {
    const grokOauthSource = readRepo("src-tauri/src/services/grok_build_oauth_service.rs");
    expect(grokOauthSource).toContain("https://cli-chat-proxy.grok.com");
    expect(grokOauthSource).toContain("https://api.x.ai");
    expect(grokOauthSource).toContain("xai-grok-cli");
    expect(grokOauthSource).toContain("grok-shell");
    expect(grokOauthSource).toContain("x-grok-client-identifier");
    expect(grokOauthSource).toContain("X-XAI-Token-Auth");
    expect(grokOauthSource).toContain("x-grok-client-version");
    expect(grokOauthSource).toContain("0.2.114");
    expect(llmClientSource).toContain("is_grok_build_oauth_origin_auth");
    expect(llmClientSource).toContain("cli-chat-proxy.grok.com");
  });

  it("adds xai as a built-in Responses-API provider", () => {
    expect(userProfileSource).toMatch(
      /#\[serde\(rename_all = "snake_case"\)\]\s*pub enum XaiAuthMode/,
    );
    expect(userProfileSource).toMatch(/enum XaiAuthMode \{[\s\S]*ApiKey[\s\S]*Oauth/);
    expect(userProfileSource).toContain("xai_auth_mode");
    expect(userProfileSource).toMatch(/fn is_builtin_provider[\s\S]*"xai"/);
    expect(llmProviderSource).toContain('"xai"');
    expect(llmProviderSource).toContain("https://api.x.ai");
    expect(llmProviderSource).toContain("grok-4.6");
  });

  it("threads xai_auth_mode through list_ai_models, set_llm_provider_config, and the API-key resolver", () => {
    expect(aiPolishSource.includes("xai_auth_mode")).toBe(true);
    expect(aiPolishSource.includes("cli-chat-proxy.grok.com/v1/models")).toBe(true);
    expect(aiPolishSource.includes("https://api.x.ai/v1/models")).toBe(true);
    expect(profileCommandSource.includes("xai_auth_mode")).toBe(true);
    expect(codexOauthSource.includes('"xai"')).toBe(true);
    expect(tauriSource.includes("xaiAuthMode: xaiAuthMode ?? null")).toBe(true);
    expect(appStateSource.includes("grok_build_oauth_session")).toBe(true);
  });
});
