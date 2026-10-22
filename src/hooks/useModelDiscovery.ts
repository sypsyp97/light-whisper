import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { AiModelInfo, AiModelListPayload } from "@/types";
import { useDebouncedCallback } from "@/hooks/useDebouncedCallback";

type ModelFetcher = (silent: boolean) => Promise<AiModelListPayload>;

interface ModelInvalidationOptions {
  clearPolish: boolean;
  clearAssistant: boolean;
}

interface UseModelDiscoveryOptions {
  polishModelsContext: string;
  polishHasAuth: boolean;
  polishMissingAuthMessage: string;
  fetchPolishModels: ModelFetcher;
  assistantModelsContext: string;
  assistantHasAuth: boolean;
  assistantUseSeparateModel: boolean;
  assistantSharesPolishModels: boolean;
  fetchAssistantModels: ModelFetcher;
}

export function useModelDiscovery({
  polishModelsContext,
  polishHasAuth,
  polishMissingAuthMessage,
  fetchPolishModels,
  assistantModelsContext,
  assistantHasAuth,
  assistantUseSeparateModel,
  assistantSharesPolishModels,
  fetchAssistantModels,
}: UseModelDiscoveryOptions) {
  const { t } = useTranslation();
  const [aiModels, setAiModels] = useState<AiModelInfo[]>([]);
  const [assistantModels, setAssistantModels] = useState<AiModelInfo[]>([]);
  const [assistantModelsLoading, setAssistantModelsLoading] = useState(false);
  const [aiModelsLoading, setAiModelsLoading] = useState(false);
  const [aiModelsError, setAiModelsError] = useState("");
  const [assistantModelsError, setAssistantModelsError] = useState("");
  const [aiModelsSourceUrl, setAiModelsSourceUrl] = useState("");
  const aiModelsRequestIdRef = useRef(0);
  const assistantModelsRequestIdRef = useRef(0);
  const aiModelsContextRef = useRef<string | null>(null);
  const assistantModelsContextRef = useRef<string | null>(null);

  const refreshAiModels = useCallback(async (silent = false) => {
    const requestId = ++aiModelsRequestIdRef.current;
    const requestContext = polishModelsContext;
    if (!polishHasAuth) {
      setAiModels([]);
      setAiModelsSourceUrl("");
      setAiModelsError(polishMissingAuthMessage);
      setAiModelsLoading(false);
      aiModelsContextRef.current = null;
      return;
    }

    setAiModelsLoading(true);
    if (!silent) {
      setAiModelsError("");
    }

    try {
      const payload = await fetchPolishModels(silent);
      if (requestId !== aiModelsRequestIdRef.current) return;
      setAiModels(payload.models);
      setAiModelsSourceUrl(payload.sourceUrl);
      setAiModelsError(payload.models.length === 0 ? t("settings.modelListEmpty") : "");
      aiModelsContextRef.current = requestContext;
    } catch (err) {
      if (requestId !== aiModelsRequestIdRef.current) return;
      const message = err instanceof Error ? err.message : t("settings.fetchModelsFailed");
      const canKeepCurrentModels = aiModelsContextRef.current === requestContext;
      if (!canKeepCurrentModels) {
        setAiModels([]);
        setAiModelsSourceUrl("");
        aiModelsContextRef.current = null;
      }
      setAiModelsError(message);
    } finally {
      if (requestId === aiModelsRequestIdRef.current) {
        setAiModelsLoading(false);
      }
    }
  }, [fetchPolishModels, polishHasAuth, polishMissingAuthMessage, polishModelsContext, t]);

  const aiModelsFetch = useDebouncedCallback((silent: boolean) => {
    void refreshAiModels(silent);
  }, 700);

  const refreshAssistantModels = useCallback(async (silent = false) => {
    const requestId = ++assistantModelsRequestIdRef.current;
    const requestContext = assistantModelsContext;
    if (assistantSharesPolishModels) {
      setAssistantModels(aiModels);
      setAssistantModelsLoading(false);
      setAssistantModelsError("");
      assistantModelsContextRef.current = requestContext;
      return;
    }
    if (!assistantHasAuth) {
      setAssistantModels([]);
      setAssistantModelsLoading(false);
      setAssistantModelsError("");
      assistantModelsContextRef.current = null;
      return;
    }

    setAssistantModelsLoading(true);
    if (!silent) {
      setAssistantModelsError("");
    }
    try {
      const payload = await fetchAssistantModels(silent);
      if (requestId !== assistantModelsRequestIdRef.current) return;
      setAssistantModels(payload.models);
      setAssistantModelsError(payload.models.length === 0 ? t("settings.modelListEmpty") : "");
      assistantModelsContextRef.current = requestContext;
    } catch (err) {
      if (requestId !== assistantModelsRequestIdRef.current) return;
      const message = err instanceof Error ? err.message : t("settings.fetchModelsFailed");
      const canKeepCurrentModels = assistantModelsContextRef.current === requestContext;
      if (!canKeepCurrentModels) {
        setAssistantModels([]);
        assistantModelsContextRef.current = null;
      }
      setAssistantModelsError(message);
    } finally {
      if (requestId === assistantModelsRequestIdRef.current) {
        setAssistantModelsLoading(false);
      }
    }
  }, [aiModels, assistantHasAuth, assistantModelsContext, assistantSharesPolishModels, fetchAssistantModels, t]);

  const assistantModelsFetch = useDebouncedCallback((silent: boolean) => {
    void refreshAssistantModels(silent);
  }, 700);

  useEffect(() => {
    if (!polishHasAuth) {
      aiModelsFetch.cancel();
      aiModelsRequestIdRef.current += 1;
      aiModelsContextRef.current = null;
      setAiModels([]);
      setAiModelsSourceUrl("");
      setAiModelsError("");
      setAiModelsLoading(false);
      return;
    }

    if (aiModelsContextRef.current !== polishModelsContext) {
      aiModelsContextRef.current = null;
      setAiModels([]);
      setAiModelsSourceUrl("");
      setAiModelsError("");
    }

    aiModelsFetch.schedule(true);

    return () => {
      aiModelsFetch.cancel();
      aiModelsRequestIdRef.current += 1;
    };
  }, [aiModelsFetch, polishHasAuth, polishModelsContext]);

  const sharedAssistantModels = assistantSharesPolishModels ? aiModels : null;

  useEffect(() => {
    if (!assistantUseSeparateModel) {
      assistantModelsFetch.cancel();
      assistantModelsRequestIdRef.current += 1;
      assistantModelsContextRef.current = null;
      setAssistantModels([]);
      setAssistantModelsError("");
      setAssistantModelsLoading(false);
      return;
    }
    if (sharedAssistantModels !== null) {
      setAssistantModels(sharedAssistantModels);
      setAssistantModelsLoading(false);
      setAssistantModelsError("");
      assistantModelsContextRef.current = assistantModelsContext;
      return;
    }
    if (!assistantHasAuth) {
      assistantModelsFetch.cancel();
      assistantModelsRequestIdRef.current += 1;
      assistantModelsContextRef.current = null;
      setAssistantModels([]);
      setAssistantModelsLoading(false);
      setAssistantModelsError("");
      return;
    }
    if (assistantModelsContextRef.current !== assistantModelsContext) {
      assistantModelsContextRef.current = null;
      setAssistantModels([]);
      setAssistantModelsError("");
    }
    assistantModelsFetch.schedule(true);
    return () => {
      assistantModelsFetch.cancel();
      assistantModelsRequestIdRef.current += 1;
    };
  }, [assistantHasAuth, assistantModelsContext, assistantModelsFetch, assistantUseSeparateModel, sharedAssistantModels]);

  const refreshAiModelsNow = useCallback(() => {
    aiModelsFetch.cancel();
    return refreshAiModels();
  }, [aiModelsFetch, refreshAiModels]);

  const refreshAssistantModelsNow = useCallback(() => {
    assistantModelsFetch.cancel();
    return refreshAssistantModels();
  }, [assistantModelsFetch, refreshAssistantModels]);

  const invalidateModels = useCallback(({ clearPolish, clearAssistant }: ModelInvalidationOptions) => {
    if (clearPolish) {
      aiModelsFetch.cancel();
      aiModelsRequestIdRef.current += 1;
      aiModelsContextRef.current = null;
      setAiModels([]);
      setAiModelsSourceUrl("");
      setAiModelsError("");
      setAiModelsLoading(false);
    }
    if (clearAssistant) {
      assistantModelsFetch.cancel();
      assistantModelsRequestIdRef.current += 1;
      assistantModelsContextRef.current = null;
      setAssistantModels([]);
      setAssistantModelsError("");
      setAssistantModelsLoading(false);
    }
  }, [aiModelsFetch, assistantModelsFetch]);

  return {
    aiModels,
    aiModelsError,
    aiModelsLoading,
    aiModelsSourceUrl,
    assistantModels,
    assistantModelsError,
    assistantModelsLoading,
    invalidateModels,
    refreshAiModels,
    refreshAiModelsNow,
    refreshAssistantModels,
    refreshAssistantModelsNow,
  };
}
