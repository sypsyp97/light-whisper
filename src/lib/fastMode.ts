import type { OpenaiAuthMode } from "@/types";

export interface FastModeToggleGate {
  /** Which Codex OAuth block is being rendered — polish side or assistant side. */
  scope: "polish" | "assistant";
  /** Whether the user is signed in via ChatGPT OAuth. */
  loggedIn: boolean;
  /** Effective OpenAI auth mode (already resolved against login state). */
  authMode: OpenaiAuthMode;
  /** Active polish-side provider key. */
  llmProvider: string;
  /** Resolved assistant-side provider key. */
  effectiveAssistantProvider: string;
}

export interface EffectiveAssistantProviderGate {
  assistantUseSeparateModel: boolean;
  assistantProvider?: string | null;
  llmProvider: string;
  availableProviders?: readonly string[];
}

export interface AssistantProviderPersistInput {
  assistantUseSeparateModel: boolean;
  assistantProvider?: string | null;
  availableProviders?: readonly string[];
}

export interface AssistantModelStateInput {
  assistantUseSeparateModel: boolean;
  savedAssistantModel?: string | null;
  polishModel: string;
  assistantDefaultModel: string;
}

export interface AssistantProviderChangeModelInput {
  nextAssistantProvider: string;
  polishProvider: string;
  polishModel: string;
  nextProviderDefaultModel: string;
}

export interface AssistantModelToggleStateInput {
  enabled: boolean;
  previousAssistantUseSeparateModel: boolean;
  assistantProvider?: string | null;
  availableProviders?: readonly string[];
  savedAssistantModel?: string | null;
  polishModel: string;
  assistantDefaultModel: string;
}

export interface AssistantModelToggleState {
  assistantModel: string;
  assistantProviderState: string;
  assistantProviderToPersist: string | null;
}

export interface AssistantModelForPolishProviderChangeInput {
  assistantUseSeparateModel: boolean;
  assistantProviderToPersist?: string | null;
  savedAssistantModel?: string | null;
  nextPolishModel: string;
  assistantDefaultModel: string;
}

export function resolveEffectiveAssistantProvider(gate: EffectiveAssistantProviderGate): string {
  const assistantProvider = gate.assistantProvider?.trim();

  if (gate.assistantUseSeparateModel && assistantProvider) {
    if (!gate.availableProviders || gate.availableProviders.includes(assistantProvider)) {
      return assistantProvider;
    }
  }

  return gate.llmProvider;
}

export function resolveAssistantProviderToPersist(
  input: AssistantProviderPersistInput,
): string | null {
  const assistantProvider = input.assistantProvider?.trim();
  if (!input.assistantUseSeparateModel || !assistantProvider) {
    return null;
  }
  if (input.availableProviders && !input.availableProviders.includes(assistantProvider)) {
    return null;
  }
  return assistantProvider;
}

export function resolveAssistantModelState(input: AssistantModelStateInput): string {
  if (!input.assistantUseSeparateModel) {
    return input.polishModel.trim() || input.assistantDefaultModel;
  }

  return input.savedAssistantModel?.trim() || input.assistantDefaultModel;
}

export function resolveAssistantModelForProviderChange(
  input: AssistantProviderChangeModelInput,
): string {
  if (input.nextAssistantProvider === input.polishProvider) {
    return input.polishModel.trim() || input.nextProviderDefaultModel;
  }

  return input.nextProviderDefaultModel;
}

export function resolveAssistantModelToggleState(
  input: AssistantModelToggleStateInput,
): AssistantModelToggleState {
  const assistantProvider = input.assistantProvider?.trim();
  const canReuseExplicitProvider = Boolean(
    input.enabled
    && input.previousAssistantUseSeparateModel
    && assistantProvider
    && (!input.availableProviders || input.availableProviders.includes(assistantProvider)),
  );
  const assistantProviderToPersist = canReuseExplicitProvider ? assistantProvider ?? null : null;
  const assistantModel = canReuseExplicitProvider
    ? resolveAssistantModelState({
      assistantUseSeparateModel: true,
      savedAssistantModel: input.savedAssistantModel,
      polishModel: input.polishModel,
      assistantDefaultModel: input.assistantDefaultModel,
    })
    : resolveAssistantModelState({
      assistantUseSeparateModel: false,
      savedAssistantModel: input.savedAssistantModel,
      polishModel: input.polishModel,
      assistantDefaultModel: input.assistantDefaultModel,
    });

  return {
    assistantModel,
    assistantProviderState: assistantProviderToPersist ?? "",
    assistantProviderToPersist,
  };
}

export function resolveAssistantModelForPolishProviderChange(
  input: AssistantModelForPolishProviderChangeInput,
): string {
  if (!input.assistantUseSeparateModel) {
    return input.nextPolishModel;
  }

  if (!input.assistantProviderToPersist) {
    return input.nextPolishModel;
  }

  return input.savedAssistantModel?.trim() || input.assistantDefaultModel;
}

/**
 * Decide whether the Fast-mode toggle should render inside a given Codex OAuth
 * block.
 *
 * Contract:
 *  - Hidden unless the user is signed in AND has explicitly (or by default)
 *    picked OAuth as the OpenAI auth mode. Fast mode is ChatGPT-only per
 *    OpenAI docs; it must never surface for plain-API-key users.
 *  - Rendered inside the polish block when polish routes through OpenAI.
 *  - Rendered inside the assistant block ONLY when polish does NOT use
 *    OpenAI but assistant does — so assistant-only-OpenAI-OAuth users can
 *    still flip the flag. When polish already uses OpenAI, the polish block
 *    owns the toggle and the assistant block suppresses it to avoid two
 *    controls bound to the same flag.
 */
export function shouldShowFastModeToggle(gate: FastModeToggleGate): boolean {
  const { scope, loggedIn, authMode, llmProvider, effectiveAssistantProvider } = gate;

  if (!loggedIn) return false;
  if (authMode !== "oauth") return false;

  if (scope === "polish") {
    return llmProvider === "openai";
  }

  // scope === "assistant"
  return effectiveAssistantProvider === "openai" && llmProvider !== "openai";
}
