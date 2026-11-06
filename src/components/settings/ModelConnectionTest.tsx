import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { testModelConnection, type ModelConnectionRequest, type ModelConnectionResult } from "@/api/tauri";

interface ModelConnectionTestProps {
  request: ModelConnectionRequest;
  disabled?: boolean;
  contextKey?: string;
}

export default function ModelConnectionTest({ request, disabled = false, contextKey = "" }: ModelConnectionTestProps) {
  const { t } = useTranslation();
  const signature = JSON.stringify([request, contextKey]);
  const context = useRef({ signature, version: 0 });
  if (context.current.signature !== signature) {
    context.current = { signature, version: context.current.version + 1 };
  }
  const mounted = useRef(true);
  const pending = useRef(false);
  const [running, setRunning] = useState(false);
  const [outcome, setOutcome] = useState<{
    version: number;
    result?: ModelConnectionResult;
    error?: string;
  } | null>(null);

  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);

  const run = async () => {
    if (pending.current || disabled) return;
    const version = context.current.version;
    pending.current = true;
    setRunning(true);
    setOutcome(null);
    try {
      const result = await testModelConnection(request);
      if (mounted.current && context.current.version === version) setOutcome({ version, result });
    } catch (error) {
      if (mounted.current && context.current.version === version) {
        setOutcome({ version, error: error instanceof Error ? error.message : String(error) });
      }
    } finally {
      pending.current = false;
      if (mounted.current) setRunning(false);
    }
  };

  const current = outcome?.version === context.current.version ? outcome : null;
  return (
    <div className="settings-column model-connection-test">
      <div className="settings-row model-connection-test-row">
        <button type="button" className="btn-ghost" disabled={disabled || running}
          aria-busy={running} onClick={() => { void run(); }}>
          {t(running ? "settings.modelConnectionTesting" : "settings.testModelConnection")}
        </button>
        {current?.result && <span role="status" className="settings-hint settings-hint-flush">
          {t("settings.modelConnectionSuccess", { model: current.result.model, ms: current.result.elapsedMs })}
        </span>}
      </div>
      {current?.error && <p className="settings-error" role="alert">
        {t("settings.modelConnectionFailed")} {current.error}
      </p>}
      <p className="settings-hint settings-hint-flush">
        {t(request.target === "asr" ? "settings.modelConnectionAsrHint"
          : request.target === "vision" ? "settings.modelConnectionVisionHint" : "settings.modelConnectionHint")}
      </p>
    </div>
  );
}
