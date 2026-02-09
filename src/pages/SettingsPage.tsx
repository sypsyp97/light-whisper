import { useState, useEffect } from "react";
import { ArrowLeft, Mic, Accessibility, Sun, Moon, Monitor, Power, Keyboard, ClipboardPaste } from "lucide-react";
import { toast } from "sonner";
import { useTheme } from "@/hooks/useTheme";
import { enableAutostart, disableAutostart, isAutostartEnabled } from "@/api/autostart";
import TitleBar from "@/components/TitleBar";
import { PADDING, INPUT_METHOD_KEY } from "@/lib/constants";

const themeOptions = [
  { mode: "light" as const, icon: Sun, label: "浅色" },
  { mode: "dark" as const, icon: Moon, label: "深色" },
  { mode: "system" as const, icon: Monitor, label: "跟随系统" },
] as const;

export default function SettingsPage({ onNavigate }: { onNavigate: (v: "main" | "settings") => void }) {
  const { isDark, theme, setTheme } = useTheme();
  const [autostart, setAutostart] = useState(false);
  const [autostartLoading, setAutostartLoading] = useState(true);
  const [inputMethod, setInputMethod] = useState<"sendInput" | "clipboard">(() => {
    try {
      return (localStorage.getItem(INPUT_METHOD_KEY) as "sendInput" | "clipboard") || "sendInput";
    } catch {
      return "sendInput";
    }
  });

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
        toast.success("已关闭开机自启动");
      } else {
        await enableAutostart();
        toast.success("已开启开机自启动");
      }
    } catch {
      setAutostart(prev); // revert
      toast.error("设置失败");
    } finally {
      setAutostartLoading(false);
    }
  };

  return (
    <div style={{ height: "100vh", width: "100vw", display: "flex", flexDirection: "column", overflow: "hidden", userSelect: "none", color: "var(--color-text-primary)" }}>

      <TitleBar
        title="设置"
        leftAction={
          <button aria-label="返回" className="icon-btn plain" onClick={() => onNavigate("main")}>
            <ArrowLeft size={14} strokeWidth={1.5} />
          </button>
        }
      />

      {/* Content */}
      <div style={{ flex: 1, overflowY: "auto", minHeight: 0, padding: `16px ${PADDING}px 16px` }}>
        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>

          {/* Appearance */}
          <section className="settings-card" style={{ animationDelay: "0ms" }}>
            <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 12 }}>
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

          {/* Input Method */}
          <section className="settings-card" style={{ animationDelay: "50ms" }}>
            <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 12 }}>
              <Keyboard size={15} style={{ color: "var(--color-accent)" }} />
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
                    try { localStorage.setItem(INPUT_METHOD_KEY, key); } catch { /* localStorage 不可用 */ }
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
          <section className="settings-card" style={{ animationDelay: "100ms" }}>
            <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 12 }}>
              <Accessibility size={15} style={{ color: "var(--color-accent)" }} />
              <h2 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>权限</h2>
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
              <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
                <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                  <Mic size={14} style={{ color: "var(--color-text-tertiary)" }} />
                  <span style={{ fontSize: 13, color: "var(--color-text-secondary)" }}>麦克风</span>
                </div>
                <button className="test-btn" onClick={async () => {
                  try {
                    const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
                    stream.getTracks().forEach(t => t.stop());
                    toast.success("麦克风权限正常");
                  } catch { toast.error("麦克风权限未授予"); }
                }}>测试</button>
              </div>
              <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
                <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                  <Accessibility size={14} style={{ color: "var(--color-text-tertiary)" }} />
                  <span style={{ fontSize: 13, color: "var(--color-text-secondary)" }}>辅助功能 / 粘贴</span>
                </div>
                <button className="test-btn" onClick={async () => {
                  try {
                    const { pasteText } = await import("@/api/clipboard");
                    await pasteText("测试粘贴", inputMethod);
                    toast.success("粘贴功能正常");
                  } catch { toast.error("粘贴功能异常"); }
                }}>测试</button>
              </div>
            </div>
          </section>

          {/* Startup */}
          <section className="settings-card" style={{ animationDelay: "150ms" }}>
            <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 12 }}>
              <Power size={15} style={{ color: "var(--color-accent)" }} />
              <h2 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>启动</h2>
            </div>
            <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
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
      <div style={{ flexShrink: 0, padding: `10px ${PADDING}px`, borderTop: "1px solid var(--color-border-subtle)", textAlign: "center" }}>
        <p style={{ fontSize: 12, color: "var(--color-text-tertiary)", margin: 0 }}>
          轻语 Whisper <span style={{ marginLeft: 4, fontSize: 11 }}>v1.0.0</span>
          <span style={{ margin: "0 6px" }}>·</span>
          本地语音转文字
        </p>
      </div>
    </div>
  );
}
