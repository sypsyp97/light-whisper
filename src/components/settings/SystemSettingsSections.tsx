import { useRef } from "react";
import { Accessibility, Copy, Download, Power, Upload } from "lucide-react";
import { useTranslation } from "react-i18next";

interface SystemSettingsSectionsProps {
  lastExportPath: string | null;
  autostart: boolean;
  autostartLoading: boolean;
  appVersion: string;
  updateStatusText: string;
  latestAvailableVersion: string | null;
  updateChecking: boolean;
  onExport: () => void;
  onCopyExportPath: () => void;
  onImport: (json: string) => Promise<void>;
  onTestPaste: () => void;
  onToggleAutostart: () => void;
  onUpdateAction: () => void;
}

export default function SystemSettingsSections({
  lastExportPath,
  autostart,
  autostartLoading,
  appVersion,
  updateStatusText,
  latestAvailableVersion,
  updateChecking,
  onExport,
  onCopyExportPath,
  onImport,
  onTestPaste,
  onToggleAutostart,
  onUpdateAction,
}: SystemSettingsSectionsProps) {
  const { t } = useTranslation();
  const importInputRef = useRef<HTMLInputElement | null>(null);

  return (
    <>
      <section className="settings-card">
        <div className="settings-section-header">
          <Download size={15} className="icon-accent" />
          <h2 className="settings-section-title">{t("settings.data")}</h2>
        </div>
        <div className="settings-data-actions">
          <button className="btn-ghost settings-data-action" onClick={onExport}>
            <Download size={13} />{t("settings.exportConfig")}
          </button>
          <button className="btn-ghost settings-data-action" onClick={() => importInputRef.current?.click()}>
            <Upload size={13} />{t("settings.importConfig")}
          </button>
          <input
            ref={importInputRef}
            className="settings-visually-hidden"
            type="file"
            accept=".json,application/json"
            aria-label={t("settings.importConfig")}
            onChange={(event) => {
              const file = event.target.files?.[0];
              event.target.value = "";
              if (!file) return;
              void file.text().then(onImport);
            }}
          />
        </div>
        {lastExportPath && (
          <div className="export-path-row">
            <div className="export-path-body">
              <span className="export-path-label">{t("settings.exportPath")}</span>
              <code className="export-path-value" title={lastExportPath}>{lastExportPath}</code>
            </div>
            <button
              type="button"
              className="export-path-copy"
              onClick={onCopyExportPath}
              aria-label={t("settings.copyExportPath")}
              title={t("settings.copyExportPath")}
            >
              <Copy size={13} />
            </button>
          </div>
        )}
      </section>

      <section className="settings-card">
        <div className="settings-section-header">
          <Accessibility size={15} className="icon-accent" />
          <h2 className="settings-section-title">{t("settings.permissions")}</h2>
        </div>
        <div className="permission-list">
          <div className="settings-row">
            <div className="permission-item">
              <Accessibility size={14} className="icon-tertiary" />
              <span className="permission-label">{t("settings.accessibilityPaste")}</span>
            </div>
            <button className="test-btn" onClick={onTestPaste}>{t("common.test")}</button>
          </div>
        </div>
      </section>

      <section className="settings-card" data-nav-id="startup">
        <div className="settings-section-header">
          <Power size={15} className="icon-accent" />
          <h2 className="settings-section-title">{t("settings.startup")}</h2>
        </div>
        <div className="settings-row">
          <span className="permission-label">{t("settings.autostart")}</span>
          <button
            role="switch"
            aria-checked={autostart}
            aria-busy={autostartLoading}
            aria-label={t("settings.autostart")}
            onClick={onToggleAutostart}
            className="toggle-switch"
            data-active={autostart}
            disabled={autostartLoading}
          >
            <div className="toggle-knob" />
          </button>
        </div>
      </section>

      <section className="settings-card">
        <div className="settings-section-header">
          <Download size={15} className="icon-accent" />
          <h2 className="settings-section-title">{t("settings.update")}</h2>
        </div>
        <div className="settings-row settings-update-row">
          <div className="permission-item settings-update-copy">
            <Download size={14} className="icon-tertiary" />
            <div className="settings-column settings-update-details">
              <span className="permission-label">{t("settings.checkAppUpdate")}</span>
              <p className="settings-hint">
                {updateStatusText || t("settings.currentVersion", { version: appVersion || "..." })}
              </p>
              {latestAvailableVersion && (
                <p className="settings-hint">
                  {t("settings.newVersionAvailable", { version: latestAvailableVersion })}
                </p>
              )}
            </div>
          </div>
          <button
            className="test-btn settings-update-button"
            onClick={onUpdateAction}
            disabled={updateChecking}
          >
            {updateChecking
              ? t("settings.checking")
              : latestAvailableVersion
                ? t("settings.goToDownload")
                : t("settings.checkUpdate")}
          </button>
        </div>
        {latestAvailableVersion && (
          <p className="settings-hint settings-update-source">{t("settings.updateSource")}</p>
        )}
      </section>
    </>
  );
}
