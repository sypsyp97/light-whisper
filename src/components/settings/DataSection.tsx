import { useRef } from "react";
import { Download, Upload } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { exportUserProfile, importUserProfile } from "@/api/tauri";
import Field from "@/components/ui/Field";
import Button from "@/components/ui/Button";

export function DataSection() {
  const { t } = useTranslation();
  const fileInputRef = useRef<HTMLInputElement | null>(null);

  const handleExport = async () => {
    try {
      const json = await exportUserProfile();
      const blob = new Blob([json], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = "light-whisper-profile.json";
      a.click();
      URL.revokeObjectURL(url);
      toast.success(t("toast.configExported"));
    } catch {
      toast.error(t("toast.configExportFailed"));
    }
  };

  const handleImport = async (file: File) => {
    try {
      const text = await file.text();
      await importUserProfile(text);
      toast.success(t("toast.configImported"));
    } catch {
      toast.error(t("toast.configImportFailed"));
    }
  };

  return (
    <section
      className="lw-settings-section"
      data-testid="settings-section-data"
      data-nav-id="data"
    >
      <h2 className="lw-settings-section-title">{t("settings.data")}</h2>
      <Field label={t("settings.data")}>
        <div className="lw-inline">
          <Button
            icon={<Download size={14} />}
            onClick={() => void handleExport()}
            data-testid="data-export-btn"
          >
            {t("settings.exportConfig")}
          </Button>
          <Button
            icon={<Upload size={14} />}
            onClick={() => fileInputRef.current?.click()}
            data-testid="data-import-btn"
          >
            {t("settings.importConfig")}
          </Button>
          <input
            ref={fileInputRef}
            type="file"
            accept="application/json"
            style={{ display: "none" }}
            data-testid="data-import-input"
            onChange={(e) => {
              const file = e.target.files?.[0];
              if (file) void handleImport(file);
              e.target.value = "";
            }}
          />
        </div>
      </Field>
    </section>
  );
}

export default DataSection;
