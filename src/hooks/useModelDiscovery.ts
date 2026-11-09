import { useCallback, useEffect } from "react";
import type { AiModelListPayload } from "@/types";
import { useDebouncedCallback } from "@/hooks/useDebouncedCallback";
import { useModelCatalog } from "@/hooks/useModelCatalog";

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
  const {
    models: aiModels, error: aiModelsError, loading: aiModelsLoading,
    sourceUrl: aiModelsSourceUrl, refresh: refreshPolishCatalog, reset: resetPolishCatalog,
  } = useModelCatalog({ context: polishModelsContext, fetchModels: fetchPolishModels });
  const {
    models: assistantModels, error: assistantModelsError, loading: assistantModelsLoading,
    refresh: refreshAssistantCatalog, reset: resetAssistantCatalog, share: shareAssistantCatalog,
  } = useModelCatalog({ context: assistantModelsContext, fetchModels: fetchAssistantModels });

  const refreshAiModels = useCallback(async (silent = false) => {
    if (!polishHasAuth) {
      resetPolishCatalog(polishMissingAuthMessage);
      return;
    }
    await refreshPolishCatalog(silent);
  }, [polishHasAuth, polishMissingAuthMessage, refreshPolishCatalog, resetPolishCatalog]);
  const aiModelsFetch = useDebouncedCallback((silent: boolean) => {
    void refreshAiModels(silent);
  }, 700);

  const refreshAssistantModels = useCallback(async (silent = false) => {
    if (assistantSharesPolishModels) {
      shareAssistantCatalog(aiModels);
      return;
    }
    if (!assistantHasAuth) {
      resetAssistantCatalog();
      return;
    }
    await refreshAssistantCatalog(silent);
  }, [aiModels, assistantHasAuth, assistantSharesPolishModels, refreshAssistantCatalog, resetAssistantCatalog, shareAssistantCatalog]);
  const assistantModelsFetch = useDebouncedCallback((silent: boolean) => {
    void refreshAssistantModels(silent);
  }, 700);

  useEffect(() => {
    if (!polishHasAuth) {
      aiModelsFetch.cancel();
      resetPolishCatalog();
      return;
    }
    aiModelsFetch.schedule(true);
    return aiModelsFetch.cancel;
  }, [aiModelsFetch, polishHasAuth, polishModelsContext, resetPolishCatalog]);

  const sharedAssistantModels = assistantSharesPolishModels ? aiModels : null;
  useEffect(() => {
    if (!assistantUseSeparateModel) {
      assistantModelsFetch.cancel();
      resetAssistantCatalog();
      return;
    }
    if (sharedAssistantModels !== null) {
      shareAssistantCatalog(sharedAssistantModels);
      return;
    }
    if (!assistantHasAuth) {
      assistantModelsFetch.cancel();
      resetAssistantCatalog();
      return;
    }
    assistantModelsFetch.schedule(true);
    return assistantModelsFetch.cancel;
  }, [assistantHasAuth, assistantModelsContext, assistantModelsFetch, assistantUseSeparateModel, resetAssistantCatalog, sharedAssistantModels, shareAssistantCatalog]);

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
      resetPolishCatalog();
    }
    if (clearAssistant) {
      assistantModelsFetch.cancel();
      resetAssistantCatalog();
    }
  }, [aiModelsFetch, assistantModelsFetch, resetAssistantCatalog, resetPolishCatalog]);

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
