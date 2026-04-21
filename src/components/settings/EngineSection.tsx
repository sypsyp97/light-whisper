import { useCallback, useEffect, useMemo, useState } from "react";
import { Globe, Cloud, RefreshCw } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import {
  getEngine,
  setEngine,
  getOnlineAsrApiKey,
  setOnlineAsrApiKey,
  getOnlineAsrEndpoint,
  setOnlineAsrEndpoint,
  getAlibabaAsrConfig,
  setAlibabaAsrModel,
  listAlibabaAsrModels,
} from "@/api/tauri";
import Field from "@/components/ui/Field";
import Picker from "@/components/ui/Picker";
import SecretInput from "@/components/ui/SecretInput";
import IconButton from "@/components/ui/IconButton";
import { useDebouncedCallback } from "@/hooks/useDebouncedCallback";

type EngineKey = "glm-asr" | "alibaba-asr";

export function EngineSection() {
  const { t } = useTranslation();
  const [engine, setEngineState] = useState<EngineKey>("alibaba-asr");
  const [apiKey, setApiKey] = useState("");
  const [region, setRegion] = useState<"international" | "domestic">("international");
  const [alibabaModel, setAlibabaModelState] = useState("qwen3-asr-flash");
  const [alibabaModels, setAlibabaModels] = useState<readonly string[]>([]);

  useEffect(() => {
    void getEngine().then((e) => setEngineState(e as EngineKey)).catch(() => {});
    void getOnlineAsrApiKey().then(setApiKey).catch(() => {});
    void getOnlineAsrEndpoint().then((r) => {
      if (r.region === "international" || r.region === "domestic") setRegion(r.region);
    }).catch(() => {});
    void getAlibabaAsrConfig().then((cfg) => {
      setAlibabaModelState(cfg.model);
      setAlibabaModels(cfg.models);
    }).catch(() => {});
  }, []);

  const keyringUser = `${engine}:${region}`;
  const apiKeySave = useDebouncedCallback((value: string, user: string) => {
    void setOnlineAsrApiKey(value, user).catch(() => toast.error(t("toast.switchEngineFailed")));
  }, 900, { onUnmount: "flush" });

  const handleApiKeyChange = useCallback((v: string) => {
    setApiKey(v);
    apiKeySave.schedule(v, keyringUser);
  }, [apiKeySave, keyringUser]);

  const handleEngineChange = useCallback(async (next: EngineKey) => {
    const prev = engine;
    setEngineState(next);
    try {
      await setEngine(next);
      toast.success(t("toast.switchedToEngine", { label: next === "alibaba-asr" ? t("settings.alibabaAsrLabel") : "GLM-ASR" }));
    } catch {
      setEngineState(prev);
      toast.error(t("toast.switchEngineFailed"));
    }
  }, [engine, t]);

  const handleRegionChange = useCallback(async (next: "international" | "domestic") => {
    const prev = region;
    setRegion(next);
    try { await setOnlineAsrEndpoint(next); } catch { setRegion(prev); }
  }, [region]);

  const handleAlibabaModelChange = useCallback(async (next: string) => {
    const prev = alibabaModel;
    setAlibabaModelState(next);
    try { await setAlibabaAsrModel(next); } catch { setAlibabaModelState(prev); }
  }, [alibabaModel]);

  const refreshAlibabaModels = useCallback(async () => {
    try {
      const res = await listAlibabaAsrModels();
      setAlibabaModels(res.models);
    } catch {
      toast.error(t("toast.micListFailed"));
    }
  }, [t]);

  const engineOptions = useMemo(() => [
    { value: "glm-asr" as const, label: "GLM-ASR", description: t("settings.glmAsrDesc"), icon: <Globe size={14} /> },
    { value: "alibaba-asr" as const, label: t("settings.alibabaAsrLabel"), description: t("settings.alibabaAsrDesc"), icon: <Cloud size={14} /> },
  ], [t]);

  const regionOptions = [
    { value: "international" as const, label: t("settings.international") },
    { value: "domestic" as const, label: t("settings.domestic") },
  ];

  const modelOptions = alibabaModels.map((m) => ({ value: m, label: m }));
  if (alibabaModel && !modelOptions.some((o) => o.value === alibabaModel)) {
    modelOptions.unshift({ value: alibabaModel, label: alibabaModel });
  }

  const placeholder = engine === "alibaba-asr"
    ? t("settings.alibabaApiKeyPlaceholder")
    : t("settings.glmApiKeyPlaceholder");

  return (
    <section
      className="lw-settings-section"
      data-testid="settings-section-engine"
      data-nav-id="engine"
    >
      <h2 className="lw-settings-section-title">{t("settings.engine")}</h2>
      <Field label={t("settings.engine")}>
        <Picker
          value={engine}
          options={engineOptions}
          onChange={(v) => void handleEngineChange(v as EngineKey)}
          data-testid="engine-picker"
        />
      </Field>
      <Field label={t("settings.apiKey")}>
        <SecretInput
          value={apiKey}
          onChange={handleApiKeyChange}
          placeholder={placeholder}
          data-testid="engine-api-key"
        />
      </Field>
      {engine === "alibaba-asr" && (
        <>
          <Field label={t("settings.apiEndpoint")}>
            <Picker
              value={region}
              options={regionOptions}
              onChange={(v) => void handleRegionChange(v as "international" | "domestic")}
              data-testid="engine-region-picker"
            />
          </Field>
          <Field label={t("settings.alibabaModelLabel")}>
            <div className="lw-inline" style={{ width: "100%" }}>
              <div style={{ flex: 1 }}>
                <Picker
                  value={alibabaModel}
                  options={modelOptions}
                  onChange={(v) => void handleAlibabaModelChange(v)}
                  data-testid="engine-model-picker"
                  searchable
                />
              </div>
              <IconButton
                label={t("settings.alibabaModelsRefresh")}
                icon={<RefreshCw size={14} />}
                onClick={() => void refreshAlibabaModels()}
                data-testid="engine-model-refresh"
              />
            </div>
          </Field>
        </>
      )}
    </section>
  );
}

export default EngineSection;
