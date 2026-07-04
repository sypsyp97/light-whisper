import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowLeft,
  Palette,
  Cloud,
  Keyboard,
  Mic,
  ClipboardPaste,
  Sparkles,
  Bot,
  Languages,
  BookOpen,
  Database,
  Shield,
  Power,
  RefreshCw,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import SettingsNav, { type SettingsNavItem } from "@/components/settings/SettingsNav";
import AppearanceSection from "@/components/settings/AppearanceSection";
import EngineSection from "@/components/settings/EngineSection";
import HotkeySection from "@/components/settings/HotkeySection";
import MicrophoneSection from "@/components/settings/MicrophoneSection";
import InputMethodSection from "@/components/settings/InputMethodSection";
import AiPolishSection from "@/components/settings/AiPolishSection";
import AssistantSection from "@/components/settings/AssistantSection";
import TranslationSection from "@/components/settings/TranslationSection";
import VocabularySection from "@/components/settings/VocabularySection";
import PermissionsSection from "@/components/settings/PermissionsSection";
import StartupSection from "@/components/settings/StartupSection";
import DataSection from "@/components/settings/DataSection";
import UpdateSection from "@/components/settings/UpdateSection";

interface SettingsPageProps {
  onNavigate?: (target: "main" | "settings") => void;
  active?: boolean;
  animClass?: string;
}

export default function SettingsPage({ onNavigate, animClass }: SettingsPageProps) {
  const { t } = useTranslation();
  const contentRef = useRef<HTMLDivElement | null>(null);
  const clickScrollRef = useRef(false);
  const [activeId, setActiveId] = useState("appearance");

  const navItems: SettingsNavItem[] = useMemo(() => [
    { id: "appearance", label: t("settings.appearance"), icon: <Palette size={14} /> },
    { id: "engine", label: t("settings.engine"), icon: <Cloud size={14} /> },
    { id: "hotkey", label: t("settings.hotkeySection"), icon: <Keyboard size={14} /> },
    { id: "microphone", label: t("settings.microphone"), icon: <Mic size={14} /> },
    { id: "input", label: t("settings.inputMethod"), icon: <ClipboardPaste size={14} /> },
    { id: "ai-polish", label: t("settings.aiPolish"), icon: <Sparkles size={14} /> },
    { id: "assistant", label: t("settings.assistant"), icon: <Bot size={14} /> },
    { id: "translation", label: t("settings.translation"), icon: <Languages size={14} /> },
    { id: "vocabulary", label: t("settings.vocabulary"), icon: <BookOpen size={14} /> },
    { id: "permissions", label: t("settings.permissions"), icon: <Shield size={14} /> },
    { id: "startup", label: t("settings.startup"), icon: <Power size={14} /> },
    { id: "data", label: t("settings.data"), icon: <Database size={14} /> },
    { id: "update", label: t("settings.update"), icon: <RefreshCw size={14} /> },
  ], [t]);

  const handleNavigate = useCallback((id: string) => {
    const container = contentRef.current;
    if (!container) return;
    const target = container.querySelector(`[data-nav-id="${id}"]`) as HTMLElement | null;
    if (!target) return;
    setActiveId(id);
    clickScrollRef.current = true;
    target.scrollIntoView({ behavior: "smooth", block: "start" });
    window.setTimeout(() => { clickScrollRef.current = false; }, 600);
  }, []);

  useEffect(() => {
    const container = contentRef.current;
    if (!container) return;
    const els = navItems
      .map(({ id }) => container.querySelector(`[data-nav-id="${id}"]`))
      .filter((e): e is Element => Boolean(e));
    if (els.length === 0) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (clickScrollRef.current) return;
        let topId = "";
        let topY = Infinity;
        for (const entry of entries) {
          if (entry.isIntersecting && entry.boundingClientRect.top < topY) {
            topY = entry.boundingClientRect.top;
            topId = (entry.target as HTMLElement).dataset.navId ?? "";
          }
        }
        if (topId) setActiveId(topId);
      },
      { root: container, rootMargin: "-10% 0px -70% 0px", threshold: 0 },
    );
    for (const el of els) observer.observe(el);
    return () => observer.disconnect();
  }, [navItems]);

  return (
    <div
      className={`lw-root lw-settings-page ${animClass ?? ""}`.trim()}
      data-testid="settings-page"
    >
      <header className="lw-titlebar">
        <div className="lw-titlebar-left">
          <button
            type="button"
            className="lw-settings-back"
            onClick={() => onNavigate?.("main")}
            data-testid="settings-back-btn"
            aria-label={t("common.back")}
          >
            <ArrowLeft size={14} />
            <span>{t("common.back")}</span>
          </button>
        </div>
        <div className="lw-titlebar-drag" data-tauri-drag-region>
          <span className="lw-titlebar-title">{t("settings.title")}</span>
        </div>
        <div className="lw-titlebar-right" />
      </header>
      <div className="lw-settings-body">
        <div ref={contentRef} className="lw-settings-content">
          <SettingsNav items={navItems} activeId={activeId} onNavigate={handleNavigate} />
          <AppearanceSection />
          <EngineSection />
          <HotkeySection />
          <MicrophoneSection />
          <InputMethodSection />
          <AiPolishSection />
          <AssistantSection />
          <TranslationSection />
          <VocabularySection />
          <PermissionsSection />
          <StartupSection />
          <DataSection />
          <UpdateSection />
        </div>
      </div>
    </div>
  );
}
