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
