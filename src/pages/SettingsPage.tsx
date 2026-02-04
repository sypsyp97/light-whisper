import {
  ArrowLeft,
  Mic,
  Accessibility,
  Sun,
  Moon,
  Monitor,
} from "lucide-react";

import { toast } from "sonner";
import { cn } from "@/lib/utils";
import { useTheme } from "@/hooks/useTheme";
import { useWindowDrag } from "@/hooks/useWindowDrag";

/* ── Settings page (inside 400x500 main window, decorations:false) ── */
export default function SettingsPage({ onNavigate }: { onNavigate: (v: "main" | "settings") => void }) {
  const { isDark, theme, setTheme } = useTheme();
  const { startDrag } = useWindowDrag();

  return (
    <div className="h-screen w-screen flex flex-col overflow-hidden select-none text-[var(--color-text-primary)]">

      {/* ═══ Title bar — 32px, same as main page ═══ */}
      <header
        className="relative z-20 flex items-center justify-between px-3 h-8 shrink-0 border-b border-[var(--color-border-subtle)] bg-[var(--color-bg-overlay)] backdrop-blur-sm"
        onMouseDown={startDrag}
      >
        <div className="flex items-center gap-1.5" onMouseDown={(e) => e.stopPropagation()}>
          <button
            onClick={() => onNavigate("main")}
            className="p-1 rounded text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)] hover:bg-[var(--color-accent-muted)] transition-colors"
            title="返回"
          >
            <ArrowLeft size={12} strokeWidth={1.8} />
          </button>
          <span
            className="text-[12px] font-semibold tracking-[0.02em] text-[var(--color-text-primary)]"
            style={{ fontFamily: "var(--font-display)" }}
          >
            设置
          </span>
        </div>
      </header>

      {/* ═══ Scrollable content ═══ */}
      <div className="flex-1 overflow-y-auto min-h-0 px-4 py-4">
        <div className="space-y-5">

          {/* ── ASR Engine ── */}
          <section className="space-y-3">
            <div className="flex items-center gap-2">
              <Mic size={14} className="text-[var(--color-accent)]" />
              <h2 className="text-[14px] font-semibold text-[var(--color-text-primary)]">识别模型</h2>
            </div>

            <div className="rounded-lg border border-[var(--color-border-subtle)] bg-[var(--color-bg-elevated)] p-3">
              <div className="text-[12px] font-semibold text-[var(--color-text-primary)]">
                Paraformer（固定）
              </div>
              <p className="text-[11px] leading-[1.5] text-[var(--color-text-tertiary)] mt-1">
                官方推荐的离线识别模型，内置 VAD 与标点恢复。
              </p>
            </div>
            <p className="text-[11px] leading-[1.5] text-[var(--color-text-tertiary)]">
              识别引擎已固定为 Paraformer，不再提供切换选项。
            </p>
          </section>

          <hr className="divider" />

          {/* ── Appearance ── */}
          <section className="space-y-3">
            <div className="flex items-center gap-2">
              {isDark ? <Moon size={14} className="text-[var(--color-accent)]" /> : <Sun size={14} className="text-[var(--color-accent)]" />}
              <h2 className="text-[14px] font-semibold text-[var(--color-text-primary)]">外观</h2>
            </div>

            <div className="grid grid-cols-3 gap-3">
              {([
                { mode: "light" as const, icon: Sun, label: "浅色" },
                { mode: "dark" as const, icon: Moon, label: "深色" },
                { mode: "system" as const, icon: Monitor, label: "跟随系统" },
              ]).map(({ mode, icon: Icon, label }) => (
                <button
                  key={mode}
                  onClick={() => setTheme(mode)}
                  className={cn(
                    "flex flex-col items-center gap-1.5 py-3 rounded-lg border transition-all",
                    theme === mode
                      ? "bg-[var(--color-accent-subtle)] border-[var(--color-border-accent)] text-[var(--color-accent)]"
                      : "bg-[var(--color-bg-elevated)] border-[var(--color-border-subtle)] text-[var(--color-text-tertiary)] hover:text-[var(--color-text-secondary)]"
                  )}
                >
                  <Icon size={18} strokeWidth={1.5} />
                  <span className="text-[11px] font-medium">{label}</span>
                </button>
              ))}
            </div>
          </section>

          <hr className="divider" />

          {/* ── Permissions ── */}
          <section className="space-y-3">
            <div className="flex items-center gap-2">
              <Accessibility size={14} className="text-[var(--color-accent)]" />
              <h2 className="text-[14px] font-semibold text-[var(--color-text-primary)]">权限</h2>
            </div>

            <div className="space-y-3">
              <div className="flex items-center justify-between py-1">
                <div className="flex items-center gap-2.5">
                  <Mic size={13} className="text-[var(--color-text-tertiary)]" />
                  <span className="text-[13px] text-[var(--color-text-secondary)]">麦克风</span>
                </div>
                <button
                  onClick={async () => {
                    try {
                      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
                      stream.getTracks().forEach((t) => t.stop());
                      toast.success("麦克风权限正常");
                    } catch { toast.error("麦克风权限未授予"); }
                  }}
                  className="px-3 py-1 rounded text-[11px] font-medium bg-[var(--color-bg-secondary)] text-[var(--color-text-secondary)] border border-[var(--color-border-subtle)] hover:bg-[var(--color-accent-subtle)] hover:text-[var(--color-accent)] transition-colors"
                >
                  测试
                </button>
              </div>
              <div className="flex items-center justify-between py-1">
                <div className="flex items-center gap-2.5">
                  <Accessibility size={13} className="text-[var(--color-text-tertiary)]" />
                  <span className="text-[13px] text-[var(--color-text-secondary)]">辅助功能 / 粘贴</span>
                </div>
                <button
                  onClick={async () => {
                    try {
                      const { pasteText } = await import("@/api/clipboard");
                      await pasteText("测试粘贴");
                      toast.success("粘贴功能正常");
                    } catch { toast.error("粘贴功能异常"); }
                  }}
                  className="px-3 py-1 rounded text-[11px] font-medium bg-[var(--color-bg-secondary)] text-[var(--color-text-secondary)] border border-[var(--color-border-subtle)] hover:bg-[var(--color-accent-subtle)] hover:text-[var(--color-accent)] transition-colors"
                >
                  测试
                </button>
              </div>
            </div>
          </section>

          <hr className="divider" />

          {/* ── About ── */}
          <section className="space-y-2.5 pb-4">
            <p className="text-[14px] font-semibold text-[var(--color-text-primary)]" style={{ fontFamily: "var(--font-display)" }}>
              轻语 Whisper
              <span className="text-[11px] font-normal text-[var(--color-text-tertiary)] ml-2" style={{ fontFamily: "var(--font-sans)" }}>
                v0.1.0
              </span>
            </p>
            <p className="text-[12px] leading-[1.6] text-[var(--color-text-secondary)]">
              本地语音转文字工具，基于 FunASR 离线语音识别引擎。
            </p>
            <div className="flex flex-wrap gap-2 pt-0.5">
              {["离线识别", "全局热键", "隐私优先"].map((tag) => (
                <span key={tag} className="chip">{tag}</span>
              ))}
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}
