import { defineConfig } from "vitest/config";
import applicationConfig from "../../vitest.config.ts";

// Resolve the application config inside the disposable verification copy so
// its @ alias follows Stryker's sandbox source, including the mutated hook.
export default defineConfig({
  ...applicationConfig,
  test: {
    ...applicationConfig.test,
    maxWorkers: 1,
    pool: "forks",
    include: [
      "src/hooks/__tests__/useAsrEngineSettings.test.tsx",
      "src/hooks/__tests__/useAsrEngineSettingsHydration.test.tsx",
      "src/hooks/__tests__/useAsrEngineSettingsFormalReplay.test.tsx",
      "src/hooks/__tests__/useAsrEngineSettingsExternalChanges.test.tsx",
      "src/hooks/__tests__/useAsrEngineSettingsMeasurementGaps.test.tsx",
      "src/hooks/__tests__/useAsrEngineSettingsSubscriptionBootstrap.test.tsx",
      "src/hooks/__tests__/useAsrEngineSettingsTransitionOwnership.test.tsx",
      "src/hooks/__tests__/useDebouncedCallback.test.tsx",
    ],
    coverage: {
      provider: "v8",
      include: ["src/hooks/useAsrEngineSettings.ts"],
      reporter: ["json", "json-summary", "text", "html"],
      reportsDirectory: "reports/asr-settings/coverage",
      reportOnFailure: true,
    },
  },
});
