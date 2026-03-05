import { useState, useEffect, useCallback, useRef } from "react";
import { ArrowLeft, Mic, Accessibility, Sun, Moon, Monitor, Power, Keyboard, ClipboardPaste, AudioLines, Zap, Sparkles, Eye, EyeOff, BookOpen, Plus, X, Download, Upload } from "lucide-react";
import { toast } from "sonner";
import { useTheme } from "@/hooks/useTheme";
import {
  disableAutostart,
  enableAutostart,
  getEngine,
  isAutostartEnabled,
  pasteText,
  setEngine,
  testMicrophone,
  setInputMethodCommand,
  setAiPolishConfig,
  getAiPolishApiKey,
  getUserProfile,
  addHotWord,
  removeHotWord,
  setLlmProviderConfig,
  exportUserProfile,
  importUserProfile,
  setSoundEnabled,
} from "@/api/tauri";
import type { UserProfile } from "@/types";
import { useRecordingContext } from "@/contexts/RecordingContext";
import TitleBar from "@/components/TitleBar";
import { PADDING, INPUT_METHOD_KEY, DEFAULT_HOTKEY, AI_POLISH_ENABLED_KEY, SOUND_ENABLED_KEY } from "@/lib/constants";
import {
  HOTKEY_MODIFIER_ORDER,
  type HotkeyModifier,
  formatHotkeyForDisplay,
  keyboardEventToHotkey,
  modifierFromKeyboardEvent,
} from "@/lib/hotkey";
import { readLocalStorage, writeLocalStorage } from "@/lib/storage";

const themeOptions = [
  { mode: "light" as const, icon: Sun, label: "浅色" },
  { mode: "dark" as const, icon: Moon, label: "深色" },
  { mode: "system" as const, icon: Monitor, label: "跟随系统" },
] as const;

const engineOptions = [
  { key: "sensevoice", icon: AudioLines, label: "SenseVoice", desc: "中英日韩粤，含标点" },
  { key: "whisper", icon: Zap, label: "Faster Whisper", desc: "99+语言，速度快" },
] as const;

const inputOptions = [
  { key: "sendInput" as const, icon: Keyboard, label: "直接输入", desc: "不占用剪贴板" },
  { key: "clipboard" as const, icon: ClipboardPaste, label: "剪贴板粘贴", desc: "兼容中文输入法" },
];

const llmProviderOptions = [
  { key: "cerebras", label: "Cerebras", desc: "GPT-OSS-120B, 极速" },
  { key: "deepseek", label: "DeepSeek", desc: "DeepSeek-Chat, 中文强" },
  { key: "custom", label: "自定义", desc: "OpenAI 兼容端点" },
];

const sourceLabels: Record<string, string> = {
  user: "手动",
  learned: "学习",
};

const sourceColors: Record<string, string> = {
  user: "var(--color-accent)",
  learned: "#10b981",
};

export default function SettingsPage({ onNavigate }: { onNavigate: (v: "main" | "settings") => void }) {
  const { isDark, theme, setTheme } = useTheme();
  const { retryModel, hotkeyDisplay, setHotkey, hotkeyError } = useRecordingContext();
  const [engine, setEngineState] = useState<string>("sensevoice");
  const [engineLoading, setEngineLoading] = useState(true);
  const [autostart, setAutostart] = useState(false);
  const [autostartLoading, setAutostartLoading] = useState(true);
  const [capturingHotkey, setCapturingHotkey] = useState(false);
  const [hotkeySaving, setHotkeySaving] = useState(false);
  const [inputMethod, setInputMethod] = useState<"sendInput" | "clipboard">(() => {
    return readLocalStorage(INPUT_METHOD_KEY) === "clipboard"
      ? "clipboard"
      : "sendInput";
  });
  const [soundEnabled, setSoundEnabledState] = useState(() => readLocalStorage(SOUND_ENABLED_KEY) !== "false");
  const [aiPolishEnabled, setAiPolishEnabled] = useState(() => readLocalStorage(AI_POLISH_ENABLED_KEY) === "true");
  const [aiPolishApiKey, setAiPolishApiKey] = useState("");
  const [showApiKey, setShowApiKey] = useState(false);
  const apiKeySaveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const llmConfigSaveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Agent profile state
  const [profile, setProfile] = useState<UserProfile | null>(null);
  const [newHotWord, setNewHotWord] = useState("");
  const [llmProvider, setLlmProvider] = useState("cerebras");
  const [customBaseUrl, setCustomBaseUrl] = useState("");
  const [customModel, setCustomModel] = useState("");

  const refreshAiPolishKey = useCallback(async (enabled = aiPolishEnabled) => {
    try {
      const key = (await getAiPolishApiKey()) || "";
      setAiPolishApiKey(key);
      await setAiPolishConfig(enabled, key).catch(() => {});
      return key;
    } catch {
      setAiPolishApiKey("");
      await setAiPolishConfig(enabled, "").catch(() => {});
      return "";
    }
  }, [aiPolishEnabled]);

  // 从系统密钥环加载 API Key，并同步 enabled 状态到后端
  useEffect(() => {
    void refreshAiPolishKey(readLocalStorage(AI_POLISH_ENABLED_KEY) === "true");
  }, [refreshAiPolishKey]);

  useEffect(() => {
    return () => {
      if (apiKeySaveTimer.current) {
        clearTimeout(apiKeySaveTimer.current);
        apiKeySaveTimer.current = null;
      }
      if (llmConfigSaveTimer.current) {
        clearTimeout(llmConfigSaveTimer.current);
        llmConfigSaveTimer.current = null;
      }
    };
  }, []);

  // 加载用户画像
  const refreshProfile = useCallback(() => {
    getUserProfile().then(p => {
      setProfile(p);
      setLlmProvider(p.llm_provider.active);
      setCustomBaseUrl(p.llm_provider.custom_base_url ?? "");
      setCustomModel(p.llm_provider.custom_model ?? "");
    }).catch(() => {});
  }, []);

  useEffect(() => {
    refreshProfile();
  }, [refreshProfile]);

  useEffect(() => {
    getEngine().then(e => {
      setEngineState(e);
      setEngineLoading(false);
    }).catch(() => setEngineLoading(false));
  }, []);

  const handleEngineSwitch = async (newEngine: string) => {
    if (engineLoading || newEngine === engine) return;
    setEngineLoading(true);
    try {
      await setEngine(newEngine);
      setEngineState(newEngine);
      toast.success(`已切换为 ${newEngine === "whisper" ? "Faster Whisper" : "SenseVoice"} 引擎`);
      retryModel();
    } catch {
      toast.error("切换引擎失败");
    } finally {
      setEngineLoading(false);
    }
  };

  useEffect(() => {
    isAutostartEnabled().then(enabled => {
      setAutostart(enabled);
      setAutostartLoading(false);
    }).catch(() => setAutostartLoading(false));
  }, []);

  const handleAutostartToggle = async () => {
    if (autostartLoading) return;
    const prev = autostart;
    // Optimistic update: toggle immediately, revert on failure
    setAutostart(!prev);
    setAutostartLoading(true);
    try {
      if (prev) {
        await disableAutostart();
        toast.success("已关闭开机自启动", { duration: 1100 });
      } else {
        await enableAutostart();
        toast.success("已开启开机自启动", { duration: 1100 });
      }
    } catch {
      setAutostart(prev); // revert
      toast.error("设置失败");
    } finally {
      setAutostartLoading(false);
    }
  };

  useEffect(() => {
    if (!capturingHotkey) return;

    const activeModifiers = new Set<HotkeyModifier>();
    let applied = false;
    const clearModifiers = () => {
      activeModifiers.clear();
    };

    const applyShortcut = (shortcut: string) => {
      if (applied) return;
      applied = true;
      setHotkeySaving(true);
      void setHotkey(shortcut)
        .then(() => {
          toast.success(`说话热键已设置为 ${formatHotkeyForDisplay(shortcut)}`);
        })
        .catch((err) => {
          const message = err instanceof Error ? err.message : "设置热键失败";
          toast.error(message);
        })
        .finally(() => {
          setHotkeySaving(false);
          setCapturingHotkey(false);
          clearModifiers();
        });
    };

    const onKeyDown = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();

      if (event.key === "Escape") {
        setCapturingHotkey(false);
        clearModifiers();
        return;
      }

      const modifier = modifierFromKeyboardEvent(event);
      if (modifier) {
        activeModifiers.add(modifier);
        return;
      }

      const shortcut = keyboardEventToHotkey(event, activeModifiers);
      if (!shortcut) return;

      applyShortcut(shortcut);
    };

    const onKeyUp = (event: KeyboardEvent) => {
      const modifier = modifierFromKeyboardEvent(event);
      if (!modifier || applied) return;

      const beforeRelease = HOTKEY_MODIFIER_ORDER
        .filter((key) => activeModifiers.has(key))
        .join("+");
      activeModifiers.delete(modifier);

      // Support modifier-only Ctrl+Win capture.
      if (beforeRelease === "Ctrl+Super") {
        applyShortcut("Ctrl+Super");
      }
    };

    const onVisibilityChange = () => {
      if (document.hidden) {
        clearModifiers();
      }
    };

    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    window.addEventListener("blur", clearModifiers);
    document.addEventListener("visibilitychange", onVisibilityChange);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
      window.removeEventListener("blur", clearModifiers);
      document.removeEventListener("visibilitychange", onVisibilityChange);
    };
  }, [capturingHotkey, setHotkey]);

  const handleResetHotkey = async () => {
    if (hotkeySaving) return;
    setHotkeySaving(true);
    try {
      await setHotkey(DEFAULT_HOTKEY);
      toast.success("已恢复默认热键 F2");
    } catch (err) {
      const message = err instanceof Error ? err.message : "恢复默认热键失败";
      toast.error(message);
    } finally {
      setHotkeySaving(false);
      setCapturingHotkey(false);
    }
  };

  const scheduleCustomLlmConfigSave = useCallback((baseUrl: string, model: string) => {
    if (llmConfigSaveTimer.current) {
      clearTimeout(llmConfigSaveTimer.current);
    }
    llmConfigSaveTimer.current = setTimeout(() => {
      setLlmProviderConfig("custom", baseUrl || undefined, model || undefined).catch(() => {});
    }, 400);
  }, []);

  const handleAddHotWord = useCallback(() => {
    const word = newHotWord.trim();
    if (!word) return;

    addHotWord(word, 3).then(() => {
      setNewHotWord("");
      refreshProfile();
      toast.success(`已添加热词: ${word}`);
    }).catch(() => toast.error("添加失败"));
  }, [newHotWord, refreshProfile]);

  return (
    <div className="page-root">

      <TitleBar
        title="设置"
        leftAction={
          <button aria-label="返回" className="icon-btn plain" onClick={() => onNavigate("main")}>
            <ArrowLeft size={14} strokeWidth={1.5} />
          </button>
        }
      />

      {/* Content */}
      <div className="settings-content" style={{ padding: `16px ${PADDING}px 16px` }}>
        <div className="settings-sections">

          {/* Appearance */}
          <section className="settings-card" style={{ animationDelay: "0ms" }}>
            <div className="settings-section-header">
              {isDark ? <Moon size={15} className="icon-accent" /> : <Sun size={15} className="icon-accent" />}
              <h2 className="settings-section-title">外观</h2>
            </div>
            <div className="settings-grid-3">
              {themeOptions.map(({ mode, icon: Icon, label }) => (
                <button
                  key={mode}
                  className="theme-btn settings-option-btn theme-option"
                  aria-label={`切换为${label}模式`}
                  aria-pressed={theme === mode}
                  onClick={() => setTheme(mode)}
                >
                  <Icon size={20} strokeWidth={1.5} />
                  <span className="settings-option-label">{label}</span>
                </button>
              ))}
            </div>
          </section>

          {/* Engine */}
          <section className="settings-card" style={{ animationDelay: "50ms" }}>
            <div className="settings-section-header">
              <AudioLines size={15} className="icon-accent" />
              <h2 className="settings-section-title">识别引擎</h2>
            </div>
            <div className="settings-grid-2">
              {engineOptions.map(({ key, icon: Icon, label, desc }) => (
                <button
                  key={key}
                  className="theme-btn settings-option-btn"
                  aria-label={label}
                  aria-pressed={engine === key}
                  disabled={engineLoading}
                  onClick={() => handleEngineSwitch(key)}
                >
                  <Icon size={20} strokeWidth={1.5} />
                  <span className="settings-option-label">{label}</span>
                  <span className="settings-option-desc">{desc}</span>
                </button>
              ))}
            </div>
          </section>

          {/* Hotkey */}
          <section className="settings-card" style={{ animationDelay: "100ms" }}>
            <div className="settings-section-header">
              <Keyboard size={15} className="icon-accent" />
              <h2 className="settings-section-title">说话热键</h2>
            </div>
            <div className="settings-column">
              <div className="settings-row" style={{ alignItems: "center", gap: 10 }}>
                <button
                  className="theme-btn hotkey-capture-btn"
                  onClick={() => setCapturingHotkey(true)}
                  disabled={hotkeySaving}
                  data-capturing={capturingHotkey}
                  style={{
                    cursor: hotkeySaving ? "wait" : "pointer",
                    opacity: hotkeySaving ? 0.7 : 1,
                  }}
                >
                  {capturingHotkey ? "请按下组合键..." : hotkeyDisplay}
                </button>
                <button
                  className="btn-ghost"
                  onClick={handleResetHotkey}
                  disabled={hotkeySaving}
                  style={{
                    fontSize: 12,
                    padding: "8px 10px",
                    cursor: hotkeySaving ? "wait" : "pointer",
                    opacity: hotkeySaving ? 0.7 : 1,
                  }}
                >
                  恢复 F2
                </button>
              </div>
              <p className="settings-hint">
                点击上方按钮后按下新热键，支持 Win 组合（如 Ctrl+Win+R），也支持纯 Ctrl+Win。按 Esc 取消设置。
              </p>
              {hotkeyError && <p className="settings-error">{hotkeyError}</p>}
            </div>
          </section>

          {/* Input Method */}
          <section className="settings-card" style={{ animationDelay: "150ms" }}>
            <div className="settings-section-header">
              <ClipboardPaste size={15} className="icon-accent" />
              <h2 className="settings-section-title">输入</h2>
            </div>
            <div className="settings-grid-2">
              {inputOptions.map(({ key, icon: Icon, label, desc }) => (
                <button
                  key={key}
                  className="theme-btn settings-option-btn"
                  aria-label={label}
                  aria-pressed={inputMethod === key}
                  onClick={() => {
                    setInputMethod(key);
                    writeLocalStorage(INPUT_METHOD_KEY, key);
                    setInputMethodCommand(key).catch(() => {});
                  }}
                >
                  <Icon size={20} strokeWidth={1.5} />
                  <span className="settings-option-label">{label}</span>
                  <span className="settings-option-desc">{desc}</span>
                </button>
              ))}
            </div>
            <div className="settings-row" style={{ marginTop: 6 }}>
              <span className="permission-label">录音提示音</span>
              <button
                role="switch"
                aria-checked={soundEnabled}
                aria-label="录音提示音"
                onClick={() => {
                  const next = !soundEnabled;
                  setSoundEnabledState(next);
                  writeLocalStorage(SOUND_ENABLED_KEY, String(next));
                  setSoundEnabled(next).catch(() => {});
                }}
                className="toggle-switch"
                style={{
                  background: soundEnabled ? "var(--color-accent)" : "var(--color-bg-tertiary)",
                }}
              >
                <div className="toggle-knob" style={{ transform: soundEnabled ? "translateX(20px)" : "translateX(0)" }} />
              </button>
            </div>
          </section>

          {/* AI Polish + LLM Backend */}
          <section className="settings-card" style={{ animationDelay: "200ms" }}>
            <div className="settings-section-header">
              <Sparkles size={15} className="icon-accent" />
              <h2 className="settings-section-title">AI 纠错</h2>
            </div>
            <div className="settings-column" style={{ gap: 10 }}>
              <div className="settings-row">
                <span className="permission-label">启用 AI 文本润色</span>
                <button
                  role="switch"
                  aria-checked={aiPolishEnabled}
                  aria-label="启用 AI 文本润色"
                  onClick={() => {
                    const next = !aiPolishEnabled;
                    setAiPolishEnabled(next);
                    writeLocalStorage(AI_POLISH_ENABLED_KEY, String(next));
                    setAiPolishConfig(next, aiPolishApiKey).catch(() => {});
                  }}
                  className="toggle-switch"
                  style={{
                    background: aiPolishEnabled ? "var(--color-accent)" : "var(--color-bg-tertiary)",
                  }}
                >
                  <div className="toggle-knob" style={{ transform: aiPolishEnabled ? "translateX(20px)" : "translateX(0)" }} />
                </button>
              </div>

              {/* LLM Backend Selection */}
              <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
                {llmProviderOptions.map(({ key, label, desc }) => (
                  <button
                    key={key}
                    className="theme-btn settings-option-btn"
                    aria-pressed={llmProvider === key}
                    onClick={async () => {
                      setLlmProvider(key);
                      await setLlmProviderConfig(key, customBaseUrl || undefined, customModel || undefined).catch(() => {});
                      await refreshAiPolishKey();
                    }}
                    style={{ flex: 1, minWidth: 90, padding: "6px 8px" }}
                  >
                    <span className="settings-option-label" style={{ fontSize: 12 }}>{label}</span>
                    <span className="settings-option-desc" style={{ fontSize: 10 }}>{desc}</span>
                  </button>
                ))}
              </div>

              {/* Custom endpoint fields */}
              {llmProvider === "custom" && (
                <>
                  <input
                    type="text"
                    className="settings-input"
                    placeholder="API Base URL (OpenAI 兼容)"
                    value={customBaseUrl}
                    onChange={(e) => {
                      const nextBaseUrl = e.target.value;
                      setCustomBaseUrl(nextBaseUrl);
                      scheduleCustomLlmConfigSave(nextBaseUrl, customModel);
                    }}
                    style={{
                      padding: "8px 10px", borderRadius: 8,
                      border: "1px solid var(--color-border)",
                      background: "var(--color-bg-secondary)",
                      color: "var(--color-text-primary)", fontSize: 13, outline: "none",
                    }}
                  />
                  <input
                    type="text"
                    className="settings-input"
                    placeholder="模型名 (如 gpt-3.5-turbo)"
                    value={customModel}
                    onChange={(e) => {
                      const nextModel = e.target.value;
                      setCustomModel(nextModel);
                      scheduleCustomLlmConfigSave(customBaseUrl, nextModel);
                    }}
                    style={{
                      padding: "8px 10px", borderRadius: 8,
                      border: "1px solid var(--color-border)",
                      background: "var(--color-bg-secondary)",
                      color: "var(--color-text-primary)", fontSize: 13, outline: "none",
                    }}
                  />
                </>
              )}

              <div className="settings-row" style={{ position: "relative" }}>
                <input
                  type={showApiKey ? "text" : "password"}
                  className="settings-input"
                  placeholder={`${llmProviderOptions.find(o => o.key === llmProvider)?.label ?? "LLM"} API Key`}
                  value={aiPolishApiKey}
                  onChange={(e) => {
                    const val = e.target.value;
                    setAiPolishApiKey(val);
                    if (apiKeySaveTimer.current) clearTimeout(apiKeySaveTimer.current);
                    apiKeySaveTimer.current = setTimeout(() => {
                      setAiPolishConfig(aiPolishEnabled, val).catch(() => {});
                    }, 600);
                  }}
                  style={{
                    flex: 1,
                    padding: "8px 36px 8px 10px",
                    borderRadius: 8,
                    border: "1px solid var(--color-border)",
                    background: "var(--color-bg-secondary)",
                    color: "var(--color-text-primary)",
                    fontSize: 13,
                    outline: "none",
                  }}
                />
                <button
                  className="icon-btn plain"
                  onClick={() => setShowApiKey(!showApiKey)}
                  style={{ position: "absolute", right: 4, top: "50%", transform: "translateY(-50%)" }}
                  aria-label={showApiKey ? "隐藏 API Key" : "显示 API Key"}
                >
                  {showApiKey ? <EyeOff size={14} /> : <Eye size={14} />}
                </button>
              </div>
              <p className="settings-hint">
                AI 纠错会自动学习你的用词习惯，并将常用词汇注入热词列表提升识别准确率。
              </p>
            </div>
          </section>

          {/* Smart Vocabulary */}
          <section className="settings-card" style={{ animationDelay: "225ms" }}>
            <div className="settings-section-header">
              <BookOpen size={15} className="icon-accent" />
              <h2 className="settings-section-title">智能词库</h2>
              {profile && (
                <span style={{ marginLeft: "auto", fontSize: 11, color: "var(--color-text-tertiary)" }}>
                  {profile.hot_words.length} 个热词 · {profile.total_transcriptions} 次转录
                </span>
              )}
            </div>
            <div className="settings-column" style={{ gap: 8 }}>
              {/* Add hot word */}
              <div style={{ display: "flex", gap: 6 }}>
                <input
                  type="text"
                  placeholder="添加热词 (如 Claude Code)"
                  value={newHotWord}
                  onChange={(e) => setNewHotWord(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && newHotWord.trim()) {
                      handleAddHotWord();
                    }
                  }}
                  style={{
                    flex: 1, padding: "7px 10px", borderRadius: 8,
                    border: "1px solid var(--color-border)",
                    background: "var(--color-bg-secondary)",
                    color: "var(--color-text-primary)", fontSize: 13, outline: "none",
                  }}
                />
                <button
                  className="test-btn"
                  onClick={() => {
                    handleAddHotWord();
                  }}
                  style={{ padding: "7px 12px" }}
                >
                  <Plus size={14} />
                </button>
              </div>

              {/* Hot word list */}
              {profile && profile.hot_words.length > 0 && (
                <div style={{
                  display: "flex", flexWrap: "wrap", gap: 4,
                  maxHeight: 120, overflow: "auto",
                  padding: "4px 0",
                }}>
                  {[...profile.hot_words]
                    .sort((a, b) => b.weight - a.weight || b.use_count - a.use_count)
                    .map((hw) => (
                    <span
                      key={hw.text}
                      style={{
                        display: "inline-flex", alignItems: "center", gap: 4,
                        padding: "3px 8px", borderRadius: 12,
                        background: "var(--color-bg-secondary)",
                        border: `1px solid ${sourceColors[hw.source] ?? "var(--color-border)"}`,
                        fontSize: 12, color: "var(--color-text-secondary)",
                      }}
                    >
                      <span style={{
                        width: 6, height: 6, borderRadius: "50%",
                        background: sourceColors[hw.source] ?? "var(--color-border)",
                        flexShrink: 0,
                      }} />
                      {hw.text}
                      <button
                        onClick={() => {
                          removeHotWord(hw.text).then(() => refreshProfile()).catch(() => {});
                        }}
                        style={{
                          background: "none", border: "none", cursor: "pointer",
                          color: "var(--color-text-tertiary)", padding: 0,
                          display: "flex", alignItems: "center",
                        }}
                      >
                        <X size={10} />
                      </button>
                    </span>
                  ))}
                </div>
              )}

              {/* Legend + actions */}
              <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
                {Object.entries(sourceLabels).map(([key, label]) => (
                  <span key={key} style={{ display: "flex", alignItems: "center", gap: 3, fontSize: 11, color: "var(--color-text-tertiary)" }}>
                    <span style={{ width: 6, height: 6, borderRadius: "50%", background: sourceColors[key] }} />
                    {label}
                  </span>
                ))}
                <span style={{ flex: 1 }} />
              </div>
            </div>
          </section>

          {/* Profile Export/Import */}
          <section className="settings-card" style={{ animationDelay: "255ms" }}>
            <div className="settings-section-header">
              <Download size={15} className="icon-accent" />
              <h2 className="settings-section-title">数据</h2>
            </div>
            <div style={{ display: "flex", gap: 6 }}>
              <button
                className="btn-ghost"
                onClick={async () => {
                  try {
                    const data = await exportUserProfile();
                    const blob = new Blob([data], { type: "application/json" });
                    const url = URL.createObjectURL(blob);
                    const a = document.createElement("a");
                    a.href = url;
                    a.download = "light-whisper-profile.json";
                    a.click();
                    setTimeout(() => URL.revokeObjectURL(url), 200);
                    toast.success("画像已导出");
                  } catch { toast.error("导出失败"); }
                }}
                style={{ flex: 1, fontSize: 12, padding: "8px" }}
              >
                <Download size={13} style={{ marginRight: 4 }} />导出画像
              </button>
              <button
                className="btn-ghost"
                onClick={() => {
                  const input = document.createElement("input");
                  input.type = "file";
                  input.accept = ".json";
                  input.onchange = async (e) => {
                    const file = (e.target as HTMLInputElement).files?.[0];
                    if (!file) return;
                    try {
                      const text = await file.text();
                      await importUserProfile(text);
                      refreshProfile();
                      await refreshAiPolishKey();
                      toast.success("画像已导入");
                    } catch { toast.error("导入失败，请检查文件格式"); }
                  };
                  input.click();
                }}
                style={{ flex: 1, fontSize: 12, padding: "8px" }}
              >
                <Upload size={13} style={{ marginRight: 4 }} />导入画像
              </button>
            </div>
          </section>

          {/* Permissions */}
          <section className="settings-card" style={{ animationDelay: "250ms" }}>
            <div className="settings-section-header">
              <Accessibility size={15} className="icon-accent" />
              <h2 className="settings-section-title">权限</h2>
            </div>
            <div className="permission-list">
              <div className="settings-row">
                <div className="permission-item">
                  <Mic size={14} className="icon-tertiary" />
                  <span className="permission-label">麦克风</span>
                </div>
                <button className="test-btn" onClick={async () => {
                  try {
                    const msg = await testMicrophone();
                    toast.success(msg);
                  } catch { toast.error("麦克风测试失败"); }
                }}>测试</button>
              </div>
              <div className="settings-row">
                <div className="permission-item">
                  <Accessibility size={14} className="icon-tertiary" />
                  <span className="permission-label">辅助功能 / 粘贴</span>
                </div>
                <button className="test-btn" onClick={async () => {
                  try {
                    await pasteText("测试粘贴", inputMethod);
                    toast.success("粘贴功能正常");
                  } catch { toast.error("粘贴功能异常"); }
                }}>测试</button>
              </div>
            </div>
          </section>

          {/* Startup */}
          <section className="settings-card" style={{ animationDelay: "300ms" }}>
            <div className="settings-section-header">
              <Power size={15} className="icon-accent" />
              <h2 className="settings-section-title">启动</h2>
            </div>
            <div className="settings-row">
              <span className="permission-label">开机自启动</span>
              <button
                role="switch"
                aria-checked={autostart}
                aria-label="开机自启动"
                onClick={handleAutostartToggle}
                className="toggle-switch"
                style={{
                  background: autostart ? "var(--color-accent)" : "var(--color-bg-tertiary)",
                }}
              >
                <div className="toggle-knob" style={{ transform: autostart ? "translateX(20px)" : "translateX(0)" }} />
              </button>
            </div>
          </section>
        </div>
      </div>

      {/* Footer */}
      <div className="settings-footer" style={{ padding: `10px ${PADDING}px` }}>
        <p className="settings-footer-text">
          轻语 Whisper <span className="settings-footer-version">v1.0.0</span>
          <span style={{ margin: "0 6px" }}>·</span>
          本地语音转文字
        </p>
      </div>
    </div>
  );
}
