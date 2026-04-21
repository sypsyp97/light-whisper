# Light Whisper — UI Rewrite Contract

**Branch:** `codex/apple-silicon-mlx-asr` (do NOT touch `main`)
**Date:** 2026-04-21
**Purpose:** Single source of truth for two isolated agents: **Tests agent** and **Implementation agent**. Both work from this file only; they never see each other's code.

---

## 0. Binding Rules (read first)

1. **Preserve every feature from main branch** except **local ASR models** (sensevoice, faster-whisper). The apple branch is **online-only ASR** (glm-asr, alibaba-asr).
2. **Do not rewrite** `src/api/tauri.ts`, `src/types/index.ts`, `src/contexts/RecordingContext.tsx`, `src/lib/constants.ts`, `src/i18n/en.ts`, `src/i18n/zh.ts`, any `src/hooks/*` file. These are the fixed infrastructure both agents compose against. Exception: `useModelStatus` on this branch already drops `downloadModels`; leave as is.
3. **Rewrite** `src/pages/MainPage.tsx`, `src/pages/SettingsPage.tsx`. Rewrite all `src/components/*.tsx` (except `SubtitleOverlay.tsx` under `src/pages/` — leave untouched).
4. **New location**: shared UI primitives live under `src/components/ui/`. Settings sections live under `src/components/settings/`. Main-page widgets live under `src/components/main/`.
5. **Styling**: macOS HIG native look. Use CSS only (no new libs). New stylesheet: `src/styles/app.css`. Keep existing `subtitle.css` and `theme.css`. Class names should start with `lw-` to avoid collisions.
6. **Icons**: keep `lucide-react`.
7. **i18n**: use existing keys in `src/i18n/en.ts` / `zh.ts`. If a key is missing for a new concept, use the closest existing key; do not add new i18n keys.
8. **Accessibility**: every interactive element must have an accessible name (`aria-label`, visible label, or `role` + text). All pickers / toggles / inputs must be keyboard-operable.
9. **Testing convention**: **every testable element carries a `data-testid`** following the format in §1.2. Tests query by `data-testid`, by `role` + `name`, or by visible text — never by CSS class.

---

## 1. Global Conventions

### 1.1 File layout (agents create these exact paths)

```
src/components/ui/
  Button.tsx              — <button> with variants
  IconButton.tsx          — square button for icons only
  Toggle.tsx              — iOS-style switch
  TextInput.tsx           — labeled text input
  SecretInput.tsx         — masked key input (exists; rewrite)
  TextArea.tsx            — multiline textarea
  Picker.tsx              — popover dropdown (replaces useExclusivePicker-based ad-hoc code)
  Segmented.tsx           — segmented control (e.g., theme: Light/Dark/System)
  Kbd.tsx                 — keyboard chord display (exists; rewrite)
  Card.tsx                — rounded card container
  Field.tsx               — label + control + hint wrapper
  Badge.tsx               — small status pill
  Banner.tsx              — dismissible error / info banner
  Modal.tsx               — centered modal with overlay
  ProgressBar.tsx         — thin progress bar

src/components/main/
  TitleBar.tsx            — window title bar (exists; rewrite)
  RecordingStage.tsx      — central circular record button + ring + status line
  RecordingButton.tsx     — the button itself (exists; rewrite)
  StatusIndicator.tsx     — engine/device chip row (exists; rewrite)
  TranscriptionResult.tsx — current-session result card (exists; rewrite)
  TranscriptionHistory.tsx — scrollable history list (exists; rewrite)
  OnboardingHint.tsx      — dismissible first-use hint card

src/components/settings/
  SettingsNav.tsx         — left side-nav list
  AppearanceSection.tsx
  EngineSection.tsx
  HotkeySection.tsx
  MicrophoneSection.tsx
  InputMethodSection.tsx
  AiPolishSection.tsx
  AssistantSection.tsx
  TranslationSection.tsx
  VocabularySection.tsx
  DataSection.tsx         — profile export/import + reset
  PermissionsSection.tsx  — macOS permission status & request buttons
  StartupSection.tsx      — autostart toggle
  UpdateSection.tsx       — version check

src/pages/
  MainPage.tsx            — rewrite; composes main/*
  SettingsPage.tsx        — rewrite; composes SettingsNav + all settings/* sections
```

### 1.2 `data-testid` naming

- **Primitives**: `ui-<name>` (e.g., `ui-toggle`, `ui-picker`, `ui-field`)
- **Sections**: `settings-section-<id>` where `<id>` is the `data-nav-id` listed in §3.
- **Specific controls**: `<section>-<control>` e.g., `engine-picker`, `hotkey-capture-btn`, `mic-device-picker`, `ai-polish-enable-toggle`.
- **Main page**: `main-record-btn`, `main-status`, `main-result`, `main-history`, `main-history-item-<id>`, `main-onboarding`, `main-error-banner`, `main-retry-btn`.
- **Modals**: `modal-<purpose>` e.g., `modal-hotkey-capture`, `modal-correction-rules`, `modal-add-provider`.

Every `data-testid` referenced in §2–§3 **must** appear verbatim in both test file queries and component JSX.

### 1.3 Tauri mocking convention (tests)

Tests never hit real Tauri. At the top of every test file that exercises Tauri:

```ts
import { vi } from "vitest";

vi.mock("@/api/tauri", () => ({
  // Every function used in the component returns a resolved promise.
  // Spy individual calls with vi.mocked(fn).
  getEngine: vi.fn(async () => "alibaba-asr"),
  setEngine: vi.fn(async () => "ok"),
  // ... etc. Each test file mocks what it needs.
}));

vi.mock("@tauri-apps/api/event", () => {
  const { createTauriEventController } = await import("@/test/tauriEventMock");
  return createTauriEventController().module;
});
```

For components that consume `useRecordingContext`, tests wrap with a test-only provider. Contract: expose a helper `renderWithRecordingContext(ui, overrides)` in `src/test/renderWithContext.tsx` — **tests agent creates this file**, **implementation agent does NOT read it** (the impl does not need to know about tests' helpers).

### 1.4 Behavior conventions

- Any debounced save uses **900 ms**.
- Toasts use `sonner` `toast.success` / `toast.error`. The i18n key shown is specified in §3.
- All async save calls catch errors and show the failure toast. Mark long-lived sync state with a loading indicator (`disabled` + spinner).
- Changing a controlled picker persists immediately (optimistic UI). On failure, revert and show error toast.

---

## 2. UI Primitives Contract

For each primitive: **file, props, required behaviors, testid**.

### 2.1 `Button.tsx`

```ts
export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "primary" | "secondary" | "ghost" | "danger";
  size?: "sm" | "md" | "lg";
  loading?: boolean;
  icon?: React.ReactNode;
  "data-testid"?: string;
}
```

- Default `variant="secondary"`, `size="md"`.
- `loading=true` → disables button + shows a rotating spinner, preserves width.
- Renders `<button type="button">` unless `type` overridden.
- **Testid**: `ui-button` (only when consumer omits `data-testid`).

### 2.2 `IconButton.tsx`

```ts
export interface IconButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  label: string;               // required — aria-label
  icon: React.ReactNode;
  variant?: "ghost" | "solid";
}
```

- Must set `aria-label={label}`.
- **Testid**: `ui-icon-button`.

### 2.3 `Toggle.tsx`

```ts
export interface ToggleProps {
  checked: boolean;
  onChange: (next: boolean) => void;
  label?: string;
  disabled?: boolean;
  "data-testid"?: string;
}
```

- Renders `<button role="switch" aria-checked={checked}>`.
- Click flips state via `onChange(!checked)`.
- Keyboard: Space/Enter toggles.
- **Testid**: passed through; query by role `switch` + `name` otherwise.

### 2.4 `TextInput.tsx`

```ts
export interface TextInputProps extends Omit<React.InputHTMLAttributes<HTMLInputElement>, "onChange"> {
  value: string;
  onChange: (next: string) => void;
  placeholder?: string;
  "data-testid"?: string;
}
```

- Standard `<input type="text">`.
- `onChange(e.target.value)`.

### 2.5 `SecretInput.tsx`

```ts
export interface SecretInputProps {
  value: string;
  onChange: (next: string) => void;
  placeholder?: string;
  "data-testid"?: string;
}
```

- Masked by default (`type="password"`).
- Eye icon toggles visibility; aria-label `common.show` / `common.hide`.
- **Testid**: passed; reveal button testid = `<testid>-reveal`.

### 2.6 `TextArea.tsx`

Multi-line counterpart of TextInput, same API, renders `<textarea>`.

### 2.7 `Picker.tsx`

```ts
export interface PickerOption<T extends string> {
  value: T;
  label: string;
  description?: string;
  icon?: React.ReactNode;
  disabled?: boolean;
}

export interface PickerProps<T extends string> {
  value: T;
  options: PickerOption<T>[];
  onChange: (next: T) => void;
  placeholder?: string;
  searchable?: boolean;          // if true, show search input at top of popover
  renderTrigger?: (opt: PickerOption<T> | undefined) => React.ReactNode;
  footer?: React.ReactNode;       // rendered at bottom of popover (e.g., "Add custom" button)
  "data-testid"?: string;
}
```

- Trigger is a button that shows selected option's label (or placeholder).
- Click opens popover with options list.
- Selecting fires `onChange` and closes.
- Escape closes.
- Click outside closes.
- **Testid**: passed; popover testid = `<testid>-popover`; options testid = `<testid>-option-<value>`.

### 2.8 `Segmented.tsx`

Like Picker but always-visible inline segmented control. Used for 2–4 options with clear icons (e.g., Theme, Input Method, Recording Mode).

```ts
export interface SegmentedProps<T extends string> {
  value: T;
  options: Array<{ value: T; label: string; icon?: React.ReactNode }>;
  onChange: (next: T) => void;
  "data-testid"?: string;
}
```

- **Testid**: passed; each segment testid = `<testid>-seg-<value>`.

### 2.9 `Kbd.tsx`

```ts
export interface KbdProps { combo: string; }
```

- Renders each key from `combo.split("+")` as its own `<kbd>` element, joined with "+".

### 2.10 `Card.tsx`

Container. `<section className="lw-card">`. Accepts children.

### 2.11 `Field.tsx`

```ts
export interface FieldProps {
  label: string;
  hint?: string;
  error?: string;
  children: React.ReactNode;
  "data-testid"?: string;
}
```

- Structure: `<label>` + child control + optional hint + optional error message.
- Associates `<label htmlFor>` with first focusable child if provided; otherwise wraps child in `<label>`.

### 2.12 `Badge.tsx`

```ts
export interface BadgeProps {
  tone?: "neutral" | "success" | "warn" | "danger" | "accent";
  children: React.ReactNode;
}
```

### 2.13 `Banner.tsx`

```ts
export interface BannerProps {
  tone: "error" | "info" | "warn";
  message: string;
  action?: { label: string; onClick: () => void; testId?: string };
  onDismiss?: () => void;
  "data-testid"?: string;
}
```

- Dismissible via X button (aria-label `common.close`), only if `onDismiss` provided.

### 2.14 `Modal.tsx`

```ts
export interface ModalProps {
  open: boolean;
  onClose: () => void;
  title: string;
  children: React.ReactNode;
  "data-testid"?: string;
}
```

- Renders via portal to document.body when `open`.
- Clicking overlay closes. Escape closes.
- Title renders as `<h2>`.

### 2.15 `ProgressBar.tsx`

```ts
export interface ProgressBarProps {
  value: number;          // 0–100
  indeterminate?: boolean;
}
```

---

## 3. Page & Section Contracts

### 3.1 Main Page (`src/pages/MainPage.tsx`)

Composes (in order):
1. `<TitleBar>` — settings icon → navigate, minimize, close.
2. `<StatusIndicator>` — shows engine ready / loading / error + device/GPU chip.
3. `<RecordingStage>` — central record button + status line.
4. `<TranscriptionResult>` — shown when `transcriptionResult` non-null.
5. `<TranscriptionHistory>` — shown when `history.length > 0`.
6. `<OnboardingHint>` — shown if `!localStorage[ONBOARDING_DISMISSED_KEY]` AND `history.length === 0`.
7. `<Banner tone="error">` — shown when `recordingError || modelError`. Retry button only for model errors.

**data-testid:** root `main-page`.

**Interactions to preserve:**
- Click settings icon → `navigate("/settings")`.
- Click minimize → `getCurrentWindow().minimize()`.
- Click close → `getCurrentWindow().hide()` (window hides, not quits).
- Record button triggers `startRecording()` / `stopRecording()` based on state.
- Copy button in result copies `transcriptionResult` to clipboard, shows toast `toast.copied`, highlights row 1500ms.
- Editing transcription in dictation mode submits debounced correction (900ms) via `submitUserCorrection(original, edited, rawOriginal)`.
- Copy item in history copies that item's text.
- Dismiss onboarding writes `ONBOARDING_DISMISSED_KEY = "true"`.
- Dismiss error banner hides it until the next error comes in.

---

### 3.2 `TitleBar.tsx`

```ts
export interface TitleBarProps {
  title?: string;                       // default i18n `app.title`
  leftAction?: { icon: React.ReactNode; label: string; onClick: () => void };
  onMinimize?: () => void;
  onClose?: () => void;
  rightExtras?: React.ReactNode;        // extra icons between title and minimize
  "data-testid"?: string;
}
```

- Default: on main page `leftAction` = settings icon (opens settings), minimize + close visible.
- data-app-region="drag" on the central title area (so window is draggable by the title area).
- **Testids**: `titlebar`, `titlebar-left-action`, `titlebar-minimize`, `titlebar-close`.

---

### 3.3 `RecordingStage.tsx`

```ts
export interface RecordingStageProps {
  isRecording: boolean;
  isProcessing: boolean;
  isReady: boolean;
  hotkeyDisplay: string;
  recordingMode: "hold" | "toggle";
  error: string | null;
  onToggle: () => void;                 // start if !isRecording else stop
}
```

- Layout: giant circular button (140–160px) centered.
- Ring pulses when `isRecording`.
- Below: status line showing hotkey and mode hint (`main.hotkeyHintToggle` / `main.hotkeyHintHold`).
- Disabled when `!isReady` or `isProcessing`.
- **Testid root**: `main-record-stage`. Button: `main-record-btn`.

---

### 3.4 `RecordingButton.tsx`

Inner button used by RecordingStage (can also stand alone).

```ts
export interface RecordingButtonProps {
  state: "idle" | "recording" | "processing" | "disabled";
  onClick: () => void;
  "data-testid"?: string;
}
```

- idle → microphone icon, accent color.
- recording → square icon, red/red-accent pulsing ring.
- processing → spinner, disabled.
- disabled → greyed out, disabled.
- aria-label cycles with state using `recording.start` / `recording.stop` / `recording.processing`.

---

### 3.5 `StatusIndicator.tsx`

```ts
export interface StatusIndicatorProps {
  stage: "checking" | "loading" | "ready" | "error";
  isReady: boolean;
  engineLabel: string;          // e.g., "GLM-ASR" or "Alibaba DashScope"
  device: string | null;
  gpuName: string | null;
  downloadProgress?: number;
  downloadMessage?: string | null;
  error: string | null;
  onRetry?: () => void;
  onCancelDownload?: () => void;
}
```

- **Testid**: `main-status`.
- When `stage==="ready"`: show green dot + engine label + optional device chip.
- When `stage==="loading"`: show spinner + `status.modelLoading`.
- When `stage==="checking"`: show `status.preparing`.
- When `stage==="error"`: show error text + retry button (testid `main-retry-btn`).

---

### 3.6 `TranscriptionResult.tsx`

```ts
export interface TranscriptionResultProps {
  text: string;
  originalText: string | null;     // untouched ASR text, for diff-based correction submission
  mode: "dictation" | "assistant";
  durationSec: number | null;
  charCount: number | null;
  detectedLanguage: string | null;
  onChange: (next: string) => void;                   // debounced 900ms via useDebouncedCallback
  onSubmitCorrection: (original: string, corrected: string, raw: string | null) => void;
  onCopy: () => void;
}
```

- **Testid root**: `main-result`.
- **Testids**: `main-result-text` (editable), `main-result-copy`, `main-result-stats`.
- Stats line uses `result.stats` with interpolation `{{chars}}`, `{{duration}}`, `{{cpm}}`.
- `cpm = round(charCount / (durationSec/60))`.
- Editable via `contenteditable="true"` or `<textarea>` (impl choice). Whichever chosen, tests will use `fireEvent.input` or `userEvent.type` and expect `onChange` to fire.
- Edits in `mode="dictation"` only call `onSubmitCorrection` (debounced). Edits in `mode="assistant"` only call `onChange`.

---

### 3.7 `TranscriptionHistory.tsx`

```ts
export interface TranscriptionHistoryProps {
  items: HistoryItem[];
  onCopy: (item: HistoryItem) => void;
}
```

- Renders a list, newest first.
- Each item shows `timeDisplay`, `text` (clamped to 3 lines, expandable on click/keyboard).
- **Testids**: root `main-history`; each row `main-history-item-<id>`; copy button `main-history-copy-<id>`.

---

### 3.8 `OnboardingHint.tsx`

```ts
export interface OnboardingHintProps {
  hotkeyDisplay: string;
  mode: "hold" | "toggle";
  onDismiss: () => void;
}
```

- Shows `main.pressHotkey` + `<Kbd combo={hotkeyDisplay}/>` + mode-specific hint.
- Dismiss button (testid `main-onboarding-dismiss`).
- Root testid: `main-onboarding`.

---

### 3.9 Settings Page (`src/pages/SettingsPage.tsx`)

Layout: left side-nav (30%) + scrollable content (70%).

```
appearance  | <AppearanceSection/>
engine      | <EngineSection/>
hotkey      | <HotkeySection/>
microphone  | <MicrophoneSection/>
input       | <InputMethodSection/>
ai-polish   | <AiPolishSection/>
assistant   | <AssistantSection/>
translation | <TranslationSection/>
vocabulary  | <VocabularySection/>
permissions | <PermissionsSection/>
startup     | <StartupSection/>
data        | <DataSection/>
update      | <UpdateSection/>
```

- Root testid: `settings-page`.
- Each section wrapper: `<section data-testid="settings-section-<id>" data-nav-id="<id>">`.
- SettingsNav items: `<button data-testid="settings-nav-<id>">`.
- Clicking a nav item scrolls smooth to that section.
- IntersectionObserver updates active nav when scrolling (preserve main-branch behavior).
- Top of page has a back button → `navigate("/")`, testid `settings-back-btn`.

---

### 3.10 `SettingsNav.tsx`

```ts
export interface SettingsNavItem { id: string; label: string; icon: React.ReactNode; }
export interface SettingsNavProps {
  items: SettingsNavItem[];
  activeId: string;
  onNavigate: (id: string) => void;
}
```

Testid: `settings-nav`; each item: `settings-nav-<id>`.

---

### 3.11 `AppearanceSection.tsx`

- **Theme**: `Segmented` with `light / dark / system`, `data-testid="appearance-theme"`. Uses `useTheme()`.
- **Language**: `Picker` with options built from i18n available locales (`en`, `zh`), `data-testid="appearance-language"`. Uses `localStorage[LANGUAGE_STORAGE_KEY]`, dispatches `window.dispatchEvent(new StorageEvent("storage", ...))` on change so SubtitleOverlay syncs.

---

### 3.12 `EngineSection.tsx`

**Engines** (apple-branch, online only):

```ts
const ENGINES = [
  { key: "glm-asr",      label: "GLM-ASR",           descKey: "settings.glmAsrDesc", icon: Globe },
  { key: "alibaba-asr",  label: "Alibaba DashScope", descKey: "settings.alibabaAsrDesc", icon: Cloud, labelKey: "settings.alibabaAsrLabel" },
] as const;
```

Controls:
- `Picker` for engine, testid `engine-picker`. Calls `getEngine()` on mount; `setEngine(key)` on change.
- `SecretInput` for API key, testid `engine-api-key`. Debounced 900ms → `setOnlineAsrApiKey(key, keyringUser)` where `keyringUser = "${engine}:${region}"`.
- If engine is `alibaba-asr`:
  - `Picker` for region (`international` / `domestic`), testid `engine-region-picker`. `setOnlineAsrEndpoint(region)`.
  - `Picker` for model, testid `engine-model-picker`. `getAlibabaAsrConfig()` / `setAlibabaAsrModel(model)`. Refresh button testid `engine-model-refresh` → `listAlibabaAsrModels()`.

**Test matrix**:
- Renders both engine options.
- Switching engine fires `setEngine`.
- Typing API key (after 900ms) fires `setOnlineAsrApiKey`.
- For alibaba, model picker populated from `listAlibabaAsrModels`.

No models-dir controls, no local-engine toggles (apple branch).

---

### 3.13 `HotkeySection.tsx`

- **Main hotkey**: capture button (testid `hotkey-capture-btn`), reset button (`hotkey-reset-btn`). Uses `useHotkeyCapture`, context `setHotkey`. Default `F2`.
- Diagnostic banner (`Banner tone="warn"`) when `hotkeyDiagnostic.systemConflict` or `hotkeyDiagnostic.warning` non-null. Testid `hotkey-diagnostic`.
- Error banner when `hotkeyError` non-null. Testid `hotkey-error-banner`.
- **Recording mode**: `Segmented` with `hold / toggle`, testid `recording-mode-segmented`. Persists to `localStorage[RECORDING_MODE_KEY]` + `setRecordingMode(mode === "toggle")`.

---

### 3.14 `MicrophoneSection.tsx`

- Device `Picker`, testid `mic-device-picker`. Options from `listInputDevices()`. First option: "follow system default" (empty value). On change → `setInputDevice(name || null)`, persist `INPUT_DEVICE_STORAGE_KEY`.
- Refresh `IconButton`, testid `mic-refresh`.
- Test `Button`, testid `mic-test`. Calls `testMicrophone()`, shows toast.
- Level monitor `Toggle`, testid `mic-level-monitor-toggle`. Persists to `MIC_LEVEL_MONITOR_ENABLED_KEY`. When on, starts `startMicrophoneLevelMonitor()` and listens for `microphone-level` event. Renders animated bar testid `mic-level-bar` with `role="meter"` `aria-valuenow`.

---

### 3.15 `InputMethodSection.tsx`

- `Segmented` two options: `sendInput` (direct input) and `clipboard` (clipboard paste), testid `input-method-segmented`. Persists to `INPUT_METHOD_KEY`, calls `setInputMethodCommand(method)`.
- `Toggle` sound enabled, testid `input-sound-toggle`. Persists to `SOUND_ENABLED_KEY`, calls `setSoundEnabled(enabled)`.

---

### 3.16 `AiPolishSection.tsx`

- Master `Toggle`, testid `polish-enable-toggle`. Persists to `AI_POLISH_ENABLED_KEY`, calls `setAiPolishConfig(enabled, apiKey)` with current key.
- Screen context `Toggle`, testid `polish-screen-context-toggle`. Calls `setAiPolishScreenContextEnabled`.
- Provider `Picker`, testid `polish-provider-picker`. Options: presets (`openai`, `deepseek`, `cerebras`, `siliconflow`, `custom_compat`) plus custom providers from `userProfile.llm_provider.custom_providers`. Footer: "Add custom provider" button opens `<Modal data-testid="modal-add-provider">` with fields: name / base URL / model / format (`openai_compat` or `anthropic`). Submits via `addCustomProvider`.
- Base URL `TextInput`, testid `polish-base-url`. Visible only when provider is `custom_compat` or a user custom one allows override. Debounced → `setLlmProviderConfig(active, baseUrl, ...)`.
- API key `SecretInput`, testid `polish-api-key`. Debounced → `setAiPolishConfig(enabled, key)`. Hidden when `openai` + `oauth` mode signed in.
- Model `Picker`, testid `polish-model-picker`, `searchable`. Populated lazily via `listAiModels(provider, baseUrl, apiKey)` once key/provider present. Manual-entry supported via search-input fallback.
- Reasoning mode `Picker`, testid `polish-reasoning-picker`. Options: `provider_default / off / light / balanced / deep`. Visible only when reasoning probe says supported. Uses `getLlmReasoningSupport`.
- **OpenAI OAuth block** (visible only when provider is `openai`):
  - Auth mode `Segmented` with `api_key / oauth`, testid `polish-openai-auth-mode`.
  - Login/Logout `Button` testid `polish-openai-oauth-btn`.
  - Fast mode `Toggle` testid `polish-fast-mode-toggle`. Visible only when `status.loggedIn && authMode==="oauth"`. Calls `setOpenaiFastMode(bool)`.
  - Status badge shows email, plan type.
- Custom prompt `TextArea`, testid `polish-custom-prompt`. Debounced → `setCustomPrompt(prompt || null)`.

---

### 3.17 `AssistantSection.tsx`

- Enable `Toggle`, testid `assistant-enable-toggle` (derived from whether `assistant_hotkey` set).
- Hotkey capture `Button`, testid `assistant-hotkey-btn`. Clear button testid `assistant-hotkey-clear`. Calls `setAssistantHotkey(shortcut | null)`.
- "Use same provider as Polish" `Toggle`, testid `assistant-same-provider-toggle`.
- When OFF: provider `Picker` (testid `assistant-provider-picker`), model `Picker` (testid `assistant-model-picker`), API key `SecretInput` (testid `assistant-api-key`), reasoning `Picker` (testid `assistant-reasoning-picker`). Persist via `setLlmProviderConfig(... assistantUseSeparateModel=true, assistantProvider, assistantModel, assistantReasoningMode ...)` and `setAssistantApiKey`.
- Screen context `Toggle`, testid `assistant-screen-context-toggle` → `setAssistantScreenContextEnabled`.
- System prompt `TextArea`, testid `assistant-system-prompt`, debounced → `setAssistantSystemPrompt(prompt || null)`.
- **Web search block** (inside Assistant):
  - Enable `Toggle`, testid `websearch-enable-toggle`.
  - Provider `Picker` (`model_native / exa / tavily`), testid `websearch-provider-picker`.
  - Max results slider / `Picker` 1–10, testid `websearch-max-results`, visible when provider != `model_native`.
  - Tavily API key `SecretInput`, testid `websearch-tavily-key`, visible when provider=`tavily`.
  - Persist via `setWebSearchConfig` / `setWebSearchApiKey`.

---

### 3.18 `TranslationSection.tsx`

- Hotkey capture, testid `translation-hotkey-btn`, clear testid `translation-hotkey-clear`. Calls `setTranslationHotkey(shortcut | null)`.
- Target language `Picker` (testid `translation-target-picker`). Options: presets `English / 日本語 / 한국어 / Français / Deutsch / Español / Русский / Português` + "Off" + "Custom…". Custom opens inline `TextInput` testid `translation-custom-input`. Calls `setTranslationTarget(lang | null)`. If server returns `true` (auto-enabled polish), show toast `toast.translationAutoPolish`.

---

### 3.19 `VocabularySection.tsx`

- Add hot word: `TextInput` + `Button`, testids `hot-word-input` and `hot-word-add-btn`. Calls `addHotWord(text, 5)` (default weight 5).
- List of current hot words with remove buttons (testid `hot-word-remove-<text>`), tone colored by `source` (user = accent, learned = warn).
- Count display uses `settings.hotWordsCount`.
- Correction rules button, testid `correction-rules-btn`. Opens `<Modal data-testid="modal-correction-rules">`:
  - Filter `Segmented` (`all / user / ai`), testid `correction-filter`.
  - Search `TextInput`, testid `correction-search`.
  - Rule list: each with delete button testid `correction-delete-<original>__<corrected>`.
  - Validation block inside modal:
    - Enable `Toggle`, testid `correction-validation-toggle` → `setCorrectionValidationConfig({enabled:true})`.
    - Separate model `Toggle`, testid `correction-validation-separate-toggle`.
    - Provider+model `Pickers` when separate on, testids `correction-validation-provider`, `correction-validation-model`.
    - Validate `Button`, testid `correction-validate-btn` → `validateCorrections()` then toast with count.

---

### 3.20 `PermissionsSection.tsx` (NEW on apple branch; replaces scattered main-branch permission UI)

Purpose: request macOS permissions explicitly. Four rows — one per permission:

| Row | Label (i18n key or literal) | Check API | Request/Open API |
|---|---|---|---|
| Microphone | `settings.permMicrophone` or literal "Microphone" | `checkPermissionMicrophone` | `requestPermissionMicrophone` |
| Accessibility | `settings.permAccessibility` or literal | `checkPermissionAccessibility` | `requestPermissionAccessibility` |
| Screen recording | `settings.permScreen` or literal | `checkPermissionScreen` | `requestPermissionScreen` |
| Automation | `settings.permAutomation` or literal | `checkPermissionAutomation` | `requestPermissionAutomation` |

Each row shows `Badge` (tone=success if granted, warn otherwise) + "Open Settings" / "Request" `Button`.

**API note to implementation agent**: The Tauri backend on this apple branch already has `check_permissions` / `request_permission` commands (see `src-tauri/src/commands/permissions.rs` / `services/permissions_service.rs`). Add the following exports in `src/api/tauri.ts` if not present:

```ts
export type PermissionKind = "microphone" | "accessibility" | "screen" | "automation";
export interface PermissionStatus { granted: boolean; canRequest: boolean; }
export function checkPermission(kind: PermissionKind): Promise<PermissionStatus> {
  return invokeCommand("check_permission", { kind });
}
export function requestPermission(kind: PermissionKind): Promise<PermissionStatus> {
  return invokeCommand("request_permission", { kind });
}
```

(The implementation agent is allowed to add these two exports to `tauri.ts`.)

Test row testids: `perm-row-<kind>`, button `perm-request-<kind>`, badge `perm-status-<kind>`.

Paste-test button retained: testid `perm-paste-test-btn` → `pasteText("ok", "clipboard")`.

---

### 3.21 `StartupSection.tsx`

- Autostart `Toggle`, testid `autostart-toggle`. Uses `isAutostartEnabled() / enableAutostart() / disableAutostart()`.

---

### 3.22 `DataSection.tsx`

- Export `Button`, testid `data-export-btn`. Calls `exportUserProfile()`, saves via `Blob` download as `light-whisper-profile.json`.
- Import `Button`, testid `data-import-btn`. Triggers a hidden `<input type="file" accept="application/json">` testid `data-import-input`. Reads file, calls `importUserProfile(json)`.
- Toasts: `toast.configExported / configImported / configExportFailed / configImportFailed`.

---

### 3.23 `UpdateSection.tsx`

- Current version `Badge` from `@tauri-apps/api/app` `getVersion()`. Testid `update-current-version`.
- Check `Button`, testid `update-check-btn`. Calls `checkAppUpdate()`. If `result.available`, show banner with link-action that calls `openAppReleasePage(result.releaseUrl)`.

---

## 4. Test File Layout

Tests agent creates:

```
src/components/ui/__tests__/
  Button.test.tsx
  Toggle.test.tsx
  Picker.test.tsx
  Segmented.test.tsx
  SecretInput.test.tsx
  Modal.test.tsx
  Field.test.tsx
  Banner.test.tsx

src/components/main/__tests__/
  TitleBar.test.tsx
  RecordingStage.test.tsx
  RecordingButton.test.tsx
  StatusIndicator.test.tsx
  TranscriptionResult.test.tsx
  TranscriptionHistory.test.tsx
  OnboardingHint.test.tsx

src/components/settings/__tests__/
  AppearanceSection.test.tsx
  EngineSection.test.tsx
  HotkeySection.test.tsx
  MicrophoneSection.test.tsx
  InputMethodSection.test.tsx
  AiPolishSection.test.tsx
  AssistantSection.test.tsx
  TranslationSection.test.tsx
  VocabularySection.test.tsx
  PermissionsSection.test.tsx
  StartupSection.test.tsx
  DataSection.test.tsx
  UpdateSection.test.tsx

src/pages/__tests__/
  MainPage.test.tsx
  SettingsPage.test.tsx   (just smoke: renders all section testids)

src/test/renderWithContext.tsx   (test helper — impl agent must NOT rely on)
```

Each test file must:
- Mock `@/api/tauri` and `@tauri-apps/api/event` at top.
- For components using `RecordingContext`, use `renderWithRecordingContext`.
- Assert data-testids and visible text exist.
- Simulate user interactions with `@testing-library/user-event`.
- Assert Tauri commands called with expected args.

Minimum test cases per component: **render**, **primary interaction**, **error state** (where applicable).

---

## 5. Styling Notes (implementation agent)

- Base palette: CSS variables already defined in `theme.css`. Do not redefine. Introduce new vars only in `app.css` scoped under `:root[data-theme]`.
- Use `backdrop-filter: blur(...)` for modal overlays and large surfaces (native vibrancy feel).
- Corner radius tokens: `--lw-radius-sm: 6px; --lw-radius-md: 10px; --lw-radius-lg: 14px`. Reuse existing if already in theme.css.
- Fonts: `-apple-system, BlinkMacSystemFont, "SF Pro Text", "SF Pro Display", system-ui, sans-serif`.
- Transitions: 160ms cubic-bezier(0.2, 0, 0, 1) for default, 240ms for layout changes.
- Focus rings: 2px outline `var(--lw-accent)` with 2px offset.

---

## 6. What Implementation Agent Must NOT Do

- Do not modify `src/api/tauri.ts` except adding the two permission functions in §3.20.
- Do not modify `src/types/index.ts`.
- Do not modify `src/contexts/RecordingContext.tsx`.
- Do not modify `src/hooks/*`.
- Do not modify `src/i18n/*` files.
- Do not modify `src/pages/SubtitleOverlay.tsx`.
- Do not add new npm dependencies.
- Do not read or write any files under `src/**/__tests__/`. Tests are someone else's concern.

## 7. What Test Agent Must NOT Do

- Do not modify any file under `src/components/ui/`, `src/components/main/`, `src/components/settings/`, or `src/pages/MainPage.tsx` / `SettingsPage.tsx`.
- Do not add new npm dependencies. Use what's in `package.json` (vitest, @testing-library/react, @testing-library/user-event, @testing-library/jest-dom).
- Do not test styling / pixel-perfect visuals; test **behavior**, **accessibility**, and **testid presence**.
- Do not rely on import paths outside this contract. If a section isn't in §3, do not write tests for it.

---

## 8. Acceptance: How We Know It Works

After integration, the orchestrator runs:
- `pnpm install` (no new deps expected)
- `pnpm test` — all tests green
- `pnpm build` or equivalent — type check passes
- `pnpm tauri build` — packages app for macOS
- Manual smoke: launch app cold, verify (1) permission dialogs appear first-run, (2) main page renders, (3) settings sections all render and selections persist.

---

**End of Contract.**
