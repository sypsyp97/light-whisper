/** Map backend search failure codes to localized, actionable messages. */
export function searchNoticeKey(message?: string): string {
  if (message?.includes("SEARCH_RATE_LIMITED")) return "subtitle.conversation.searchRateLimited";
  if (message?.includes("SEARCH_AUTH_REQUIRED")) return "subtitle.conversation.searchAuthRequired";
  if (message?.includes("SEARCH_ACCESS_DENIED")) return "subtitle.conversation.searchAccessDenied";
  if (message?.includes("SEARCH_TIMEOUT")) return "subtitle.conversation.searchTimeout";
  if (message?.includes("SEARCH_INVALID_RESPONSE")) return "subtitle.conversation.searchInvalidResponse";
  return "subtitle.conversation.searchFailed";
}
