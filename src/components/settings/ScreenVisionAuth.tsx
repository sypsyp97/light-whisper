import { useTranslation } from "react-i18next";
import SecretInput from "@/components/SecretInput";
import type { OpenaiAuthMode, XaiAuthMode } from "@/types";

interface ScreenVisionAuthProps {
  provider: string;
  openaiAuthMode: OpenaiAuthMode;
  xaiAuthMode?: XaiAuthMode;
  loggedIn: boolean;
  grokLoggedIn?: boolean;
  apiKey: string;
  onChange: (value: string) => void;
}

export default function ScreenVisionAuth({
  provider,
  openaiAuthMode,
  xaiAuthMode = "api_key",
  loggedIn,
  grokLoggedIn = false,
  apiKey,
  onChange,
}: ScreenVisionAuthProps) {
  const { t } = useTranslation();
  if (provider === "xai" && xaiAuthMode === "oauth") {
    return (
      <p className="settings-hint" style={{ margin: 0 }}>
        {t(grokLoggedIn ? "settings.grokBuildOauthConnectedHint" : "settings.grokBuildOauthHint", {
          summary: "xAI",
        })}
      </p>
    );
  }
  if (provider === "openai" && openaiAuthMode === "oauth") {
    return (
      <p className="settings-hint" style={{ margin: 0 }}>
        {t(loggedIn ? "settings.screenVisionUsesOauth" : "settings.screenVisionOauthRequired")}
      </p>
    );
  }
  return (
    <div className="settings-column" style={{ gap: 6 }}>
      <span className="settings-option-desc">{t("settings.screenVisionApiKey")}</span>
      <SecretInput
        value={apiKey}
        placeholder={t("settings.screenVisionApiKeyPlaceholder")}
        ariaLabel={t("settings.screenVisionApiKey")}
        ariaLabelShow={t("settings.showApiKey")}
        ariaLabelHide={t("settings.hideApiKey")}
        onChange={onChange}
      />
    </div>
  );
}
