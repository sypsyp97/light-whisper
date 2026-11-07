import { useEffect, useId, useRef, useState } from "react";
import { CircleAlert, CircleCheck, LoaderCircle, PlugZap } from "lucide-react";
import { useTranslation } from "react-i18next";
import { testModelConnection, type ModelConnectionRequest, type ModelConnectionResult } from "@/api/tauri";

interface ModelConnectionTestProps {
  request: ModelConnectionRequest;
  disabled?: boolean;
  contextKey?: string;
}

export default function ModelConnectionTest({ request, disabled = false, contextKey = "" }: ModelConnectionTestProps) {
  const { t } = useTranslation();
  const hintId = useId();
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
    <div className="model-connection-test">
      <p id={hintId} className="settings-hint model-connection-test-hint">
        {t(request.target === "asr" ? "settings.modelConnectionAsrHint"
          : request.target === "vision" ? "settings.modelConnectionVisionHint" : "settings.modelConnectionHint")}
      </p>
      <button type="button" className="btn-ghost btn-ghost-sm model-connection-test-button"
        disabled={disabled || running} aria-describedby={hintId}
        aria-busy={running} onClick={() => { void run(); }}>
        {running ? <LoaderCircle size={14} className="animate-spin" aria-hidden="true" />
          : <PlugZap size={14} aria-hidden="true" />}
        {t(running ? "settings.modelConnectionTesting" : "settings.testModelConnection")}
      </button>
      {current?.result && <p role="status" className="model-connection-test-result" data-success="true">
        <CircleCheck size={14} aria-hidden="true" />
        <span>{t("settings.modelConnectionSuccess", { model: current.result.model, ms: current.result.elapsedMs })}</span>
      </p>}
      {current?.error && <p className="model-connection-test-result settings-error" role="alert">
        <CircleAlert size={14} aria-hidden="true" />
        <span>{t("settings.modelConnectionFailed")} {current.error}</span>
      </p>}
    </div>
  );
}
