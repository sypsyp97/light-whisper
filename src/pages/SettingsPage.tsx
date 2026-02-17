import { useState, useEffect } from "react";
import { ArrowLeft, Mic, Accessibility, Sun, Moon, Monitor, Power, Keyboard, ClipboardPaste, AudioLines, Zap } from "lucide-react";
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
} from "@/api/tauri";
import { useRecordingContext } from "@/contexts/RecordingContext";
import TitleBar from "@/components/TitleBar";
import { PADDING, INPUT_METHOD_KEY, DEFAULT_HOTKEY } from "@/lib/constants";
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
        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>

          {/* Appearance */}
          <section className="settings-card" style={{ animationDelay: "0ms" }}>
            <div className="settings-section-header">
              {isDark ? <Moon size={15} style={{ color: "var(--color-accent)" }} /> : <Sun size={15} style={{ color: "var(--color-accent)" }} />}
              <h2 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>外观</h2>
            </div>
            <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: 10 }}>
              {themeOptions.map(({ mode, icon: Icon, label }) => (
                <button
                  key={mode}
                  className="theme-btn"
                  aria-label={`切换为${label}模式`}
                  aria-pressed={theme === mode}
                  onClick={() => setTheme(mode)}
                  style={{
                    display: "flex", flexDirection: "column", alignItems: "center", gap: 8,
                    padding: "10px 10px", borderRadius: 6,
                    border: `1px solid ${theme === mode ? "var(--color-border-accent)" : "var(--color-border-subtle)"}`,
                    background: theme === mode ? "var(--color-accent-subtle)" : "var(--color-bg-elevated)",
                    color: theme === mode ? "var(--color-accent)" : "var(--color-text-tertiary)",
                    cursor: "pointer",
                  }}
                >
                  <Icon size={20} strokeWidth={1.5} />
                  <span style={{ fontSize: 12, fontWeight: 500 }}>{label}</span>
                </button>
              ))}
            </div>
          </section>

          {/* Engine */}
          <section className="settings-card" style={{ animationDelay: "50ms" }}>
            <div className="settings-section-header">
              <AudioLines size={15} style={{ color: "var(--color-accent)" }} />
              <h2 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>识别引擎</h2>
            </div>
            <div style={{ display: "grid", gridTemplateColumns: "repeat(2, 1fr)", gap: 10 }}>
              {([
                { key: "sensevoice", icon: AudioLines, label: "SenseVoice", desc: "中英日韩粤，含标点" },
                { key: "whisper", icon: Zap, label: "Faster Whisper", desc: "99+语言，速度快" },
              ] as const).map(({ key, icon: Icon, label, desc }) => (
                <button
                  key={key}
                  className="theme-btn"
                  aria-label={label}
                  aria-pressed={engine === key}
                  disabled={engineLoading}
                  onClick={() => handleEngineSwitch(key)}
                  style={{
                    display: "flex", flexDirection: "column", alignItems: "center", gap: 6,
                    padding: "10px 10px", borderRadius: 6,
                    border: `1px solid ${engine === key ? "var(--color-border-accent)" : "var(--color-border-subtle)"}`,
                    background: engine === key ? "var(--color-accent-subtle)" : "var(--color-bg-elevated)",
                    color: engine === key ? "var(--color-accent)" : "var(--color-text-tertiary)",
                    cursor: engineLoading ? "wait" : "pointer",
                    opacity: engineLoading ? 0.6 : 1,
                  }}
                >
                  <Icon size={20} strokeWidth={1.5} />
                  <span style={{ fontSize: 12, fontWeight: 500 }}>{label}</span>
                  <span style={{ fontSize: 11, color: "var(--color-text-tertiary)" }}>{desc}</span>
                </button>
              ))}
            </div>
          </section>

          {/* Hotkey */}
          <section className="settings-card" style={{ animationDelay: "100ms" }}>
            <div className="settings-section-header">
              <Keyboard size={15} style={{ color: "var(--color-accent)" }} />
              <h2 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>说话热键</h2>
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
              <div className="settings-row" style={{ alignItems: "center", gap: 10 }}>
                <button
                  className="theme-btn"
                  onClick={() => setCapturingHotkey(true)}
                  disabled={hotkeySaving}
                  style={{
                    flex: 1,
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    padding: "10px 12px",
                    borderRadius: 6,
                    border: `1px solid ${capturingHotkey ? "var(--color-border-accent)" : "var(--color-border-subtle)"}`,
                    background: capturingHotkey ? "var(--color-accent-subtle)" : "var(--color-bg-elevated)",
                    color: capturingHotkey ? "var(--color-accent)" : "var(--color-text-secondary)",
                    cursor: hotkeySaving ? "wait" : "pointer",
                    opacity: hotkeySaving ? 0.7 : 1,
                    fontSize: 12,
                    fontWeight: 600,
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
              <p style={{ fontSize: 11, color: "var(--color-text-tertiary)", lineHeight: 1.5, margin: 0 }}>
                点击上方按钮后按下新热键，支持 Win 组合（如 Ctrl+Win+R），也支持纯 Ctrl+Win。按 Esc 取消设置。
              </p>
              {hotkeyError && (
                <p style={{ fontSize: 11, color: "var(--color-error)", lineHeight: 1.5, margin: 0 }}>
                  {hotkeyError}
                </p>
              )}
            </div>
          </section>

          {/* Input Method */}
          <section className="settings-card" style={{ animationDelay: "150ms" }}>
            <div className="settings-section-header">
              <ClipboardPaste size={15} style={{ color: "var(--color-accent)" }} />
              <h2 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>输入</h2>
            </div>
            <div style={{ display: "grid", gridTemplateColumns: "repeat(2, 1fr)", gap: 10 }}>
              {([
                { key: "sendInput" as const, icon: Keyboard, label: "直接输入", desc: "不占用剪贴板" },
                { key: "clipboard" as const, icon: ClipboardPaste, label: "剪贴板粘贴", desc: "兼容中文输入法" },
              ]).map(({ key, icon: Icon, label, desc }) => (
                <button
                  key={key}
                  className="theme-btn"
                  aria-label={label}
                  aria-pressed={inputMethod === key}
                  onClick={() => {
                    setInputMethod(key);
                    writeLocalStorage(INPUT_METHOD_KEY, key);
                    setInputMethodCommand(key).catch(() => {});
                  }}
                  style={{
                    display: "flex", flexDirection: "column", alignItems: "center", gap: 6,
                    padding: "10px 10px", borderRadius: 6,
                    border: `1px solid ${inputMethod === key ? "var(--color-border-accent)" : "var(--color-border-subtle)"}`,
                    background: inputMethod === key ? "var(--color-accent-subtle)" : "var(--color-bg-elevated)",
                    color: inputMethod === key ? "var(--color-accent)" : "var(--color-text-tertiary)",
                    cursor: "pointer",
                  }}
                >
                  <Icon size={20} strokeWidth={1.5} />
                  <span style={{ fontSize: 12, fontWeight: 500 }}>{label}</span>
                  <span style={{ fontSize: 11, color: "var(--color-text-tertiary)" }}>{desc}</span>
                </button>
              ))}
            </div>
          </section>

          {/* Permissions */}
          <section className="settings-card" style={{ animationDelay: "200ms" }}>
            <div className="settings-section-header">
              <Accessibility size={15} style={{ color: "var(--color-accent)" }} />
              <h2 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>权限</h2>
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
              <div className="settings-row">
                <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                  <Mic size={14} style={{ color: "var(--color-text-tertiary)" }} />
                  <span style={{ fontSize: 13, color: "var(--color-text-secondary)" }}>麦克风</span>
                </div>
                <button className="test-btn" onClick={async () => {
                  try {
                    const msg = await testMicrophone();
                    toast.success(msg);
                  } catch { toast.error("麦克风测试失败"); }
                }}>测试</button>
              </div>
              <div className="settings-row">
                <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                  <Accessibility size={14} style={{ color: "var(--color-text-tertiary)" }} />
                  <span style={{ fontSize: 13, color: "var(--color-text-secondary)" }}>辅助功能 / 粘贴</span>
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
          <section className="settings-card" style={{ animationDelay: "250ms" }}>
            <div className="settings-section-header">
              <Power size={15} style={{ color: "var(--color-accent)" }} />
              <h2 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>启动</h2>
            </div>
            <div className="settings-row">
              <span style={{ fontSize: 13, color: "var(--color-text-secondary)" }}>开机自启动</span>
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
        <p style={{ fontSize: 12, color: "var(--color-text-tertiary)", margin: 0 }}>
          轻语 Whisper <span style={{ marginLeft: 4, fontSize: 11 }}>v1.0.0</span>
          <span style={{ margin: "0 6px" }}>·</span>
          本地语音转文字
        </p>
      </div>
    </div>
  );
}
