import { useCallback, useEffect, useMemo, useState } from "react";
import { X } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import {
  addHotWord,
  removeHotWord,
  getUserProfile,
  removeCorrection,
  validateCorrections,
  setCorrectionValidationConfig,
  setLlmProviderConfig,
} from "@/api/tauri";
import type { HotWord, CorrectionPattern, UserProfile } from "@/types";
import Field from "@/components/ui/Field";
import TextInput from "@/components/ui/TextInput";
import Button from "@/components/ui/Button";
import IconButton from "@/components/ui/IconButton";
import Modal from "@/components/ui/Modal";
import Segmented from "@/components/ui/Segmented";
import Toggle from "@/components/ui/Toggle";
import Picker from "@/components/ui/Picker";

export default function VocabularySection() {
  const { t } = useTranslation();
  const [profile, setProfile] = useState<UserProfile | null>(null);
  const [newWord, setNewWord] = useState("");
  const [showRules, setShowRules] = useState(false);

  const refresh = useCallback(async () => {
    try { setProfile(await getUserProfile()); } catch { /* */ }
  }, []);

  useEffect(() => { void refresh(); }, [refresh]);

  const hotWords: HotWord[] = profile?.hot_words ?? [];
  const corrections: CorrectionPattern[] = profile?.correction_patterns ?? [];

  const handleAdd = useCallback(async () => {
    const trimmed = newWord.trim();
    if (!trimmed) return;
    try {
      await addHotWord(trimmed, 5);
      toast.success(t("toast.hotWordAdded", { word: trimmed }));
      setNewWord("");
      await refresh();
    } catch {
      toast.error(t("toast.hotWordAddFailed"));
    }
  }, [newWord, refresh, t]);

  const handleRemove = useCallback(async (text: string) => {
    try { await removeHotWord(text); await refresh(); } catch { /* */ }
  }, [refresh]);

  return (
    <section
      className="lw-settings-section"
      data-testid="settings-section-vocabulary"
      data-nav-id="vocabulary"
    >
      <h2 className="lw-settings-section-title">{t("settings.vocabulary")}</h2>
      <Field label={t("settings.vocabulary")}>
        <div className="lw-inline" style={{ width: "100%" }}>
          <div style={{ flex: 1 }}>
            <TextInput
              value={newWord}
              onChange={setNewWord}
              placeholder={t("settings.addHotWordPlaceholder")}
              onKeyDown={(e) => { if (e.key === "Enter") void handleAdd(); }}
              data-testid="hot-word-input"
            />
          </div>
          <Button onClick={() => void handleAdd()} data-testid="hot-word-add-btn">
            {t("common.add")}
          </Button>
        </div>
      </Field>
      <div className="lw-field-hint">
        {t("settings.hotWordsCount", {
          count: hotWords.length,
          transcriptions: profile?.total_transcriptions ?? 0,
        })}
      </div>
      <div className="lw-inline" style={{ flexWrap: "wrap", gap: 6 }}>
        {hotWords.map((w) => (
          <span
            key={w.text}
            className={`lw-hot-word ${w.source === "learned" ? "lw-hot-word--learned" : ""}`}
          >
            {w.text}
            <button
              type="button"
              className="lw-hot-word-remove"
              aria-label={t("common.clear")}
              onClick={() => void handleRemove(w.text)}
              data-testid={`hot-word-remove-${w.text}`}
            >
              <X size={12} />
            </button>
          </span>
        ))}
      </div>
      <div>
        <Button
          variant="ghost"
          onClick={() => setShowRules(true)}
          data-testid="correction-rules-btn"
        >
          {t("settings.correctionRules")} · {t("settings.correctionRulesCount", { count: corrections.length })}
        </Button>
      </div>
      <CorrectionRulesModal
        open={showRules}
        onClose={() => { setShowRules(false); void refresh(); }}
        corrections={corrections}
        profile={profile}
      />
    </section>
  );
}

function CorrectionRulesModal({
  open,
  onClose,
  corrections,
  profile,
}: {
  open: boolean;
  onClose: () => void;
  corrections: CorrectionPattern[];
  profile: UserProfile | null;
}) {
  const { t } = useTranslation();
  const [filter, setFilter] = useState<"all" | "user" | "ai">("all");
  const [search, setSearch] = useState("");
  const [validationEnabled, setValidationEnabled] = useState(() => Boolean(profile?.correction_validation_enabled));
  const [separateModel, setSeparateModel] = useState(() => Boolean(profile?.llm_provider.validation_use_separate_model));
  const [validationProvider, setValidationProvider] = useState<string>(
    () => profile?.llm_provider.validation_provider ?? "cerebras",
  );
  const [validationModel, setValidationModel] = useState<string>(
    () => profile?.llm_provider.validation_model ?? "",
  );
  const [validating, setValidating] = useState(false);

  useEffect(() => {
    setValidationEnabled(Boolean(profile?.correction_validation_enabled));
    setSeparateModel(Boolean(profile?.llm_provider.validation_use_separate_model));
    setValidationProvider(profile?.llm_provider.validation_provider ?? "cerebras");
    setValidationModel(profile?.llm_provider.validation_model ?? "");
  }, [profile]);

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    return corrections.filter((c) => {
      if (filter === "user" && c.source !== "user") return false;
      if (filter === "ai" && c.source !== "ai") return false;
      if (!q) return true;
      return c.original.toLowerCase().includes(q) || c.corrected.toLowerCase().includes(q);
    });
  }, [corrections, filter, search]);

  const handleRemove = useCallback(async (c: CorrectionPattern) => {
    try { await removeCorrection(c.original, c.corrected); } catch { /* */ }
  }, []);

  const handleValidationToggle = useCallback(async (next: boolean) => {
    setValidationEnabled(next);
    try {
      await setCorrectionValidationConfig({
        enabled: next,
        useSeparateModel: separateModel,
        provider: validationProvider || null,
        model: validationModel || null,
      });
    } catch { /* */ }
  }, [separateModel, validationProvider, validationModel]);

  const handleSeparateToggle = useCallback(async (next: boolean) => {
    setSeparateModel(next);
    try {
      await setCorrectionValidationConfig({
        enabled: validationEnabled,
        useSeparateModel: next,
        provider: validationProvider || null,
        model: validationModel || null,
      });
      await setLlmProviderConfig("", undefined, undefined, undefined, undefined, undefined, undefined, null);
    } catch { /* */ }
  }, [validationEnabled, validationProvider, validationModel]);

  const handleValidate = useCallback(async () => {
    setValidating(true);
    try {
      const count = await validateCorrections();
      if (count > 0) toast.success(t("settings.correctionValidationRemoved", { count }));
      else toast.success(t("settings.correctionValidationAllValid"));
    } catch (err) {
      toast.error(t("settings.correctionValidationFailed", { error: err instanceof Error ? err.message : "" }));
    } finally {
      setValidating(false);
    }
  }, [t]);

  return (
    <Modal open={open} onClose={onClose} title={t("settings.correctionRules")} data-testid="modal-correction-rules">
      <div className="lw-stack lw-stack--md">
        <Segmented
          value={filter}
          options={[
            { value: "all", label: t("settings.correctionFilterAll") },
            { value: "user", label: t("settings.correctionFilterUser") },
            { value: "ai", label: t("settings.correctionFilterAi") },
          ]}
          onChange={(v) => setFilter(v as "all" | "user" | "ai")}
          data-testid="correction-filter"
        />
        <TextInput
          value={search}
          onChange={setSearch}
          placeholder={t("settings.correctionSearchPlaceholder")}
          data-testid="correction-search"
        />
        <div className="lw-stack">
          {filtered.length === 0 ? (
            <div className="lw-field-hint">{t("settings.correctionEmpty")}</div>
          ) : (
            filtered.map((c) => (
              <div key={`${c.original}__${c.corrected}`} className="lw-settings-row">
                <div className="lw-settings-row-label">
                  <span className="lw-settings-row-main">{c.original} → {c.corrected}</span>
                  <span className="lw-settings-row-desc">{c.source}</span>
                </div>
                <IconButton
                  label={t("common.clear")}
                  icon={<X size={14} />}
                  onClick={() => void handleRemove(c)}
                  data-testid={`correction-delete-${c.original}__${c.corrected}`}
                />
              </div>
            ))
          )}
        </div>
        <div className="lw-stack lw-stack--md" style={{ borderTop: "1px solid var(--lw-border-subtle)", paddingTop: 12 }}>
          <Field label={t("settings.correctionValidationToggle")} hint={t("settings.correctionValidationHint")}>
            <Toggle
              checked={validationEnabled}
              onChange={(v) => void handleValidationToggle(v)}
              label={t("settings.correctionValidationToggle")}
              data-testid="correction-validation-toggle"
            />
          </Field>
          <Field label={t("settings.correctionValidationSeparateModel")}>
            <Toggle
              checked={separateModel}
              onChange={(v) => void handleSeparateToggle(v)}
              label={t("settings.correctionValidationSeparateModel")}
              data-testid="correction-validation-separate-toggle"
            />
          </Field>
          {separateModel && (
            <>
              <Field label={t("settings.provider")}>
                <Picker
                  value={validationProvider}
                  options={[
                    { value: "openai", label: "OpenAI" },
                    { value: "deepseek", label: "DeepSeek" },
                    { value: "cerebras", label: "Cerebras" },
                    { value: "siliconflow", label: "SiliconFlow" },
                  ]}
                  onChange={setValidationProvider}
                  data-testid="correction-validation-provider"
                />
              </Field>
              <Field label={t("settings.modelLabel")}>
                <TextInput
                  value={validationModel}
                  onChange={setValidationModel}
                  placeholder={t("settings.correctionValidationModelPlaceholder")}
                  data-testid="correction-validation-model"
                />
              </Field>
            </>
          )}
          <Button
            onClick={() => void handleValidate()}
            loading={validating}
            data-testid="correction-validate-btn"
          >
            {t("settings.correctionValidationRun")}
          </Button>
        </div>
      </div>
    </Modal>
  );
}
