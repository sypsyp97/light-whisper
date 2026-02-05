import { useState, useEffect } from "react";
import { ArrowLeft, Mic, Accessibility, Sun, Moon, Monitor, Power } from "lucide-react";
import { toast } from "sonner";
import { useTheme } from "@/hooks/useTheme";
import { useWindowDrag } from "@/hooks/useWindowDrag";
import { enableAutostart, disableAutostart, isAutostartEnabled } from "@/api/autostart";

const PADDING = 24;

const testBtnStyle: React.CSSProperties = {
  padding: "8px 16px",
  borderRadius: 4,
  fontSize: 12,
  fontWeight: 500,
  background: "var(--color-bg-secondary)",
  color: "var(--color-text-secondary)",
  border: "1px solid var(--color-border-subtle)",
  cursor: "pointer",
  transition: "all 0.15s ease",
};

export default function SettingsPage({ onNavigate }: { onNavigate: (v: "main" | "settings") => void }) {
  const { isDark, theme, setTheme } = useTheme();
  const { startDrag } = useWindowDrag();
  const [autostart, setAutostart] = useState(false);
  const [autostartLoading, setAutostartLoading] = useState(true);

  useEffect(() => {
    isAutostartEnabled().then(enabled => {
      setAutostart(enabled);
      setAutostartLoading(false);
    }).catch(() => setAutostartLoading(false));
  }, []);

  const handleAutostartToggle = async () => {
    setAutostartLoading(true);
    try {
      if (autostart) {
        await disableAutostart();
        setAutostart(false);
        toast.success("已关闭开机自启动");
      } else {
        await enableAutostart();
        setAutostart(true);
        toast.success("已开启开机自启动");
      }
    } catch {
      toast.error("设置失败");
    } finally {
      setAutostartLoading(false);
    }
  };

  const themeOptions = [
    { mode: "light" as const, icon: Sun, label: "浅色" },
    { mode: "dark" as const, icon: Moon, label: "深色" },
    { mode: "system" as const, icon: Monitor, label: "跟随系统" },
  ];

  return (
    <div style={{ height: "100vh", width: "100vw", display: "flex", flexDirection: "column", overflow: "hidden", userSelect: "none", color: "var(--color-text-primary)" }}>

      {/* Title bar */}
      <header onMouseDown={startDrag} style={{ display: "flex", alignItems: "center", padding: `0 ${PADDING - 8}px`, height: 36, flexShrink: 0, borderBottom: "1px solid var(--color-border-subtle)", background: "var(--color-bg-overlay)", backdropFilter: "blur(8px)" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }} onMouseDown={e => e.stopPropagation()}>
          <button aria-label="返回" onClick={() => onNavigate("main")} style={{ padding: 8, borderRadius: 4, border: "none", background: "transparent", color: "var(--color-text-tertiary)", cursor: "pointer", display: "flex", alignItems: "center", justifyContent: "center" }}>
            <ArrowLeft size={14} strokeWidth={1.5} />
          </button>
          <span style={{ fontSize: 13, fontWeight: 600, letterSpacing: "0.01em", fontFamily: "var(--font-display)" }}>设置</span>
        </div>
      </header>

      {/* Content */}
      <div style={{ flex: 1, overflowY: "auto", minHeight: 0, padding: `20px ${PADDING}px 16px` }}>
        <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>

          {/* Appearance */}
          <section>
            <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 12 }}>
              {isDark ? <Moon size={15} style={{ color: "var(--color-accent)" }} /> : <Sun size={15} style={{ color: "var(--color-accent)" }} />}
              <h2 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>外观</h2>
            </div>
            <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: 10 }}>
              {themeOptions.map(({ mode, icon: Icon, label }) => (
                <button
                  key={mode}
                  onClick={() => setTheme(mode)}
                  style={{
                    display: "flex", flexDirection: "column", alignItems: "center", gap: 8,
                    padding: "14px 12px", borderRadius: 6,
                    border: `1px solid ${theme === mode ? "var(--color-border-accent)" : "var(--color-border-subtle)"}`,
                    background: theme === mode ? "var(--color-accent-subtle)" : "var(--color-bg-elevated)",
                    color: theme === mode ? "var(--color-accent)" : "var(--color-text-tertiary)",
                    cursor: "pointer", transition: "all 0.15s ease",
                  }}
                >
                  <Icon size={20} strokeWidth={1.5} />
                  <span style={{ fontSize: 12, fontWeight: 500 }}>{label}</span>
                </button>
              ))}
            </div>
          </section>

          <div style={{ height: 1, background: "var(--color-border-subtle)" }} />

          {/* Permissions */}
          <section>
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
                <button onClick={async () => {
                  try {
                    const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
                    stream.getTracks().forEach(t => t.stop());
                    toast.success("麦克风权限正常");
                  } catch { toast.error("麦克风权限未授予"); }
                }} style={testBtnStyle}>测试</button>
              </div>
              <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
                <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                  <Accessibility size={14} style={{ color: "var(--color-text-tertiary)" }} />
                  <span style={{ fontSize: 13, color: "var(--color-text-secondary)" }}>辅助功能 / 粘贴</span>
                </div>
                <button onClick={async () => {
                  try {
                    const { pasteText } = await import("@/api/clipboard");
                    await pasteText("测试粘贴");
                    toast.success("粘贴功能正常");
                  } catch { toast.error("粘贴功能异常"); }
                }} style={testBtnStyle}>测试</button>
              </div>
            </div>
          </section>

          <div style={{ height: 1, background: "var(--color-border-subtle)" }} />

          {/* Startup */}
          <section>
            <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 12 }}>
              <Power size={15} style={{ color: "var(--color-accent)" }} />
              <h2 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>启动</h2>
            </div>
            <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
              <span style={{ fontSize: 13, color: "var(--color-text-secondary)" }}>开机自启动</span>
              <button
                onClick={handleAutostartToggle}
                disabled={autostartLoading}
                style={{
                  width: 44, height: 24, borderRadius: 12, border: "none", padding: 2,
                  background: autostart ? "var(--color-accent)" : "var(--color-bg-tertiary)",
                  cursor: autostartLoading ? "wait" : "pointer",
                  transition: "background 0.2s ease",
                  opacity: autostartLoading ? 0.6 : 1,
                }}
              >
                <div style={{
                  width: 20, height: 20, borderRadius: "50%",
                  background: "white", boxShadow: "0 1px 3px rgba(0,0,0,0.2)",
                  transform: autostart ? "translateX(20px)" : "translateX(0)",
                  transition: "transform 0.2s ease",
                }} />
              </button>
            </div>
          </section>
        </div>
      </div>

      {/* Footer */}
      <div style={{ flexShrink: 0, padding: `14px ${PADDING}px`, borderTop: "1px solid var(--color-border-subtle)", textAlign: "center" }}>
        <p style={{ fontSize: 12, color: "var(--color-text-tertiary)", margin: 0 }}>
          轻语 Whisper <span style={{ marginLeft: 4, fontSize: 11 }}>v0.1.0</span>
          <span style={{ margin: "0 6px" }}>·</span>
          本地语音转文字
        </p>
      </div>
    </div>
  );
}
