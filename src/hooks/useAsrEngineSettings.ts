import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import {
  getAlibabaAsrConfig,
  getEngine,
  getOnlineAsrApiKey,
  getOnlineAsrEndpoint,
  listAlibabaAsrModels,
  setAlibabaAsrModel,
  setEngine,
  setOnlineAsrApiKey,
  setOnlineAsrEndpoint,
} from "@/api/tauri";
import { useDebouncedCallback } from "@/hooks/useDebouncedCallback";
import { getAsrEngineCapability } from "@/lib/asrEngineCapabilities";

interface UseAsrEngineSettingsOptions {
  engineLabel: (engine: string) => string;
  retryModel: () => void;
}

export function useAsrEngineSettings({
  engineLabel,
  retryModel,
}: UseAsrEngineSettingsOptions) {
  const { t } = useTranslation();
  const configVersion = useRef(0);
  const keyReadVersion = useRef(0);
  const modelSelectionVersion = useRef(0);
  const modelsRequestId = useRef(0);
  const mounted = useRef(false);
  const transitioning = useRef(false);
  const configRetryVersion = useRef<number | null>(null);
  const retryModelRef = useRef(retryModel);
  retryModelRef.current = retryModel;
  const [configRetryOwner, setConfigRetryOwner] = useState<number | null>(null);
  const [engine, setEngineState] = useState("qwen3-asr-0.6b");
  const [engineLoading, setEngineLoading] = useState(true);
  const [engineReady, setEngineReady] = useState(false);
  const [endpointReady, setEndpointReady] = useState(false);
  const [configReadErrors, setConfigReadErrors] = useState({ engine: false, endpoint: false, key: false });
  const [onlineAsrApiKey, setOnlineAsrApiKeyState] = useState("");
  const [onlineAsrRegion, setOnlineAsrRegion] = useState("international");
  const [onlineAsrUrl, setOnlineAsrUrl] = useState("");
  const [onlineAsrRegionLoading, setOnlineAsrRegionLoading] = useState(false);
  const [alibabaAsrModel, setAlibabaAsrModelState] = useState("qwen-audio-3.1-asr-flash");
  const [alibabaAsrModels, setAlibabaAsrModelsState] = useState<readonly string[]>([]);
  const [alibabaAsrModelsSource, setAlibabaAsrModelsSource] = useState<"live" | "fallback">("fallback");
  const [alibabaAsrModelsLoading, setAlibabaAsrModelsLoading] = useState(false);

  const onlineAsrEndpointReady = engineReady && endpointReady;
  const onlineAsrKeyReady = engineReady
    && (engine === "glm-asr" || (engine === "alibaba-asr" && endpointReady));
  const asrConfigError = configReadErrors.engine || configReadErrors.endpoint || configReadErrors.key;
  const asrConfigRetrying = configRetryOwner === configVersion.current;

  const readAsrConfig = useCallback(async (
    parts: { engine?: boolean; endpoint?: boolean; key?: boolean },
    waitForAll = false,
  ) => {
    const version = configVersion.current;
    const keyVersion = keyReadVersion.current;
    const current = () => mounted.current && version === configVersion.current;
    setConfigReadErrors((previous) => ({
      engine: parts.engine ? false : previous.engine,
      endpoint: parts.endpoint ? false : previous.endpoint,
      key: parts.key ? false : previous.key,
    }));
    if (parts.engine) setEngineLoading(true);

    const reads = [
      parts.engine && getEngine().then((value) => {
        if (!current()) return;
        setEngineState(value);
        setEngineReady(true);
      }).catch((error) => {
        if (!current()) return;
        setConfigReadErrors((previous) => ({ ...previous, engine: true }));
        throw error;
      }).finally(() => { if (current()) setEngineLoading(false); }),
      parts.endpoint && getOnlineAsrEndpoint().then((endpoint) => {
        if (!current()) return;
        setOnlineAsrRegion(endpoint.region);
        setOnlineAsrUrl(endpoint.url);
        setEndpointReady(true);
      }).catch((error) => {
        if (!current()) return;
        setConfigReadErrors((previous) => ({ ...previous, endpoint: true }));
        throw error;
      }),
      parts.key && getOnlineAsrApiKey().then((key) => {
        if (current() && keyVersion === keyReadVersion.current) setOnlineAsrApiKeyState(key || "");
      }).catch((error) => {
        if (!current() || keyVersion !== keyReadVersion.current) return;
        setConfigReadErrors((previous) => ({ ...previous, key: true }));
        throw error;
      }),
    ];
    return waitForAll
      ? Promise.allSettled(reads).then((results) => current() && results.some((result) => result.status === "rejected"))
      : Promise.all(reads).then(() => false, () => current());
  }, []);

  const retryAsrConfig = useCallback(async () => {
    const version = configVersion.current;
    if (transitioning.current || configRetryVersion.current === version) return;
    configRetryVersion.current = version;
    setConfigRetryOwner(version);
    try {

      await readAsrConfig(configReadErrors);
    } finally {
      if (configRetryVersion.current === version) {
        configRetryVersion.current = null;
        setConfigRetryOwner(null);
      }
    }
  }, [configReadErrors, readAsrConfig]);

  const computeOnlineAsrKeyringUser = useCallback((engineValue: string, region: string): string => {
    if (engineValue === "alibaba-asr") {
      return region === "domestic" ? "alibaba-asr-cn-api-key" : "alibaba-asr-intl-api-key";
    }
    return "glm-asr-api-key";
  }, []);

  const onlineAsrKeySave = useDebouncedCallback(
    async (value: string, keyringUser: string) => {
      try {
        await setOnlineAsrApiKey(value, keyringUser);
      } catch (error) {
        toast.error(t("toast.onlineAsrKeySaveFailed"));
        throw error;
      }
    },
    600,
    { onUnmount: "flush" },
  );

  useEffect(() => {
    mounted.current = true;
    const version = ++configVersion.current;
    const modelVersion = modelSelectionVersion.current;
    const catalogVersion = modelsRequestId.current;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const current = () => !disposed && version === configVersion.current;
    const hydrateInitialConfig = () => {
      if (current()) void readAsrConfig({ engine: true, endpoint: true, key: true });
    };
    getAlibabaAsrConfig().then((config) => {
      if (!current()) return;
      if (modelVersion === modelSelectionVersion.current) setAlibabaAsrModelState(config.model);
      if (catalogVersion === modelsRequestId.current) setAlibabaAsrModelsState(config.models);
    }).catch(() => {});
    void listen("engine-config-changed", () => {
      if (disposed || !mounted.current) return;
      const eventVersion = ++configVersion.current;
      keyReadVersion.current += 1;
      modelsRequestId.current += 1;
      transitioning.current = false;
      configRetryVersion.current = null;
      setConfigRetryOwner(null);
      setEngineReady(false);
      setEndpointReady(false);
      setOnlineAsrApiKeyState("");
      setOnlineAsrUrl("");
      setOnlineAsrRegionLoading(false);
      setAlibabaAsrModelsLoading(false);
      void readAsrConfig({ engine: true, endpoint: true, key: true }, true).then(() => {
        if (!disposed && mounted.current && eventVersion === configVersion.current) {
          retryModelRef.current();
        }
      });
    }).then((stop) => {
      if (disposed) stop();
      else {
        unlisten = stop;
        hydrateInitialConfig();
      }
    }).catch((error) => {
      if (!disposed) {
        console.error("Failed to watch speech engine settings", error);
        hydrateInitialConfig();
      }
    });
    return () => {
      disposed = true;
      mounted.current = false;
      configVersion.current += 1;
      modelsRequestId.current += 1;
      unlisten?.();
    };
  }, [readAsrConfig]);

  const refreshAlibabaModels = useCallback(async () => {
    const requestId = ++modelsRequestId.current;
    const version = configVersion.current;
    const current = () => mounted.current && requestId === modelsRequestId.current && version === configVersion.current;
    setAlibabaAsrModelsLoading(true);
    try {
      const result = await listAlibabaAsrModels();
      if (!current()) return;
      if (result.warning) toast.warning(result.warning);
      if (result.models.length > 0) {
        setAlibabaAsrModelsState(result.models);
        setAlibabaAsrModelsSource(result.source);
      }
    } catch {
      if (current()) toast.error(t("settings.fetchModelsFailed"));
    } finally {
      if (current()) setAlibabaAsrModelsLoading(false);
    }
  }, [t]);

  const alibabaHasKey = engine === "alibaba-asr" && onlineAsrApiKey.trim().length > 0;
  useEffect(() => {
    if (engine !== "alibaba-asr" || !alibabaHasKey) return;
    void refreshAlibabaModels();
  }, [alibabaHasKey, engine, onlineAsrRegion, refreshAlibabaModels]);

  const handleEngineSwitch = useCallback(async (newEngine: string) => {
    if (transitioning.current || engineLoading || newEngine === engine) return;
    let version = configVersion.current;
    const current = () => mounted.current && version === configVersion.current;
    transitioning.current = true;
    setEngineLoading(true);

    try {
      try {
        await onlineAsrKeySave.flush();
      } catch {
        return;
      }
      if (!current()) return;
      await setEngine(newEngine);
      if (!current()) return;
      version = ++configVersion.current;
      modelsRequestId.current += 1;
      setAlibabaAsrModelsLoading(false);
      setOnlineAsrApiKeyState("");
      setOnlineAsrUrl("");
      setEngineState(newEngine);
      setEngineReady(true);
      setEndpointReady(false);
      setConfigReadErrors({ engine: false, endpoint: false, key: false });
      toast.success(t("toast.switchedToEngine", { label: engineLabel(newEngine) }));

      if (getAsrEngineCapability(newEngine)?.execution === "cloud") {
        if (await readAsrConfig({ key: true, endpoint: true }) && current()) {
          toast.error(t("toast.onlineAsrConfigReadFailed"));
        }
      }
      if (current()) retryModel();
    } catch {
      if (current()) toast.error(t("toast.switchEngineFailed"));
    } finally {
      if (current()) {
        transitioning.current = false;
        setEngineLoading(false);
      }
    }
  }, [engine, engineLabel, engineLoading, onlineAsrKeySave, readAsrConfig, retryModel, t]);

  const handleOnlineAsrRegionChange = useCallback(async (region: string) => {
    if (transitioning.current || engineLoading || onlineAsrRegionLoading || !onlineAsrEndpointReady || onlineAsrRegion === region) return;
    let version = configVersion.current;
    const current = () => mounted.current && version === configVersion.current;
    transitioning.current = true;
    setOnlineAsrRegionLoading(true);
    try {
      try {
        await onlineAsrKeySave.flush();
      } catch {
        return;
      }
      if (!current()) return;
      const endpoint = await setOnlineAsrEndpoint(region);
      if (!current()) return;
      version = ++configVersion.current;
      modelsRequestId.current += 1;
      setAlibabaAsrModelsLoading(false);
      if (engine === "alibaba-asr") setOnlineAsrApiKeyState("");
      setOnlineAsrRegion(endpoint.region);
      setOnlineAsrUrl(endpoint.url);
      setEndpointReady(true);
      if (engine === "alibaba-asr") {
        if (await readAsrConfig({ key: true }) && current()) {
          toast.error(t("toast.onlineAsrConfigReadFailed"));
        }
      }
    } catch {
      if (current()) toast.error(t("toast.onlineAsrRegionSwitchFailed"));
    } finally {
      if (current()) {
        transitioning.current = false;
        setOnlineAsrRegionLoading(false);
      }
    }
  }, [engine, engineLoading, onlineAsrEndpointReady, onlineAsrKeySave, onlineAsrRegion, onlineAsrRegionLoading, readAsrConfig, t]);

  const handleOnlineAsrApiKeyChange = useCallback((value: string) => {
    if (transitioning.current || !onlineAsrKeyReady) return;
    keyReadVersion.current += 1;
    setConfigReadErrors((previous) => ({ ...previous, key: false }));
    setOnlineAsrApiKeyState(value);
    onlineAsrKeySave.schedule(
      value,
      computeOnlineAsrKeyringUser(engine, onlineAsrRegion),
    );
  }, [computeOnlineAsrKeyringUser, engine, onlineAsrKeyReady, onlineAsrKeySave, onlineAsrRegion]);

  const handleAlibabaAsrModelSelect = useCallback(async (model: string) => {
    try {
      await setAlibabaAsrModel(model);
      modelSelectionVersion.current += 1;
      setAlibabaAsrModelState(model);
    } catch {

    }
  }, []);

  return {
    alibabaAsrModel,
    alibabaAsrModels,
    alibabaAsrModelsLoading,
    alibabaAsrModelsSource,
    alibabaHasKey,
    asrConfigError,
    asrConfigRetrying,
    engine,
    engineLoading,
    handleAlibabaAsrModelSelect,
    handleEngineSwitch,
    handleOnlineAsrApiKeyChange,
    handleOnlineAsrRegionChange,
    onlineAsrApiKey,
    onlineAsrEndpointReady,
    onlineAsrKeyReady,
    onlineAsrRegion,
    onlineAsrRegionLoading,
    onlineAsrUrl,
    refreshAlibabaModels,
    retryAsrConfig,
  };
}
