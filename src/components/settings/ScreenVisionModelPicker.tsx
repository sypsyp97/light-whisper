import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ChevronsUpDown } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useExclusivePicker } from "@/hooks/useExclusivePicker";
import { useDebouncedCallback } from "@/hooks/useDebouncedCallback";
import { listAiModels } from "@/api/tauri";
import type { AiModelInfo, OpenaiAuthMode } from "@/types";
import ScreenVisionAuth from "@/components/settings/ScreenVisionAuth";

interface ScreenVisionModelPickerProps {
  model: string;
  provider: string;
  baseUrl: string;
  apiKey: string;
  loggedIn: boolean;
  authIdentity: string;
  openaiAuthMode?: OpenaiAuthMode;
  providerOptions: Array<{ key: string; label: string }>;
  onProviderChange: (provider: string) => void;
  onApiKeyChange: (value: string) => void;
  onChange: (model: string) => void;
  onBlur: () => void;
  onSelect: (model: string) => void;
}

export default function ScreenVisionModelPicker({
  model,
  provider,
  baseUrl,
  apiKey,
  loggedIn,
  authIdentity,
  openaiAuthMode,
  providerOptions,
  onProviderChange,
  onApiKeyChange,
  onChange,
  onBlur,
  onSelect,
}: ScreenVisionModelPickerProps) {
  const { t } = useTranslation();
  const picker = useExclusivePicker<"model">();
  const searchInputRef = useRef<HTMLInputElement | null>(null);
  const requestIdRef = useRef(0);
  const [search, setSearch] = useState("");
  const [models, setModels] = useState<AiModelInfo[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [sourceUrl, setSourceUrl] = useState("");
  const hasAuth = provider === "openai" && openaiAuthMode === "oauth"
    ? loggedIn
    : Boolean(apiKey.trim());
  const requestContext = JSON.stringify([
    provider,
    baseUrl,
    apiKey.trim(),
    authIdentity,
    openaiAuthMode,
  ]);
  const contextRef = useRef<string | null>(null);
  const filteredModels = useMemo(() => {
    const keyword = search.trim().toLowerCase();
    if (!keyword) return models;
    return models.filter((item) => item.id.toLowerCase().includes(keyword)
      || (item.ownedBy ?? "").toLowerCase().includes(keyword));
  }, [models, search]);
  const selectedModel = models.find((item) => item.id === model);

  const refresh = useCallback(async (silent = false) => {
    const requestId = ++requestIdRef.current;
    if (!hasAuth) {
      setModels([]);
      setSourceUrl("");
      setError(t("settings.apiKeyOrLoginMissing"));
      contextRef.current = null;
      return;
    }
    setLoading(true);
    if (!silent) setError("");
    try {
      const payload = await listAiModels(
        provider,
        baseUrl || undefined,
        apiKey.trim(),
        !silent,
        provider === "openai" ? openaiAuthMode : undefined,
      );
      if (requestId !== requestIdRef.current) return;
      setModels(payload.models);
      setSourceUrl(payload.sourceUrl);
      setError(payload.models.length === 0 ? t("settings.modelListEmpty") : "");
      contextRef.current = requestContext;
    } catch (err) {
      if (requestId !== requestIdRef.current) return;
      if (contextRef.current !== requestContext) {
        setModels([]);
        setSourceUrl("");
        contextRef.current = null;
      }
      setError(err instanceof Error ? err.message : t("settings.fetchModelsFailed"));
    } finally {
      if (requestId === requestIdRef.current) setLoading(false);
    }
  }, [apiKey, baseUrl, hasAuth, openaiAuthMode, provider, requestContext, t]);
  const fetchModels = useDebouncedCallback((silent: boolean) => {
    void refresh(silent);
  }, 700);

  useEffect(() => {
    if (contextRef.current !== requestContext) {
      contextRef.current = null;
      setModels([]);
      setSourceUrl("");
      setError("");
    }
    fetchModels.schedule(true);
    return () => {
      fetchModels.cancel();
      requestIdRef.current += 1;
    };
  }, [fetchModels, requestContext]);

  const select = (value: string) => {
    const normalized = value.trim();
    if (!normalized) return;
    onSelect(normalized);
    setSearch("");
    picker.close();
  };

  return (
    <div className="settings-column" style={{ gap: 8, width: "100%" }}>
      <div style={{ display: "flex", gap: 8, alignItems: "flex-start" }}>
        <select
          aria-label={t("settings.screenVisionProvider")}
          value={provider}
          onChange={(event) => onProviderChange(event.target.value)}
          className="settings-input"
          style={{ flex: 1, padding: "8px 10px", fontSize: 12 }}
        >
          {providerOptions.map((option) => (
            <option key={option.key} value={option.key}>{option.label}</option>
          ))}
        </select>
        <div
          className="picker-shell"
          ref={picker.setRef("model")}
          style={{ flex: 1, zIndex: picker.isOpen("model") ? 9 : "auto" }}
        >
      <div className="picker-inline-row">
        <input
          type="text"
          aria-label={t("settings.screenVisionModel")}
          placeholder={t("settings.screenVisionModelPlaceholder")}
          value={model}
          onChange={(event) => onChange(event.target.value)}
          onBlur={onBlur}
          className="settings-input"
        />
        <button
          type="button"
          className="picker-inline-button"
          data-open={picker.isOpen("model")}
          aria-haspopup="listbox"
          aria-expanded={picker.isExpanded("model")}
          onClick={() => {
            picker.toggle("model");
            requestAnimationFrame(() => searchInputRef.current?.focus());
          }}
          aria-label={t("settings.openModelList")}
          title={t("settings.openModelList")}
        >
          <ChevronsUpDown size={14} className="icon-tertiary" />
        </button>
      </div>
      {picker.isOpen("model") && (
        <div className={picker.popoverClass("model")}>
          <div className="picker-toolbar">
            <input
              ref={searchInputRef}
              type="text"
              className="settings-input picker-search-input"
              placeholder={t("settings.searchModelPlaceholder")}
              aria-label={t("settings.searchModelLabel")}
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter" && search.trim()) {
                  event.preventDefault();
                  select(search);
                }
              }}
            />
            <button
              type="button"
              className="btn-ghost btn-ghost-sm"
              onClick={() => {
                fetchModels.cancel();
                void refresh();
              }}
              disabled={loading}
              style={{ opacity: loading ? 0.7 : 1 }}
            >
              {loading ? t("settings.fetching") : t("common.refresh")}
            </button>
          </div>
          <p className="settings-hint" style={{ margin: 0 }}>
            {sourceUrl
              ? t("settings.modelSourceUrl", { url: sourceUrl })
              : t("settings.autoFetchHint")}
          </p>
          {search.trim() ? (
            <button
              type="button"
              className="picker-option picker-option-action"
              onClick={() => select(search)}
            >
              <span className="picker-option-copy">
                <strong>{t("settings.useAsModel", { name: search.trim() })}</strong>
                <span>{t("settings.asCurrentModelName")}</span>
              </span>
            </button>
          ) : null}
          <div className="picker-list" role="listbox">
            {filteredModels.length > 0 ? filteredModels.map((item) => (
              <button
                key={item.id}
                type="button"
                className="picker-option"
                data-active={model === item.id}
                onClick={() => select(item.id)}
              >
                <span className="picker-option-copy">
                  <strong>{item.id}</strong>
                  <span>{item.ownedBy || provider}</span>
                </span>
              </button>
            )) : (
              <p className="picker-empty">{error || t("settings.noModels")}</p>
            )}
          </div>
        </div>
      )}
          <p className="settings-hint" style={{ margin: "4px 0 0" }}>
            {selectedModel?.ownedBy
              || (models.length > 0
                ? t("settings.availableModels", { count: models.length })
                : error || t("settings.canInputModelName"))}
          </p>
        </div>
      </div>
      <ScreenVisionAuth
        provider={provider}
        openaiAuthMode={openaiAuthMode ?? "api_key"}
        loggedIn={loggedIn}
        apiKey={apiKey}
        onChange={onApiKeyChange}
      />
    </div>
  );
}
