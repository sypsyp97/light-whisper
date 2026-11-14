import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { getGpuBackend, listGpuBackends, setGpuBackend } from "@/api/tauri";

const OPTIONS = ["auto", "cuda", "vulkan", "cpu"] as const;
type Backend = (typeof OPTIONS)[number];

const LABEL: Record<Backend, string> = {
  auto: "settings.gpuBackendAuto",
  cuda: "settings.gpuBackendCuda",
  vulkan: "settings.gpuBackendVulkan",
  cpu: "settings.gpuBackendCpu",
};

const DESCRIPTION: Record<Backend, string> = {
  auto: "settings.gpuBackendAutoDesc",
  cuda: "settings.gpuBackendCudaDesc",
  vulkan: "settings.gpuBackendVulkanDesc",
  cpu: "settings.gpuBackendCpuDesc",
};

const isBackend = (value: string): value is Backend =>
  (OPTIONS as readonly string[]).includes(value);

export default function GpuBackendControl() {
  const { t } = useTranslation();
  const [backend, setBackend] = useState<Backend>("auto");
  const [installed, setInstalled] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const pending = useRef(false);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    let cancelled = false;
    Promise.all([getGpuBackend(), listGpuBackends()])
      .then(([value, list]) => {
        if (cancelled) return;
        setBackend(isBackend(value) ? value : "auto");
        setInstalled(Array.isArray(list) ? list : []);
      })
      .catch(() => {
        // Missing config reads as auto, matching an engine.json without the key.
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
      mounted.current = false;
    };
  }, []);

  const choose = async (next: Backend) => {
    if (pending.current || saving || next === backend) return;
    pending.current = true;
    setSaving(true);
    try {
      const saved = await setGpuBackend(next);
      if (mounted.current) setBackend(isBackend(saved) ? saved : "auto");
    } catch {
      toast.error(t("settings.gpuBackendSaveFailed"));
    } finally {
      pending.current = false;
      if (mounted.current) setSaving(false);
    }
  };

  const available = (option: Backend) =>
    option === "auto" || option === "cpu" || installed.includes(option);

  return (
    <div className="settings-inline-panel" style={{ marginTop: 8 }}>
      <div className="settings-column" style={{ gap: 6 }}>
        <div className="settings-column" style={{ gap: 2 }}>
          <span className="permission-label">{t("settings.gpuBackendTitle")}</span>
          <span className="settings-hint" style={{ margin: 0 }}>{t("settings.gpuBackendDesc")}</span>
        </div>
        <div className="settings-column" style={{ gap: 4 }} role="group" aria-label={t("settings.gpuBackendTitle")}>
          {OPTIONS.map((option) => {
            const enabled = available(option);
            return (
              <button
                key={option}
                type="button"
                className="picker-option"
                data-active={backend === option}
                aria-pressed={backend === option}
                disabled={loading || saving || !enabled}
                title={enabled ? undefined : t("settings.gpuBackendUnavailable")}
                onClick={() => { void choose(option); }}
              >
                <span className="picker-option-copy">
                  <strong>{t(LABEL[option])}</strong>
                  <span>{enabled ? t(DESCRIPTION[option]) : t("settings.gpuBackendUnavailable")}</span>
                </span>
              </button>
            );
          })}
        </div>
        <span className="settings-hint" style={{ margin: 0 }}>{t("settings.gpuBackendRestart")}</span>
      </div>
    </div>
  );
}
