# Light Whisper - Feature Inventory (MAIN Branch)

**Generated for UI Rewrite Planning**  
**Date:** 2026-04-21

---

## Executive Summary

This document comprehensively catalogs all features on the `main` branch of Light Whisper. The app is a voice-to-text utility with advanced AI polishing, multiple ASR engines (local + online), and contextual assistance features. Main components are split across:

- **MainPage** (recording UI + transcription display + history)
- **SettingsPage** (~4196 lines, 9 major sections)
- **SubtitleOverlay** (floating status window)
- **RecordingContext** (state management bridge)
- **Hooks** (useRecording, useModelStatus, useHotkey, etc.)

---

## 1. Main Page (src/pages/MainPage.tsx)

### Recording & Control

- **RecordingButton**: Toggle or hold-to-talk button, responds to hotkey and manual click
- **States**: `isRecording`, `isProcessing`, `isReady`, `recordingError`, `modelError`
- **Recording modes**: 
  - **Hold mode** (default): Press hotkey, speak, release to transcribe
  - **Toggle mode**: Press hotkey to start, press again to stop
- **Mode persisted in localStorage** via `RECORDING_MODE_KEY`

### UI Elements

- **TitleBar** (top): Settings gear icon (left), Minimize/Close buttons (right)
- **StatusIndicator**: Device name, GPU name, download progress, model status, retry button
- **RecordingButton**: Center of recording zone
- **TranscriptionResult**: Editable result text, character count, duration (s), CPM, detected language
- **TranscriptionHistory**: List of past recordings, copy-to-clipboard for each item
- **Error banner**: Dismissible error display with optional Retry button for model errors
- **Onboarding hint**: First-use card displays hotkey combo and instructions, dismissed after first successful transcription (state: `ONBOARDING_DISMISSED_KEY` in localStorage)

### Interactions

- **Copy to clipboard**: Result & history items, toast feedback, brief visual highlight (1500ms)
- **Edit transcription**: Text is editable (both current result and history); edits in "dictation" mode trigger debounced correction submission to backend
- **Correction learning**: Original ASR text captured separately (`originalAsrText`), diffs vs edited text are debounced (900ms) and sent to `submitUserCorrection`
- **Navigation**: Settings button navigates to SettingsPage; pending edits flushed before nav

### State Management

- Subscribes to **RecordingContext** for:
  - Recording state, transcription, history, model status, hotkey display
  - Download progress/messages, error messages
- **Local state**:
  - `copiedId`: Track which item shows "copied" highlight
  - `errorDismissed`: Hide error banner temporarily
  - `onboardingDismissed`: Hide onboarding hint
  - `isToggleMode`: Cached recording mode at mount

---

## 2. Settings Page (src/pages/SettingsPage.tsx) — 4196 Lines

### Architecture

- **Mutually exclusive picker dropdowns** via `useExclusivePicker` hook
- **Side navigation**: IntersectionObserver tracks scroll to highlight active section
- **State persisted to**: localStorage + Tauri backend via debounced API calls
- **LLM provider drafts**: Local JSON cache for unsaved provider configs (`LLM_PROVIDER_DRAFTS_KEY`)

### Navigation Sections (in order)

```
appearance → engine → hotkey → microphone → input → ai-polish → assistant → translation → vocabulary → misc
```

---

### 2.1 Appearance & Language

**Data-nav-id:** `appearance`

- **Theme picker**: Light | Dark | System (uses `useTheme` hook)
  - Icons: Sun, Moon, Monitor
  - Persisted to localStorage (`THEME_STORAGE_KEY`)
  - Affects SubtitleOverlay in real-time
  
- **Language picker**: Dropdown
  - Options pulled dynamically from i18n keys
  - Persisted to localStorage (`LANGUAGE_STORAGE_KEY`)
  - Real-time sync across windows via storage event listener

---

### 2.2 ASR Engine (data-nav-id: `engine`)

**Supported engines** (4 total):

| Engine | Type | Key | Label | Description |
|--------|------|-----|-------|-------------|
| SenseVoice | LOCAL | sensevoice | SenseVoice | CN/EN/JP/KR/Cantonese, high accuracy |
| Faster Whisper | LOCAL | whisper | Faster Whisper | 99+ languages, fast inference |
| GLM-ASR | ONLINE | glm-asr | GLM-ASR | Zhipu online ASR, Chinese-optimized |
| Alibaba DashScope | ONLINE | alibaba-asr | Alibaba DashScope | Qwen ASR & Omni models, regional |

**LOCAL_ENGINES** = `["sensevoice", "whisper"]`  
**ONLINE_ENGINES** = `["glm-asr", "alibaba-asr"]`

**Controls per engine:**

- **SenseVoice & Whisper**: No API config needed; model files self-managed
- **GLM-ASR**: API Key input, endpoint region selector (not exposed in UI, handled by backend)
- **Alibaba DashScope**: 
  - API Key input
  - Region picker (international | domestic CN)
  - Model picker (dropdown, refreshable)
  - Live model list or fallback to hardcoded defaults

**Engine-specific settings:**

- **Models directory** (for local engines):
  - Current path display
  - Browse button (`pickFolder()`)
  - Reset to default button
  - Migrate checkbox (auto-migrate files if path changes)
  - Migration status messages
  - Custom vs default flag display

**Calls made:**
- `getEngine()` → fetch current
- `setEngine(engine)` → switch
- `getAlibabaAsrConfig()`, `setAlibabaAsrModel(model)`, `listAlibabaAsrModels()` → Alibaba only
- `getOnlineAsrApiKey()`, `setOnlineAsrApiKey(key)`, `getOnlineAsrEndpoint()`, `setOnlineAsrEndpoint(region)` → Online ASR
- `getModelsDir()`, `pickFolder()`, `setModelsDir(path, migrate)` → Models dir

---

### 2.3 Hotkey (data-nav-id: `hotkey`)

**Main recording hotkey:**
- **Capture button** (`useHotkeyCapture` hook): Shows current combo or "Press key combination..."
- **Reset button**: Resets to default F2
- **Display format**: Kbd component (e.g., "Cmd+Shift+R")
- **Diagnostic panel**: 
  - System conflict warning (if another app owns the hotkey)
  - General warning (e.g., accessibility permission pending)
  - Error message (if registration failed)

**Recording mode picker** (nested in hotkey section):
- **Hold to talk** (default): Release key to finish recording
- **Toggle mode**: Press again to stop

**Calls made:**
- `setHotkey(shortcut)` → register custom hotkey (via RecordingContext)
- `unregisterAllHotkeys()` → cleanup on exit
- `registerCustomHotkey(shortcut)` → direct registration
- `getHotkeyDiagnostic()` → fetch diagnostic info
- `setRecordingMode(toggle: boolean)` → persist mode

---

### 2.4 Microphone (data-nav-id: `microphone`)

**Controls:**
- **Device picker**: Dropdown showing available audio input devices
  - "Follow system default" option (empty selection)
  - List of named devices (mark system default in description)
  - Fallback warning if saved device unavailable
- **Refresh button**: Reload device list
- **Test button**: Test current mic (plays sound, updates level monitor if enabled)
- **Level monitor toggle**: Enable/disable real-time waveform visualization
- **Microphone level display**: Animated bar (0–100%)

**Monitoring states:**
- Monitor off
- Recording (preview paused)
- Listening (show live levels, respond to speech)
- Not started (device busy)

**Calls made:**
- `listInputDevices()` → get device list
- `setInputDevice(name)` → select device
- `testMicrophone()` → test & toast feedback
- `startMicrophoneLevelMonitor()`, `stopMicrophoneLevelMonitor()` → real-time levels
- Listens for `microphone-level` event (backend sends `{ deviceName?, level? }`)

**Persisted settings:**
- Selected device name → localStorage (`INPUT_DEVICE_STORAGE_KEY`)
- Monitor enabled flag → localStorage (`MIC_LEVEL_MONITOR_ENABLED_KEY`)

---

### 2.5 Input Method (data-nav-id: `input`)

**Two strategies:**
1. **Direct Input** (default): Uses keyboard simulation, ignores clipboard
2. **Clipboard Paste**: Copies text to clipboard, relies on app focus for paste compatibility

**Controls:**
- Radio-like button group showing both options
- Description under each

**Additional toggle:**
- **Recording Sound**: Enable/disable sound played during input simulation (in some apps)

**Calls made:**
- `setInputMethodCommand(method)` → inform backend
- `setSoundEnabled(enabled)` → toggle recording sound

**Persisted:**
- Input method → localStorage (`INPUT_METHOD_KEY`)
- Sound enabled → localStorage (`SOUND_ENABLED_KEY`)

---

### 2.6 AI Polish (data-nav-id: `ai-polish`)

**Main toggle:**
- **Enable AI Polish**: True/false switch, debounced API call
- **Screen context toggle**: Capture screen for vision-based polishing (if model supports images)

**Provider selection (dropdown):**
- **Presets**: OpenAI, DeepSeek, Cerebras, SiliconFlow, Custom Compatible
- **Custom providers**: User-added providers (edit/delete from dropdown)
- **Add custom provider form** (inline in dropdown):
  - Provider name input
  - Base URL input (e.g., `https://api.example.com`)
  - Default model input
  - API format select (OpenAI Compatible | Anthropic)
  - Cancel / Add buttons

**Base URL handling:**
- Preset providers: Show official URL, read-only
- Custom/Custom Compat: Allow override in separate input field (or use custom provider's URL)

**Model selection (dropdown):**
- **Search input**: Filter by model name
- **API fetch**: Auto-fetch models when API key entered
- **Manual entry**: Type model name directly if not in list
- **Reasoning mode selector**: When provider supports (provider_default | off | light | balanced | deep)

**API Key:**
- **SecretInput** component (masked)
- Debounced save (900ms)
- Only shown if provider requires manual auth

**OpenAI OAuth:**
- **Sign in with ChatGPT button**: Opens OAuth flow
- **Connection status badge**: Shows email, plan type (Plus/Pro), account ID
- **Fast mode toggle** (if OAuth signed in):
  - Uses ChatGPT priority (~1.5x faster, ~2x credit consumption)
  - Only visible & effective when logged in
- **Auth mode selector** (appears if OpenAI chosen):
  - API Key mode: Use manual key
  - OAuth mode: Use ChatGPT sign-in (requires sign-in first)

**Custom prompt field:**
- **Input**: Long-form text area
- **Placeholder**: "e.g. I'm a developer, keep all English technical terms; replace 'Light Words' with 'Light Whisper'"
- **Hint**: "Custom correction rules, higher priority than built-in rules. Leave empty to disable."
- **Debounced save** (900ms)

**Reasoning mode** (if supported by model):
- Dropdown: provider_default | off | light | balanced | deep
- Detection message: "Detecting reasoning support for current model..."
- Fallback: If model doesn't support, show warning and use default

**Calls made:**
- `setAiPolishConfig(enabled, apiKey)` → toggle & set key
- `getAiPolishApiKey()` → fetch current key on mount
- `setAiPolishScreenContextEnabled(enabled)` → toggle screen context
- `listAiModels(provider, baseUrl, apiKey)` → fetch model list
- `setLlmProviderConfig(...)` → save all polish settings
- `getLlmReasoningSupport(provider, model, baseUrl, apiKey)` → check reasoning capability
- `addCustomProvider(name, baseUrl, model, format)` → add custom provider
- `updateCustomProvider(id, name, baseUrl, model, format)` → edit provider
- `removeCustomProvider(id)` → delete provider
- `setCustomPrompt(prompt)` → save polish prompt
- OpenAI OAuth: `getOpenaiCodexOauthStatus()`, `loginOpenaiCodexOauth()`, `logoutOpenaiCodexOauth()`, `setOpenaiFastMode(enabled)`

**Persisted:**
- AI Polish enabled → localStorage (`AI_POLISH_ENABLED_KEY`)
- API key → Tauri keyring (secure)
- Provider, model, prompt → Tauri backend via `setLlmProviderConfig`

---

### 2.7 Assistant (data-nav-id: `assistant`)

**Purpose:** Separate AI assistant for generating responses, independent of AI Polish.

**Enable toggle:**
- Turn on/off assistant feature

**Hotkey capture:**
- Button shows current combo or "Set hotkey..."
- Click to capture new hotkey
- Clear button to remove hotkey

**Provider & model selection:**
- **Use same provider as AI Polish**: Checkbox
  - If ON: Use AI Polish provider/model/auth
  - If OFF: Pick separate provider & model
- **Provider dropdown**: Same list as AI Polish (with custom providers)
- **Model dropdown**: Search + fetch-on-API-key-entry
- **Reasoning mode picker**: provider_default | off | light | balanced | deep

**Screen context toggle:**
- Capture screen for vision input to assistant (if model supports)

**System prompt field:**
- **Input**: Long-form text area
- **Placeholder**: "You are a helpful assistant..."
- **Hint**: "Guides assistant behavior"
- **Debounced save** (900ms)

**API Key:**
- **SecretInput** (masked)
- Only shown if using separate model from AI Polish
- Falls back to AI Polish key if empty & same provider

**Calls made:**
- `setAssistantHotkey(shortcut)` → register hotkey
- `setAssistantSystemPrompt(prompt)` → save system prompt
- `setAssistantScreenContextEnabled(enabled)` → toggle screen context
- `setAssistantApiKey(apiKey)` → set separate API key
- `getAssistantApiKey()` → fetch key on mount
- `setLlmProviderConfig(...)` → save assistant-specific settings (provider, model, reasoning, separate flag)
- Same model list fetching as AI Polish

**Persisted:**
- Hotkey → backend (`setAssistantHotkey`)
- Prompt → backend (`setAssistantSystemPrompt`)
- Settings → backend via `setLlmProviderConfig`

---

### 2.8 Translation (data-nav-id: `translation`)

**Hotkey capture:**
- Capture button shows current combo or "Set hotkey..."
- Clear button to remove hotkey
- **Purpose**: Trigger real-time translation of recognized speech

**Target language picker (expandable):**
- **Preset languages**: English, 日本語, 한국어, Français, Deutsch, Español, Русский, Português
- **Off option**: Disable translation
- **Custom language input**: Type any language name (e.g., "Italian")
- **Hint**: "AI will auto-enable polish if not already on"

**Status display:**
- Shows current target language or "Not enabled"

**Calls made:**
- `setTranslationHotkey(shortcut)` → register hotkey
- `setTranslationTarget(language)` → set target language

**Persisted:**
- Hotkey → backend
- Target language → backend (`setTranslationTarget`)

---

### 2.9 Vocabulary / Smart Vocabulary (data-nav-id: `vocabulary`)

**Hot words management:**
- **Add input field**: Text input + button
- **Hot word list**: Scrollable pills, sorted by weight & use_count
  - Color-coded by source (manual = accent color, learned = warning color)
  - Remove button (X) on each pill
  - Display count: "N hot words learned from M transcriptions"

**Correction rules management (modal):**
- **Button**: "Manage" link
- **Modal displays**:
  - List of correction patterns (original → corrected)
  - Filters: All | User-added | AI-learned
  - Search by original or corrected text
  - Delete button per rule
  - Source legend (user = accent, ai = learned, learned = warning)

**Validation system** (nested inside correction modal):
- **Enable validation toggle**: Check corrections against model
- **Separate model option**: Use different LLM for validation vs polish
- **Provider/model selection** (if separate)
- **Validate button**: Runs validation, shows results
- **Results display**: "Checked N rules in Xs. M corrections confirmed."

**Calls made:**
- `addHotWord(text, weight)` → add hot word
- `removeHotWord(text)` → remove hot word
- `removeCorrection(original, corrected)` → delete rule
- `validateCorrections()` → run validation
- `setCorrectionValidationConfig(params)` → toggle validation & settings
- `getUserProfile()` → fetch hot words & corrections on mount

**Persisted:**
- All via backend (profile)

---

### 2.10 Web Search (inside assistant section, sub-section)

**Enable toggle:**
- Turn on/off web search for assistant

**Provider picker:**
- **model_native**: Built-in model search (no API key needed)
- **exa**: Exa.ai search provider
- **tavily**: Tavily API provider

**Max results slider:**
- Range 1–10 (only shown for non-native providers)

**API keys:**
- **Exa**: (Not exposed in current UI, managed internally)
- **Tavily**: SecretInput for API key (debounced save)

**Calls made:**
- `setWebSearchConfig(enabled, provider, maxResults)` → toggle & settings
- `setWebSearchApiKey(apiKey)` → save Tavily key
- `getWebSearchApiKey()` → fetch key on mount

**Persisted:**
- Settings → backend, api keys → keyring

---

### 2.11 Data / Import-Export & Permissions & Startup & Update (data-nav-id: `misc` + others)

**Export/Import profile:**
- **Export button**: Downloads `light-whisper-profile.json` with all user settings, hot words, corrections
- **Import button**: File picker, accepts `.json`, imports profile
- Calls: `exportUserProfile()`, `importUserProfile(jsonData)`

**Permissions panel:**
- **Accessibility / Paste test**: Button to test paste function via accessibility API
- Call: `pasteText(testString, method)` → toast feedback

**Autostart at login:**
- **Toggle switch**: Enable/disable app launch on system boot
- Calls: `enableAutostart()`, `disableAutostart()`, `isAutostartEnabled()`

**Update checker:**
- **Check for updates button**: Queries GitHub releases
- **Status display**: Current version (from `getVersion()`), latest available (if update found)
- **Download link**: Opens GitHub release page
- Calls: `checkAppUpdate()`, `openAppReleasePage(url)`

---

### 2.12 Settings Page — All Tauri API Calls Summary

```
Engine:
  getEngine, setEngine, getAlibabaAsrConfig, setAlibabaAsrModel, listAlibabaAsrModels
  getOnlineAsrApiKey, setOnlineAsrApiKey, getOnlineAsrEndpoint, setOnlineAsrEndpoint
  getModelsDir, pickFolder, setModelsDir

Hotkey & Recording:
  registerCustomHotkey, registerAssistantHotkey, unregisterAllHotkeys, getHotkeyDiagnostic
  setRecordingMode

Microphone:
  listInputDevices, setInputDevice, testMicrophone
  startMicrophoneLevelMonitor, stopMicrophoneLevelMonitor

Input Method:
  setInputMethodCommand, setSoundEnabled, pasteText

AI Polish:
  setAiPolishConfig, getAiPolishApiKey, setAiPolishScreenContextEnabled
  listAiModels, setLlmProviderConfig, getLlmReasoningSupport
  addCustomProvider, updateCustomProvider, removeCustomProvider
  setCustomPrompt, getOpenaiCodexOauthStatus, loginOpenaiCodexOauth, logoutOpenaiCodexOauth
  setOpenaiFastMode
Assistant & Translation:
  setAssistantHotkey, setAssistantSystemPrompt, setAssistantScreenContextEnabled
  setAssistantApiKey, getAssistantApiKey
  setTranslationHotkey, setTranslationTarget

Vocabulary & Validation:
  addHotWord, removeHotWord, removeCorrection
  validateCorrections, setCorrectionValidationConfig
  getUserProfile, exportUserProfile, importUserProfile

Web Search:
  setWebSearchConfig, setWebSearchApiKey, getWebSearchApiKey

Autostart:
  enableAutostart, disableAutostart, isAutostartEnabled

Update:
  checkAppUpdate, openAppReleasePage, getVersion (via Tauri)

Profile:
  submitUserCorrection (from MainPage)
```

---

## 3. Subtitle Overlay (src/pages/SubtitleOverlay.tsx)

**Purpose:** Floating bottom-screen window showing real-time transcription, AI polish progress, and assistant output.

**Phases & display content:**
- **idle**: Window hidden, waiting for recording state
- **recording**: Waveform bars animation, "Listening..." text
- **processing**: Shows interim transcription, "Recognizing..." indicator
- **searching**: Assistant web search progress, "Searching the web..." 
- **polishing**: AI Polish progress, token count ("Polishing... 42 tokens"), smooth streaming text
- **result**: Final transcription or assistant response, clickable (copy + auto-dismiss after 2s)

**Real-time text streaming:**
- Interim transcription during recording/processing (via `useSmoothText` hook for smooth grapheme-by-grapheme animation)
- Polishing progress via `ai-polish-status` event (tokens, fallback signals)
- Assistant response via `assistant-stream` event (chunked text, auto-scroll)

**Waveform visualization:**
- Animated bars during recording phase
- Received via backend event

**Assistant panel:**
- Interactive when result shows assistant response
- Copy button (shows checkmark briefly)
- Auto-scroll on new chunks
- Dismiss button

**Theme sync:**
- Reads `THEME_STORAGE_KEY` from localStorage (light/dark/system)
- Listens to window `storage` event for live theme switching
- Syncs language via `LANGUAGE_STORAGE_KEY`

**Event listeners:**
- `recording-state`: Recording on/off, mode (dictation vs assistant), processing flag
- `transcription-result`: Interim & final text, duration, language, mode
- `ai-polish-status`: Polishing progress, token count, fallback signals
- `assistant-stream`: Assistant response chunks, session ID tracking

---

## 4. Components (src/components/)

### TitleBar.tsx
- Left action slot (default: Settings button)
- Title text (center)
- Right action slots (default: Minimize, Close buttons)
- Accessible labels on all buttons

### RecordingButton.tsx
- **States**: Ready (listening), Recording, Processing
- **Shapes/colors**: Changes based on state
- **Interaction**: Click to toggle (if toggle mode) or visual feedback (if hold mode)
- **Disabled when**: Model not ready, or in incompatible state
- **Accessible**: `role="button"`, ARIA labels

### TranscriptionResult.tsx
- **Editable text area**: Contenteditable or textarea
- **Stats line**: Character count, duration, CPM (chars/min), language
- **Copy button**: Copies to clipboard
- **Edit callback**: Reports changes to parent (MainPage)
- **Draft mode**: Shows unsaved indicator if in progress

### TranscriptionHistory.tsx
- **List of past items**: Each with timestamp
- **Copy button per item**: Context menu or inline button
- **Delete/clear**: Likely via swipe or context menu (not explicitly visible in code)
- **Virtualization**: Large lists possible but not evident in current code
- **Click to re-edit**: Likely selects item for editing (behavior not fully visible)

### StatusIndicator.tsx
- **Device info**: Display name, GPU name
- **Model status**: "Ready", "Loading", "Error", "Downloading"
- **Download progress**: Percentage bar + cancel button
- **Error retry**: Retry button if model failed
- **Recording button slot**: Renders RecordingButton inside
- **State-dependent UI**: Hide/show elements based on stage

### SecretInput.tsx
- **Masked input** for API keys
- **Show/hide toggle** (eye icon)
- **Focused/blurred styles**
- **Accessible**: aria-label support

### Kbd.tsx
- **Display formatted keyboard combo**: "Cmd+Shift+R", "Ctrl+Alt+M"
- **Platform-aware**: Shows Cmd on macOS, Ctrl on Windows/Linux
- **Styling**: Badge-like appearance

---

## 5. Hooks (src/hooks/)

### useRecording.ts
- **Subscribes to Tauri events**: `recording-state`, `recording-error`, `transcription-result`, `transcription-waveform`, `ai-polish-status`, `assistant-stream`
- **Manages state**: isRecording, isProcessing, transcriptionResult, originalAsrText, editBaselineText, history, charCount, duration, language
- **Exposes methods**: startRecording(), stopRecording(), setTranscriptionResult(), setEditBaselineText()
- **Session tracking**: Avoids stale updates with session ID filtering
- **History management**: Appends final results with timestamps
- **Error handling**: Captures error messages from backend

### useModelStatus.ts
- **Subscribes to**: `funasr-status` event (model loading, device info, GPU)
- **Stages**: "waiting" | "error" | "downloading" | "loading" | "ready"
- **Exposes**: stage, isReady, device, gpuName, downloadProgress, downloadMessage, isDownloading, error
- **Methods**: downloadModels(), cancelDownload(), retry()
- **Auto-retry**: On model failures, with exponential backoff

### useHotkey.ts
- **Subscribes to**: `hotkey-status` event (registration success/failure, diagnostic)
- **Displays**: Hotkey combo, error message, system conflict warnings
- **Methods**: setHotkey(shortcut)
- **Validation**: Checks for system conflicts, shows diagnostic info
- **Persistent state**: Reads from backend via `getHotkeyDiagnostic()`

### useHotkeyCapture.ts
- **Modal interaction**: Starts key capture when user clicks button
- **Display states**: "Capturing..." vs "Press any key..."
- **Debounce**: Prevents accidental double-registration
- **Callbacks**: onSave (async), onCancel
- **Saving state**: Shows loading indicator during save

### useExclusivePicker.ts
- **Mutual exclusion**: Only one picker open at a time
- **Ref management**: `setRef(id)` for each picker container
- **Methods**: `isOpen(id)`, `toggle(id)`, `open(id)`, `close(id)`, `popoverClass(id)`
- **Popover positioning**: Calculated based on container size & viewport
- **Keyboard support**: Likely Escape to close (inferred from picker pattern)

### useDebouncedCallback.ts
- **Debounces async callbacks**: Delays execution by N ms
- **Methods**: `schedule(...args)`, `cancel()`, `flush()` (immediate execute)
- **Cleanup**: `onUnmount` option ("cancel" | "flush")
- **Usage**: API key saves, custom prompt saves (900ms debounce typical)

### useTheme.ts
- **State**: isDark, theme (light/dark/system)
- **Methods**: setTheme(mode)
- **Persistence**: localStorage (`THEME_STORAGE_KEY`)
- **System sync**: Watches `prefers-color-scheme` media query
- **DOM attribute**: Sets `data-theme` on `<html>`

### useSmoothText.ts
- **Grapheme-aware**: Splits text by grapheme clusters (CJK, emoji safe)
- **Smooth animation**: Returns segmented text chunks for staggered display
- **Purpose**: Smooth streaming text in UI (subtitle overlay, assistant panel)
- **Helper**: `segmentGraphemes(text)` → array of visible chars/clusters

---

## 6. Contexts (src/contexts/)

### RecordingContext.tsx

**Provides:**
```typescript
RecordingContextValue {
  // Recording
  isRecording, isProcessing, startRecording, stopRecording,
  recordingError, transcriptionResult, setTranscriptionResult,
  originalAsrText, editBaselineText, setEditBaselineText,
  durationSec, charCount, detectedLanguage, history, recordingMode,
  
  // Model
  stage, isReady, device, gpuName, downloadProgress, downloadMessage,
  isDownloading, modelError, downloadModels, cancelDownload, retryModel,
  
  // Hotkey
  hotkeyDisplay, hotkeyError, setHotkey, hotkeyDiagnostic
}
```

**Initialization:**
- On mount, syncs localStorage settings to backend:
  - Input method (clipboard vs direct)
  - Input device
  - Sound enabled flag
  - Recording mode (toggle vs hold)
  - AI Polish enabled + API key

**Used by:** MainPage, SettingsPage (read), SubtitleOverlay (read)

---

## 7. Tauri API Surface (src/api/tauri.ts)

### Commands (invoke calls)

**No-arg commands** (76 total):
```
startFunASR, checkAppUpdate, checkFunASRStatus, checkModelFiles,
downloadModels, cancelModelDownload, restartFunASR, getEngine,
hideMainWindow, showSubtitleWindow, hideSubtitleWindow,
getOpenaiCodexOauthStatus, loginOpenaiCodexOauth, logoutOpenaiCodexOauth,
unregisterAllHotkeys, startRecording, stopRecording, testMicrophone,
listInputDevices, startMicrophoneLevelMonitor, stopMicrophoneLevelMonitor,
getUserProfile, exportUserProfile, getHotkeyDiagnostic
```

**Parameterized commands** (examples):
```
setEngine(engine: string)
copyToClipboard(text: string)
pasteText(text: string, method?: "sendInput" | "clipboard")
registerCustomHotkey(shortcut: string)
registerAssistantHotkey(shortcut: string)
setInputDevice(name?: string | null)
setInputMethodCommand(method: string)
setSoundEnabled(enabled: boolean)
setAiPolishConfig(enabled: boolean, apiKey: string)
getAiPolishApiKey()
setAiPolishScreenContextEnabled(enabled: boolean)
listAiModels(provider: string, baseUrl?: string, apiKey: string)
addHotWord(text: string, weight: number)
removeHotWord(text: string)
setLlmProviderConfig(active, customBaseUrl?, customModel?, polishReasoningMode?, 
                     assistantReasoningMode?, assistantUseSeparateModel?, assistantModel?,
                     assistantProvider?, openaiAuthMode?)
setAssistantApiKey(apiKey: string)
getAssistantApiKey()
getLlmReasoningSupport(provider, model, baseUrl, apiKey)
importUserProfile(jsonData: string)
submitUserCorrection(original, corrected, rawOriginal?)
setRecordingMode(toggle: boolean)
setTranslationTarget(target: string | null)
setTranslationHotkey(shortcut: string | null)
setCustomPrompt(prompt: string | null)
setOpenaiFastMode(enabled: boolean)
setAssistantHotkey(shortcut: string | null)
setAssistantSystemPrompt(prompt: string | null)
setAssistantScreenContextEnabled(enabled: boolean)
setWebSearchConfig(enabled, provider, maxResults)
setWebSearchApiKey(apiKey: string)
getWebSearchApiKey()
addCustomProvider(name, baseUrl, model, format)
updateCustomProvider(id, name?, baseUrl?, model?, format?)
removeCustomProvider(id: string)
removeCorrection(original, corrected)
validateCorrections()
setCorrectionValidationConfig(params)
setOnlineAsrApiKey(apiKey, keyringUser?)
getOnlineAsrApiKey()
getOnlineAsrEndpoint()
setOnlineAsrEndpoint(region: string)
getAlibabaAsrConfig()
setAlibabaAsrModel(model: string)
listAlibabaAsrModels()
getModelsDir()
pickFolder()
setModelsDir(path: string | null, migrate: boolean)
```

**Autostart plugin** (re-exported):
```
enableAutostart(), disableAutostart(), isAutostartEnabled()
```

### Event Listeners

**Backend events** (frontend subscribes via `listen()`):
```
recording-state: { sessionId, isRecording, isProcessing, error?, mode? }
recording-error: { message: string; sessionId? }
transcription-result: { sessionId?, text, interim, durationSec?, charCount?, language?, mode?, originalText? }
transcription-waveform: { bars: number[] }
ai-polish-status: { status: "polishing" | "fallback" | "streaming", tokens?, sessionId? }
assistant-stream: { sessionId?, chunk?, status? }
microphone-level: { deviceName?, level? }
hotkey-status: { registered: boolean, error?: string, diagnostic? }
funasr-status: { running, ready, model_loaded, device?, gpu_name?, message, ... }
```

---

## 8. Internationalization (src/i18n/)

### Key namespaces (from en.ts):

```
common: (copy, close, retry, cancel, settings, minimize, loading, change, back, add, clear, show, hide, test, refresh)
app: (title, errorRestart)
main: (quickStart, pressHotkey, hotkeyHintToggle, hotkeyHintHold, autoInputHint, settingsHint)
status: (online, listening, recognizing, clickToStart, downloadingModel, modelLoading, ...)
recording: (stop, processing, start)
result: (title, recognizingSpeech, stats, editableTranscription)
subtitle: (aiListening, listening, recognizing, polishing, polishingWithTokens, webSearching, ...)
toast: (correctionRecorded, correctionFailed, aiPolishApplied, aiPolishFailed, switchedToEngine, ...)
model: (engineStartFailed, slowNetwork, downloadFailed, reasoningDetecting, reasoningUnavailable, ...)
settings: (
  title, appearance, themeLight, themeDark, themeSystem, language,
  engine, sensevoiceDesc, whisperDesc, glmAsrDesc, alibabaAsrDesc, alibabaAsrLabel,
  hotkeySection, recordingMode, holdToTalk, toggleMode, hotkeyLabel, hotkeyHint,
  microphone, levelMonitor, selectMic, followSystemMic, systemDefaultDevice, testMicrophone,
  micLevelMonitor, micMonitorOff, micRecordingPaused, micSpeakToTest,
  inputMethod, directInput, clipboardPaste, recordingSound,
  aiPolish, enableAiPolish, screenContext, provider, selectProvider, searchProvider,
  addCustomProvider, providerName, providerBaseUrl, defaultModel, apiFormat,
  openaiCompat, baseUrl, apiKey, model, openModelList, searchModel,
  fetching, fetchModelsFromApi, fillApiKeyToLoadModels, fillApiKeyOrLogin,
  codexOauthLabel, codexOauthHint, codexOauthLogin, codexOauthReauth, codexOauthLogout,
  fastModeLabel, fastModeHint, openaiAuthModeLabel, openaiAuthModeApiKey, openaiAuthModeOauth,
  polishReasoningMode, customPrompt, customPromptPlaceholder,
  assistant, assistantHotkey, assistantSystemPrompt, assistantScreenContext,
  translation, translationHotkey, targetLanguage, selectLanguage, customLanguage,
  vocabulary, hotWordsCount, addHotWord, correctionRules, correctionManage, correctionRulesCount,
  data, exportConfig, importConfig,
  permissions, accessibilityPaste, testPasteContent,
  startup, autostart,
  update, checkAppUpdate, currentVersion, newVersionAvailable, checkUpdate, goToDownload,
  webSearch, webSearchEnabled, webSearchProvider, webSearchModelNative, webSearchExa, webSearchTavily,
  webSearchMaxResults, webSearchTavilyApiKeyLabel, webSearchTavilyKeyPlaceholder,
  footer, footerSubtitle
)
```

### Language files:
- **en.ts** (English) — ~800 keys
- **zh.ts** (Simplified Chinese) — ~800 keys (parallel structure)

---

## 9. File Structure (Complete src/)

```
src/
├── pages/
│   ├── MainPage.tsx                    (~300 lines)
│   ├── SettingsPage.tsx                (~4196 lines) ⚠️ LARGEST FILE
│   ├── SubtitleOverlay.tsx             (~700 lines)
│   └── __tests__/
│       └── SettingsPage.online-asr-only.test.tsx
├── components/
│   ├── Kbd.tsx                         (~50 lines)
│   ├── RecordingButton.tsx             (~100 lines)
│   ├── SecretInput.tsx                 (~150 lines)
│   ├── StatusIndicator.tsx             (~200 lines)
│   ├── TitleBar.tsx                    (~100 lines)
│   ├── TranscriptionHistory.tsx        (~150 lines)
│   ├── TranscriptionResult.tsx         (~250 lines)
│   └── __tests__/
│       └── StatusIndicator.test.tsx
├── hooks/
│   ├── useDebouncedCallback.ts         (~100 lines)
│   ├── useExclusivePicker.ts           (~150 lines)
│   ├── useHotkey.ts                    (~200 lines)
│   ├── useHotkeyCapture.ts             (~200 lines)
│   ├── useModelStatus.ts               (~250 lines)
│   ├── useRecording.ts                 (~250 lines)
│   ├── useSmoothText.ts                (~150 lines)
│   ├── useTheme.ts                     (~100 lines)
│   └── __tests__/
│       └── useRecording.test.tsx
├── contexts/
│   ├── RecordingContext.tsx            (~150 lines)
│   └── __tests__/
│       └── RecordingContext.test.tsx
├── api/
│   ├── tauri.ts                        (~400 lines)
│   └── tauri.fastMode.test.ts
├── lib/
│   ├── constants.ts                    (~30 lines)
│   ├── storage.ts                      (~50 lines)
│   ├── hotkey.ts                       (~100 lines)
│   ├── fastMode.ts                     (~100 lines)
│   └── fastMode.test.ts
├── i18n/
│   ├── index.ts                        (~50 lines)
│   ├── en.ts                           (~1000 lines)
│   └── zh.ts                           (~1000 lines)
├── types/
│   └── index.ts                        (~300 lines)
├── styles/
│   ├── theme.css
│   ├── subtitle.css
│   └── [other css files]
├── test/
│   ├── setup.ts
│   └── tauriEventMock.ts
├── main.tsx
└── vite-env.d.ts
```

**Total source files:** ~35 TypeScript/TSX files  
**Largest files:**
1. SettingsPage.tsx — 4196 lines
2. en.ts, zh.ts — ~1000 lines each
3. tauri.ts — 400 lines
4. types/index.ts — 300 lines

---

## 10. Permission & Capability Requirements

### Tauri Plugin Permissions (inferred from API usage)

- **Clipboard**: `copyToClipboard()`, `pasteText()`
- **Window**: `getCurrentWindow().minimize()`, `hideMainWindow()`, `showSubtitleWindow()`, `hideSubtitleWindow()`
- **Autostart**: `enableAutostart()`, `disableAutostart()`, `isAutostartEnabled()`
- **App**: `getVersion()`, `checkAppUpdate()`, `openAppReleasePage()`
- **Keyring/Secure Storage**: API keys, OAuth tokens (implicit, backend-managed)
- **Accessibility** (macOS): Required for hotkey capture, paste function, screen context
- **Audio**: Microphone input (backend-managed via FunASR)
- **Filesystem**: Model directory access, import/export JSON profiles

### macOS-specific Permissions
- **Microphone access**: Prompted by system, checked by backend
- **Accessibility (Input Monitoring)**: Required for global hotkey registration + keyboard input simulation
- **Screen Recording**: Required if screen context for AI Polish/Assistant enabled
- **AppleScript/Automation**: May be needed for inter-app focus & clipboard interaction

---

## 11. Key State Management Patterns

### Local Storage (frontend-only)
- `ONBOARDING_DISMISSED_KEY`: "true" after first successful transcription
- `RECORDING_MODE_KEY`: "toggle" or "hold" (default)
- `INPUT_METHOD_KEY`: "clipboard" or "sendInput"
- `INPUT_DEVICE_STORAGE_KEY`: Device name (or empty for system default)
- `SOUND_ENABLED_KEY`: "true" / "false"
- `AI_POLISH_ENABLED_KEY`: "true" / "false"
- `MIC_LEVEL_MONITOR_ENABLED_KEY`: "true" / "false"
- `THEME_STORAGE_KEY`: "light" / "dark" / "system"
- `LANGUAGE_STORAGE_KEY`: "en" / "zh" / etc.
- `LLM_PROVIDER_DRAFTS_KEY`: JSON serialized provider configs (unsaved edits)

### Backend (Tauri persistent)
- User profile (hot words, corrections, transcription count)
- LLM provider config (active provider, models, API keys via keyring)
- ASR engine choice
- Translation settings (target language, hotkey)
- Assistant settings (hotkey, system prompt, provider)
- Web search config
- Correction validation settings

### Session State (React, lost on app restart)
- Recording state, transcription text, history (current session only)
- UI picker/modal open states
- Debounced API call timers
- Hotkey capture state

---

## 12. Critical Data Flows & Workflows

### Recording to Transcription
1. **User presses hotkey** → `useHotkey` detects key
2. **MainPage calls** `startRecording()` → Backend starts audio capture
3. **Backend emits `recording-state`** → `useRecording` updates UI state
4. **Backend emits `transcription-result` (interim)** → Subtitle overlay shows partial text, MainPage updates
5. **User releases hotkey** → `stopRecording()` called
6. **Backend processes** → Final transcription + optional AI Polish
7. **Backend emits `transcription-result` (final)** → Full result displayed, added to history
8. **User edits text** → In dictation mode, diff submitted as correction

### AI Polish Workflow
1. **AI Polish enabled** in settings
2. **Final transcription received** from ASR
3. **Backend calls LLM** (provider from settings)
4. **Backend emits `ai-polish-status`** → Subtitle overlay shows "Polishing..."
5. **LLM streams response** → Tokens emitted in real-time
6. **Final polished text** replaces transcription in UI
7. **User can edit** polished result (not tracked as correction in this mode)

### Assistant Mode
1. **User presses assistant hotkey**
2. **Recording mode switches** to "assistant" (different recording profile)
3. **Transcription completed** → Treated as user prompt (not auto-pasted)
4. **Backend calls LLM** with system prompt + user message
5. **LLM streams response** → Subtitle overlay shows chunks in real-time
6. **User can copy** final response, auto-disappears after 2s if not interacted
7. **No auto-paste** in assistant mode (user reads in overlay)

### Translation Workflow
1. **Translation hotkey pressed** during or after recording
2. **Backend translates** transcription to target language
3. **AI Polish auto-enabled** if not already (per UI logic)
4. **Result displayed** in subtitle overlay
5. **Output method** same as regular transcription (paste)

---

## 13. Notable Features & Behaviors

### Hotkey System
- **Global hotkey registration**: Requires accessibility permissions (macOS)
- **Diagnostic feedback**: System conflicts detected, warnings displayed
- **Default**: F2 (customizable)
- **Three separate hotkeys**: Recording, Translation, Assistant (each optional)

### Model Download & Management
- **Local models** (Whisper, SenseVoice): Auto-downloaded on first use
- **Models directory**: Customizable, default to app data folder
- **Migration**: Moving directory preserves files
- **Progress**: Real-time download % shown in status indicator
- **Cancellation**: User can cancel in-progress downloads

### Input Methods
- **Direct Input**: Keyboard simulation (accessibility API)
- **Clipboard Paste**: Copy to clipboard, let app paste (CJK-friendly)
- **Recording sound**: Optional audible feedback during input

### Correction Learning
- **User edits** in dictation mode recorded as corrections
- **AI learns** from corrections over time
- **Correction rules modal**: View, filter, delete learned patterns
- **Validation**: Optional separate LLM to verify corrections

### Web Search
- **For assistant mode**: Augment responses with live web data
- **Providers**: Model-native (no key), Exa (key), Tavily (key)
- **Max results**: Configurable 1–10

### Screening System (Secondary AI Model)
- **Correction validation**: Optional LLM to fact-check transcriptions
- **Separate model option**: Can use different provider than main Polish
- **Batch validation**: Run on all corrections to flag incorrect ones

### Reasoning Modes (LLM)
- **provider_default**: Use model's default strategy
- **off**: Disable or minimize reasoning
- **light**: Fast, direct responses (prefer speed)
- **balanced**: Trade-off
- **deep**: Thorough reasoning (slower, more expensive)
- **Auto-detection**: Frontend probes model capability on provider/model change
- **Fallback**: If detection fails, treated as unsupported

### OpenAI Codex OAuth
- **Sign in with ChatGPT**: OAuth flow (browser popup)
- **Fast mode**: ChatGPT priority processing (~1.5× faster, ~2× cost)
- **Plan detection**: Shows Plus/Pro status, account summary
- **API key fallback**: Manual key takes priority if both set
- **Auto-mode selection**: Intelligently defaults to oauth if signed in

### Screen Context (Vision)
- **AI Polish**: Capture screen content to assist polishing
- **Assistant**: Include screen in assistant prompts
- **Fallback**: Models without image support skip image, use text only
- **Accessibility**: Requires screen recording permission (macOS)

---

## 14. Edge Cases & Error Handling

### Model Loading Failures
- **Symptom**: Model not found or download failed
- **Recovery**: Retry button, exponential backoff, detailed error messages
- **Logging**: Backend logs to `funasr_stderr.log` for troubleshooting

### Microphone Issues
- **Unavailable device**: Falls back to system default with warning
- **No devices**: Error message, user must select from list refresh
- **Permission denied**: Requires system settings change

### API Key Expiry
- **OAuth token**: Expires, requires re-sign-in
- **Manual API key**: No automatic refresh; user updates in settings

### Hotkey Conflicts
- **System already owns**: Diagnostic shows conflict, user chooses alternate
- **App registration fails**: Error message, suggest common shortcuts
- **No permission**: Accessibility required, guide user to system settings

### Network Issues (Online ASR / LLM)
- **Slow network**: Warning banner, auto-retry on timeout
- **API unavailable**: Error toast, user can retry manually
- **Rate limited**: Backend queues or returns 429, user retries later

### File I/O
- **Profile import**: Validates JSON, rolls back on parse error
- **Models directory migration**: Warns before moving, preserves original
- **Folder picker cancelled**: Silent no-op (user cancels file dialog)

---

## 15. Browser & Runtime Environment

### Tauri Runtime
- **Webview**: Platform-native (WebKit on macOS, WebView2 on Windows)
- **IPC**: JSON-serialized commands + events between frontend & Rust backend
- **File system**: Full access to user's documents, models folder, config
- **Permissions**: Per-platform (macOS requires explicit accessibility grants)

### React + TypeScript
- **Version**: Modern (hooks, suspense, context)
- **State management**: React Context + component state (no Redux/Zustand)
- **Styling**: CSS modules or inline styles (imported from theme.css)
- **Accessibility**: ARIA labels, semantic HTML, keyboard support

### Build Tools
- **Vite**: Build tool (vite-env.d.ts)
- **i18n**: react-i18next for translations

---

## 16. Summary Table: ASR Engines

| Engine | Type | Local/Online | Key Identifier | Description | Key Settings |
|--------|------|--------------|---|---|---|
| SenseVoice | LOCAL | Local | sensevoice | CN/EN/JP/KR/Cantonese, multilingual | Model download dir |
| Faster Whisper | LOCAL | Local | whisper | 99+ languages, fast | Model download dir |
| GLM-ASR | ONLINE | Online | glm-asr | Zhipu-powered, Chinese optimized | API key, endpoint |
| Alibaba DashScope | ONLINE | Online | alibaba-asr | Qwen ASR & Omni models, regional | API key, region, model choice |

**Local engines** will be dropped on apple branch.  
**Online engines** require API keys (managed in Settings > Engine sections).

---

## 17. Completeness Checklist

- [x] Main Page widgets, interactions, state
- [x] All 9 SettingsPage sections enumerated with controls, API calls, persisted state
- [x] ASR engines (4 total, 2 local + 2 online)
- [x] AI Polish providers, OAuth, reasoning modes, custom providers
- [x] Assistant hotkey, separate model support, system prompt
- [x] Translation hotkey, target language, web search integration
- [x] Hotkey system (recording, translation, assistant, global registration)
- [x] Clipboard & input method strategies
- [x] Subtitle overlay phases, event listeners, themes
- [x] Microphone device selection, level monitoring
- [x] Permissions (accessibility, microphone, screen recording, clipboard)
- [x] Autostart at login
- [x] Theme (light/dark/system), language (en/zh)
- [x] File structure and largest files identified
- [x] All Tauri API commands and event listeners
- [x] i18n namespace overview
- [x] State management patterns (localStorage, backend, session)
- [x] Critical workflows (recording → polish, assistant, translation)
- [x] Error handling and edge cases
- [x] Hot words, corrections, validation
- [x] Profile import/export
- [x] Update checking
- [x] Custom LLM providers (add, edit, delete)
- [x] Web search (model-native, Exa, Tavily)

---

## 18. Notes for UI Rewrite

### High Priority (Core Features)
1. Recording button + hotkey binding
2. Transcription result display & editing
3. Settings navigation + appearance
4. Engine selection & switching
5. Microphone device picker
6. LLM provider + model selection
7. API key inputs (SecretInput component reusable)

### Medium Priority (Advanced Features)
8. Hotkey capture modal
9. Correction rules management (modal)
10. Custom LLM provider addition
11. AI Polish reasoning mode selector
12. Assistant hotkey & system prompt
13. Translation hotkey & language picker
14. Web search config
15. Profile import/export

### Low Priority (Admin)
16. Update checker
17. Autostart toggle
18. Permissions test button
19. Models directory management

### Reusable Components
- **SecretInput**: Masked API key field (reuse across all API key inputs)
- **Kbd**: Keyboard combo display (hotkey capture, recording mode hints)
- **Picker dropdown**: Heavily used (engine, provider, model, device, theme, language)
  - Consider extracting as a shared component if heavily customized
- **StatusIndicator**: Already modular, mostly logic (no big refactor needed)

### State Preservation
- Ensure localStorage keys remain unchanged (or provide migration)
- Tauri backend APIs must be called with same argument shapes
- Event listeners: `listen()` subscriptions must handle same event payloads

### Testing Surface
- 76 Tauri API commands to test (mock or E2E)
- 10+ event listeners (mock backend events)
- 9 settings sections (functional tests per section)
- i18n: Verify all keys present in both en.ts and zh.ts

---

**End of Feature Inventory**
