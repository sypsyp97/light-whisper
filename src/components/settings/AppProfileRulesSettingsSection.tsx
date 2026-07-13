import { useEffect, useState } from "react";
import { AppWindow, ArrowDown, ArrowUp, Pencil, Plus, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";

import { setAppProfileRules } from "@/api/tauri";
import type {
  AppProfileRule,
  AppRuleOverride,
  AppTranslationOverride,
  UserProfile,
} from "@/types";

interface AppProfileRulesSettingsSectionProps {
  profile: UserProfile | null;
  onSaved: () => void;
}

function createRule(): AppProfileRule {
  return {
    id: crypto.randomUUID(),
    name: "",
    enabled: true,
    process_name: "",
    window_title_contains: null,
    ai_polish: "inherit",
    translation: "inherit",
    translation_target: null,
    screen_context: "inherit",
    history: "inherit",
    custom_prompt: null,
  };
}

function OverrideSelect({ value, onChange }: {
  value: AppRuleOverride;
  onChange: (value: AppRuleOverride) => void;
}) {
  const { t } = useTranslation();
  return (
    <select
      className="settings-input app-rule-select"
      value={value}
      onChange={(event) => onChange(event.target.value as AppRuleOverride)}
    >
      <option value="inherit">{t("settings.appRuleInherit")}</option>
      <option value="enabled">{t("settings.appRuleEnabled")}</option>
      <option value="disabled">{t("settings.appRuleDisabled")}</option>
    </select>
  );
}

function RuleEditor({ value, onChange, onCancel, onSubmit }: {
  value: AppProfileRule;
  onChange: (rule: AppProfileRule) => void;
  onCancel: () => void;
  onSubmit: () => void;
}) {
  const { t } = useTranslation();
  const patch = (updates: Partial<AppProfileRule>) => onChange({ ...value, ...updates });

  return (
    <div className="app-rule-editor">
      <div className="app-rule-editor-grid">
        <label className="settings-column app-rule-field">
          <span>{t("settings.appRuleName")}</span>
          <input
            className="settings-input"
            value={value.name}
            placeholder={t("settings.appRuleNamePlaceholder")}
            onChange={(event) => patch({ name: event.target.value })}
          />
        </label>
        <label className="settings-column app-rule-field">
          <span>{t("settings.appRuleProcess")}</span>
          <input
            className="settings-input"
            value={value.process_name}
            placeholder="Code.exe"
            autoFocus
            onChange={(event) => patch({ process_name: event.target.value })}
          />
        </label>
      </div>

      <label className="settings-column app-rule-field">
        <span>{t("settings.appRuleWindowTitle")}</span>
        <input
          className="settings-input"
          value={value.window_title_contains ?? ""}
          placeholder={t("settings.appRuleWindowTitlePlaceholder")}
          onChange={(event) => patch({ window_title_contains: event.target.value || null })}
        />
      </label>

      <div className="app-rule-editor-grid">
        <label className="settings-column app-rule-field">
          <span>{t("settings.appRuleAiPolish")}</span>
          <OverrideSelect value={value.ai_polish} onChange={(ai_polish) => patch({ ai_polish })} />
        </label>
        <label className="settings-column app-rule-field">
          <span>{t("settings.appRuleScreenContext")}</span>
          <OverrideSelect value={value.screen_context} onChange={(screen_context) => patch({ screen_context })} />
        </label>
        <label className="settings-column app-rule-field">
          <span>{t("settings.appRuleHistory")}</span>
          <OverrideSelect value={value.history} onChange={(history) => patch({ history })} />
        </label>
        <label className="settings-column app-rule-field">
          <span>{t("settings.appRuleTranslation")}</span>
          <select
            className="settings-input app-rule-select"
            value={value.translation}
            onChange={(event) => patch({ translation: event.target.value as AppTranslationOverride })}
          >
            <option value="inherit">{t("settings.appRuleInherit")}</option>
            <option value="disabled">{t("settings.appRuleDisabled")}</option>
            <option value="target">{t("settings.appRuleTranslationTarget")}</option>
          </select>
        </label>
      </div>

      {value.translation === "target" && (
        <label className="settings-column app-rule-field">
          <span>{t("settings.appRuleTargetLanguage")}</span>
          <input
            className="settings-input"
            value={value.translation_target ?? ""}
            placeholder={t("settings.appRuleTargetPlaceholder")}
            onChange={(event) => patch({ translation_target: event.target.value || null })}
          />
        </label>
      )}

      <label className="settings-column app-rule-field">
        <span>{t("settings.appRuleCustomPrompt")}</span>
        <textarea
          className="settings-input app-rule-textarea"
          value={value.custom_prompt ?? ""}
          placeholder={t("settings.appRuleCustomPromptPlaceholder")}
          onChange={(event) => patch({ custom_prompt: event.target.value || null })}
        />
      </label>

      <div className="app-rule-editor-actions">
        <button type="button" className="btn-ghost btn-ghost-sm" onClick={onCancel}>
          {t("common.cancel")}
        </button>
        <button
          type="button"
          className="test-btn"
          disabled={!value.process_name.trim() || (value.translation === "target" && !value.translation_target?.trim())}
          onClick={onSubmit}
        >
          {t("settings.appRuleSave")}
        </button>
      </div>
    </div>
  );
}

export default function AppProfileRulesSettingsSection({ profile, onSaved }: AppProfileRulesSettingsSectionProps) {
  const { t } = useTranslation();
  const [rules, setRules] = useState<AppProfileRule[]>([]);
  const [editing, setEditing] = useState<AppProfileRule | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setRules(profile?.app_profile_rules ?? []);
  }, [profile?.app_profile_rules]);

  const persist = async (next: AppProfileRule[]) => {
    const previous = rules;
    setRules(next);
    setSaving(true);
    try {
      await setAppProfileRules(next);
      onSaved();
      return true;
    } catch {
      setRules(previous);
      toast.error(t("settings.appRulesSaveFailed"));
      return false;
    } finally {
      setSaving(false);
    }
  };

  const saveEditor = async () => {
    if (!editing) return;
    const normalized: AppProfileRule = {
      ...editing,
      name: editing.name.trim(),
      process_name: editing.process_name.trim(),
      window_title_contains: editing.window_title_contains?.trim() || null,
      translation_target: editing.translation_target?.trim() || null,
      custom_prompt: editing.custom_prompt?.trim() || null,
    };
    const exists = rules.some((rule) => rule.id === normalized.id);
    const next = exists
      ? rules.map((rule) => rule.id === normalized.id ? normalized : rule)
      : [...rules, normalized];
    if (await persist(next)) setEditing(null);
  };

  const move = (index: number, offset: -1 | 1) => {
    const target = index + offset;
    if (target < 0 || target >= rules.length) return;
    const next = [...rules];
    [next[index], next[target]] = [next[target], next[index]];
    void persist(next);
  };

  return (
    <section className="settings-card" data-nav-id="app-profiles">
      <div className="settings-section-header app-rules-header">
        <AppWindow size={15} className="icon-accent" />
        <h2 className="settings-section-title">{t("settings.appProfiles")}</h2>
        <span className="app-rules-count">{rules.length}/100</span>
      </div>
      <div className="settings-column app-rules-column">
        <p className="settings-hint settings-hint-flush">{t("settings.appProfilesHint")}</p>

        {rules.length === 0 && !editing && (
          <div className="app-rules-empty">{t("settings.appRulesEmpty")}</div>
        )}

        {rules.map((rule, index) => (
          <div className="app-rule-card" key={rule.id} data-enabled={rule.enabled}>
            <button
              type="button"
              role="switch"
              aria-checked={rule.enabled}
              aria-label={t("settings.appRuleToggle", { name: rule.name || rule.process_name })}
              className="toggle-switch app-rule-toggle"
              data-active={rule.enabled}
              disabled={saving}
              onClick={() => {
                void persist(rules.map((item) => item.id === rule.id
                  ? { ...item, enabled: !item.enabled }
                  : item));
              }}
            >
              <div className="toggle-knob" />
            </button>
            <div className="app-rule-summary">
              <strong>{rule.name || rule.process_name}</strong>
              <span>{rule.process_name}{rule.window_title_contains ? ` · ${rule.window_title_contains}` : ""}</span>
            </div>
            <div className="app-rule-actions">
              <button type="button" className="btn-ghost app-rule-icon-button" aria-label={t("settings.appRuleMoveUp")} disabled={saving || index === 0} onClick={() => move(index, -1)}>
                <ArrowUp size={13} />
              </button>
              <button type="button" className="btn-ghost app-rule-icon-button" aria-label={t("settings.appRuleMoveDown")} disabled={saving || index === rules.length - 1} onClick={() => move(index, 1)}>
                <ArrowDown size={13} />
              </button>
              <button type="button" className="btn-ghost app-rule-icon-button" aria-label={t("settings.appRuleEdit")} disabled={saving} onClick={() => setEditing({ ...rule })}>
                <Pencil size={13} />
              </button>
              <button type="button" className="btn-ghost app-rule-icon-button app-rule-delete" aria-label={t("settings.appRuleDelete")} disabled={saving} onClick={() => { void persist(rules.filter((item) => item.id !== rule.id)); }}>
                <Trash2 size={13} />
              </button>
            </div>
          </div>
        ))}

        {editing ? (
          <RuleEditor
            value={editing}
            onChange={setEditing}
            onCancel={() => setEditing(null)}
            onSubmit={() => { void saveEditor(); }}
          />
        ) : (
          <button type="button" className="btn-ghost app-rule-add" disabled={saving || rules.length >= 100} onClick={() => setEditing(createRule())}>
            <Plus size={13} />
            {t("settings.appRuleAdd")}
          </button>
        )}
      </div>
    </section>
  );
}
