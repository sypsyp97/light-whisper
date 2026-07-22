import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import { X } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";

import {
  removeCorrection,
  setCorrectionValidationConfig,
  validateCorrections,
} from "@/api/tauri";
import type { CorrectionPattern, UserProfile } from "@/types";

interface CorrectionRulesModalProps {
  profile: UserProfile | null;
  allProviderOptions: { key: string; label: string }[];
  validationEnabled: boolean;
  setValidationEnabled: (value: boolean) => void;
  validationUseSeparateModel: boolean;
  setValidationUseSeparateModel: (value: boolean) => void;
  validationProvider: string | null;
  setValidationProvider: (value: string | null) => void;
  validationModel: string;
  setValidationModel: (value: string) => void;
  validationRunning: boolean;
  setValidationRunning: (value: boolean) => void;
  validationResult: string | null;
  setValidationResult: (value: string | null) => void;
  returnFocusRef: RefObject<HTMLButtonElement | null>;
  onClose: () => void;
  onRefreshProfile: () => void;
}

const correctionSourceColors: Record<string, string> = {
  user: "var(--color-accent)",
  ai: "var(--color-learned)",
  learned: "var(--color-warning)",
};

export default function CorrectionRulesModal({
  profile,
  allProviderOptions,
  validationEnabled,
  setValidationEnabled,
  validationUseSeparateModel,
  setValidationUseSeparateModel,
  validationProvider,
  setValidationProvider,
  validationModel,
  setValidationModel,
  validationRunning,
  setValidationRunning,
  validationResult,
  setValidationResult,
  returnFocusRef,
  onClose,
  onRefreshProfile,
}: CorrectionRulesModalProps) {
  const { t } = useTranslation();
  const [search, setSearch] = useState("");
  const [sourceFilter, setSourceFilter] = useState<"all" | "user" | "ai">("all");
  const dialogRef = useRef<HTMLDivElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    searchInputRef.current?.focus();
    return () => {
      returnFocusRef.current?.focus();
    };
  }, [returnFocusRef]);

  const handleDialogKeyDown = useCallback((event: KeyboardEvent) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onClose();
      return;
    }
    if (event.key !== "Tab") return;

    const dialog = dialogRef.current;
    if (!dialog) return;
    const focusable = Array.from(dialog.querySelectorAll<HTMLElement>([
      "button:not([disabled])",
      "input:not([disabled])",
      "select:not([disabled])",
      "textarea:not([disabled])",
      "a[href]",
      '[tabindex]:not([tabindex="-1"])',
    ].join(","))).filter((element) => (
      !element.hasAttribute("hidden") && element.getAttribute("aria-hidden") !== "true"
    ));

    if (focusable.length === 0) {
      event.preventDefault();
      dialog.focus();
      return;
    }
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    const active = document.activeElement;
    if (event.shiftKey && (active === first || !dialog.contains(active))) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && (active === last || !dialog.contains(active))) {
      event.preventDefault();
      first.focus();
    }
  }, [onClose]);

  useEffect(() => {
    document.addEventListener("keydown", handleDialogKeyDown);
    return () => document.removeEventListener("keydown", handleDialogKeyDown);
  }, [handleDialogKeyDown]);

  const patterns: CorrectionPattern[] = profile?.correction_patterns ?? [];

  const filtered = patterns.filter((pattern) => {
    if (sourceFilter !== "all" && pattern.source !== sourceFilter) return false;
    if (search.trim()) {
      const keyword = search.trim().toLowerCase();
      if (!pattern.original.toLowerCase().includes(keyword)
        && !pattern.corrected.toLowerCase().includes(keyword)) return false;
    }
    return true;
  });

  const handleDelete = async (original: string, corrected: string) => {
    try {
      await removeCorrection(original, corrected);
      onRefreshProfile();
    } catch {
      toast.error(t("settings.correctionDeleteFailed"));
    }
  };

  return (
    <div className="correction-modal">
      <button
        type="button"
        className="modal-dismiss"
        tabIndex={-1}
        aria-label={t("common.close")}
        onClick={onClose}
      />
      <div
        ref={dialogRef}
        className="animate-fade-in correction-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="correction-rules-title"
        tabIndex={-1}
      >
        <div style={{
          display: "flex", alignItems: "center", gap: 8,
          padding: "14px 16px 12px",
          borderBottom: "1px solid var(--color-border-subtle)",
        }}>
          <span id="correction-rules-title" style={{ fontSize: 14, fontWeight: 600, color: "var(--color-text-primary)", flex: 1 }}>
            {t("settings.correctionRules")}
          </span>
          <span style={{ fontSize: 11, color: "var(--color-text-tertiary)" }}>
            {t("settings.correctionRulesCount", { count: patterns.length })}
          </span>
          <button
            className="icon-btn"
            type="button"
            onClick={onClose}
            aria-label={t("common.close")}
          >
            <X size={15} />
          </button>
        </div>

        <div style={{ padding: "10px 16px 0", display: "flex", gap: 8, alignItems: "center" }}>
          <input
            ref={searchInputRef}
            type="text"
            aria-label={t("settings.correctionSearchLabel")}
            placeholder={t("settings.correctionSearchPlaceholder")}
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            className="settings-input"
            style={{ flex: 1, padding: "6px 10px", fontSize: 12 }}
          />
          {(["all", "user", "ai"] as const).map((filter) => (
            <button
              key={filter}
              type="button"
              onClick={() => setSourceFilter(filter)}
              className="test-btn"
              aria-pressed={sourceFilter === filter}
              style={sourceFilter === filter ? {
                background: "var(--color-accent-subtle)",
                color: "var(--color-accent)",
                borderColor: "var(--color-border-accent)",
              } : undefined}
            >
              {filter === "all"
                ? t("settings.correctionFilterAll")
                : filter === "user"
                  ? t("settings.correctionFilterUser")
                  : t("settings.correctionFilterAi")}
            </button>
          ))}
        </div>

        <div style={{ flex: 1, overflowY: "auto", padding: "8px 16px" }}>
          {filtered.length === 0 ? (
            <p style={{ fontSize: 12, color: "var(--color-text-tertiary)", textAlign: "center", padding: "24px 0" }}>
              {t("settings.correctionEmpty")}
            </p>
          ) : (
            filtered.map((pattern) => (
              <div
                key={`${pattern.original}→${pattern.corrected}`}
                style={{
                  display: "flex", alignItems: "center", gap: 8,
                  padding: "7px 0",
                  borderBottom: "1px solid var(--color-border-subtle)",
                }}
              >
                <span
                  style={{
                    width: 6, height: 6, borderRadius: "50%", flexShrink: 0,
                    background: correctionSourceColors[pattern.source] ?? "var(--color-border)",
                  }}
                />
                <span style={{ flex: 1, fontSize: 12, color: "var(--color-text-primary)", minWidth: 0 }}>
                  <span style={{ color: "var(--color-text-secondary)" }}>{pattern.original}</span>
                  <span style={{ margin: "0 6px", color: "var(--color-text-tertiary)" }}>→</span>
                  <span>{pattern.corrected}</span>
                </span>
                <span style={{
                  fontSize: 11, color: "var(--color-text-tertiary)",
                  background: "var(--color-bg-secondary)",
                  border: "1px solid var(--color-border-subtle)",
                  borderRadius: "var(--radius-full)", padding: "1px 7px", flexShrink: 0,
                }}>
                  {pattern.count}
                </span>
                <button
                  className="icon-btn"
                  type="button"
                  onClick={() => void handleDelete(pattern.original, pattern.corrected)}
                  aria-label={t("settings.correctionDeleteRuleLabel", {
                    original: pattern.original,
                    corrected: pattern.corrected,
                  })}
                  style={{ flexShrink: 0 }}
                >
                  <X size={13} />
                </button>
              </div>
            ))
          )}
        </div>

        <div style={{ padding: "12px 16px 14px", borderTop: "1px solid var(--color-border-subtle)" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 4 }}>
            <label id="correction-validation-label" style={{ fontSize: 13, color: "var(--color-text-primary)", flex: 1 }}>
              {t("settings.correctionValidationToggle")}
            </label>
            <button
              className="toggle-switch"
              role="switch"
              aria-checked={validationEnabled}
              aria-labelledby="correction-validation-label"
              onClick={async () => {
                const next = !validationEnabled;
                setValidationEnabled(next);
                await setCorrectionValidationConfig({ enabled: next });
              }}
              style={{ background: validationEnabled ? "var(--color-accent)" : "var(--color-bg-tertiary)" }}
            >
              <div className="toggle-knob" style={{ transform: validationEnabled ? "translateX(20px)" : "translateX(0)" }} />
            </button>
          </div>
          <p style={{ fontSize: 11, color: "var(--color-text-tertiary)", margin: "0 0 8px" }}>
            {t("settings.correctionValidationHint")}
          </p>

          {validationEnabled && (
            <>
              <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 6 }}>
                <label id="correction-validation-model-label" style={{ fontSize: 12, color: "var(--color-text-secondary)", flex: 1 }}>
                  {t("settings.correctionValidationSeparateModel")}
                </label>
                <button
                  className="toggle-switch"
                  role="switch"
                  aria-checked={validationUseSeparateModel}
                  aria-labelledby="correction-validation-model-label"
                  onClick={async () => {
                    const next = !validationUseSeparateModel;
                    setValidationUseSeparateModel(next);
                    await setCorrectionValidationConfig({
                      enabled: validationEnabled,
                      useSeparateModel: next,
                    });
                  }}
                  style={{ background: validationUseSeparateModel ? "var(--color-accent)" : "var(--color-bg-tertiary)" }}
                >
                  <div className="toggle-knob" style={{ transform: validationUseSeparateModel ? "translateX(20px)" : "translateX(0)" }} />
                </button>
              </div>

              {validationUseSeparateModel && (
                <div style={{ display: "flex", gap: 6, marginBottom: 6 }}>
                  <select
                    aria-label={t("settings.provider")}
                    value={validationProvider ?? ""}
                    onChange={async (event) => {
                      const value = event.target.value || null;
                      setValidationProvider(value);
                      await setCorrectionValidationConfig({
                        enabled: validationEnabled,
                        provider: value,
                      });
                    }}
                    className="settings-input"
                    style={{ flex: 1, padding: "6px 8px", fontSize: 12 }}
                  >
                    <option value="">{t("settings.correctionValidationFollowPolish")}</option>
                    {allProviderOptions.map((option) => (
                      <option key={option.key} value={option.key}>{option.label}</option>
                    ))}
                  </select>
                  <input
                    type="text"
                    aria-label={t("settings.modelLabel")}
                    placeholder={t("settings.correctionValidationModelPlaceholder")}
                    value={validationModel}
                    onChange={(event) => setValidationModel(event.target.value)}
                    onBlur={async () => {
                      await setCorrectionValidationConfig({
                        enabled: validationEnabled,
                        model: validationModel || null,
                      });
                    }}
                    className="settings-input"
                    style={{ flex: 1, padding: "6px 8px", fontSize: 12 }}
                  />
                </div>
              )}

              <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <button
                  className="test-btn"
                  disabled={validationRunning}
                  onClick={async () => {
                    setValidationRunning(true);
                    setValidationResult(null);
                    try {
                      const removed = await validateCorrections();
                      setValidationResult(removed > 0
                        ? t("settings.correctionValidationRemoved", { count: removed })
                        : t("settings.correctionValidationAllValid"));
                    } catch (error) {
                      setValidationResult(t("settings.correctionValidationFailed", {
                        error: error instanceof Error ? error.message : String(error),
                      }));
                    } finally {
                      setValidationRunning(false);
                      onRefreshProfile();
                    }
                  }}
                  style={{ padding: "5px 12px", fontSize: 12 }}
                >
                  {validationRunning
                    ? t("settings.correctionValidationRunning")
                    : t("settings.correctionValidationRun")}
                </button>
                <span
                  role="status"
                  aria-atomic="true"
                  style={{ fontSize: 11, color: "var(--color-text-tertiary)" }}
                >
                  {validationResult ?? ""}
                </span>
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
