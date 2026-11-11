import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { AiModelInfo, AiModelListPayload } from "@/types";

export function useModelCatalog({ context, fetchModels }: {
  context: string;
  fetchModels: (silent: boolean) => Promise<AiModelListPayload>;
}) {
  const { t } = useTranslation();
  const [models, setModels] = useState<AiModelInfo[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [sourceUrl, setSourceUrl] = useState("");
  const requestIdRef = useRef(0);
  const loadedContextRef = useRef<string | null>(null);

  const reset = useCallback((message = "", resetLoading = true) => {
    requestIdRef.current += 1;
    loadedContextRef.current = null;
    setModels([]);
    setSourceUrl("");
    setError(message);
    if (resetLoading) setLoading(false);
  }, []);

  const share = useCallback((sharedModels: AiModelInfo[]) => {
    requestIdRef.current += 1;
    loadedContextRef.current = context;
    setModels(sharedModels);
    setSourceUrl("");
    setError("");
    setLoading(false);
  }, [context]);

  const refresh = useCallback(async (silent = false) => {
    const requestId = ++requestIdRef.current;
    setLoading(true);
    if (!silent) setError("");
    try {
      const payload = await fetchModels(silent);
      if (requestId !== requestIdRef.current) return;
      setModels(payload.models);
      setSourceUrl(payload.sourceUrl);
      setError(payload.warning || (payload.models.length === 0 ? t("settings.modelListEmpty") : ""));
      loadedContextRef.current = context;
    } catch (err) {
      if (requestId !== requestIdRef.current) return;
      if (loadedContextRef.current !== context) {
        setModels([]);
        setSourceUrl("");
        loadedContextRef.current = null;
      }
      setError(err instanceof Error ? err.message : t("settings.fetchModelsFailed"));
    } finally {
      if (requestId === requestIdRef.current) setLoading(false);
    }
  }, [context, fetchModels, t]);

  useEffect(() => {
    setLoading(false);
    if (loadedContextRef.current !== context) {
      loadedContextRef.current = null;
      setModels([]);
      setSourceUrl("");
      setError("");
    }
    return () => { requestIdRef.current += 1; };
  }, [context]);

  return { models, loading, error, sourceUrl, refresh, reset, share };
}
