# API defaults

Reviewed against official documentation on 2026-10-18. These defaults apply when
no model or connection has been saved. Explicit saved selections remain intact.
Model pickers use the authenticated upstream model catalog when available;
the built-in lists are discovery fallbacks, not model allowlists.

## Language models

| Provider | Default model | Effective request URL | Protocol / source |
| --- | --- | --- | --- |
| OpenAI API | `gpt-6-luna` | `https://api.openai.com/v1/responses` | [Responses; fast, low-cost model](https://developers.openai.com/api/docs/models/gpt-6-luna) |
| xAI API | `grok-4.7` | `https://api.x.ai/v1/responses` | [Responses](https://docs.x.ai/developers/grok-4-7) |
| DeepSeek | `deepseek-flash` | `https://api.deepseek.com/v1/responses` | [Responses and model aliases](https://api-docs.deepseek.com/quick_start/pricing/) |
| Cerebras | `qwen-3.8-27b` | `https://api.cerebras.ai/v1/chat/completions` | [Chat Completions](https://inference-docs.cerebras.ai/resources/openai) |
| SiliconFlow | `Qwen/Qwen3.8-27B` | `https://api.siliconflow.cn/v1/chat/completions` | [Chat Completions](https://docs.siliconflow.cn/docs/api/chat-completions-post), [model catalog](https://www.siliconflow.cn/models) |
| OpenRouter | `~deepseek/deepseek-flash-latest` | `https://openrouter.ai/api/v1/chat/completions` | [Chat Completions](https://openrouter.ai/docs/api-reference/chat-completions), [model catalog](https://openrouter.ai/models) |

OpenAI retains the small-model role previously filled by GPT-4.1 mini. The new
Cerebras and SiliconFlow Qwen defaults retain native thinking controls. Cerebras
Qwen supports `reasoning_effort: none`; legacy GPT-OSS handling stays unchanged.

DeepSeek's canonical Flash ID currently resolves to DeepSeek-V4.1-Flash-1017.
`deepseek-v4-flash` and `deepseek-v4-pro` keep their existing Responses routing;
legacy Chat-only model selections and custom gateways keep their original route.
The shared request builder selects the effective DeepSeek protocol for polishing,
assistant, selection and validation requests.
Image capability follows the selected model's upstream `input_modalities`, rather
than a provider-wide assumption. DeepSeek's authenticated `/v1/models` list is
matched by exact model ID; failed or inconclusive metadata queries remain unknown
and retain the existing image-request fallback. See [model metadata](https://api-docs.deepseek.com/api/list-models/)
and [image input](https://api-docs.deepseek.com/guides/vision/).
The [Responses compatibility guide](https://api-docs.deepseek.com/guides/responses_api/)
currently supports native `web_search` with Pro and ignores that tool with Flash;
protocol regression tests do not establish upstream search availability.

Cerebras and SiliconFlow do not currently document an OpenAI Responses endpoint.
Custom OpenAI-compatible connections default to Chat Completions and honor an
explicit `/responses` URL. Anthropic connections retain `/v1/messages`.

OpenRouter is detected by host, so both the built-in preset and an
OpenAI-compatible custom connection pointed at `openrouter.ai` share the same
behavior. Reasoning uses OpenRouter's unified `reasoning` object: the off tier
sends `reasoning.enabled = false`, and the light/balanced/deep tiers map to
`reasoning.effort` (`low`/`medium`/`high`).
Models flagged `mandatory` by OpenRouter reject
`reasoning.enabled = false`; the request then retries once without any
reasoning control and keeps the model default.

Chat Completions and Responses requests omit optional output-token limits, so
the provider chooses its default instead of sharing a fixed 4,096-token budget
between reasoning and visible output. This does not remove provider-side limits.
Anthropic Messages requires `max_tokens`: its default is 16,384, with one retry
at the previous 4,096-token budget if the model explicitly rejects the larger
limit. A successful downgrade is reused for that endpoint and model in memory.

Subscription authentication keeps its existing dedicated transport: OpenAI Codex
uses `https://chatgpt.com/backend-api/codex/responses`, and Grok Build uses
`https://cli-chat-proxy.grok.com/v1/responses`. Their model lists remain dynamic.
Ordinary API model discovery uses the corresponding provider's `/v1/models`.

## Decision models

Decision APIs return typed probabilities and are separate from Responses.

| Provider | Default model | Request URL | Official source |
| --- | --- | --- | --- |
| TypeSafe | `jev-latest` | `https://api.typesafe.ai/v1/systemone` | [Stable alias; currently Jev 1.13](https://docs.typesafe.ai/models) |
| OpenRouter | `~typesafe/jev-latest` | `https://openrouter.ai/api/alpha/decisions` | [Latest Jev alias](https://openrouter.ai/docs/guides/community/typesafe-sdk#model-ids) |
| Vercel | `typesafe-ai/jev` | `https://ai-gateway.vercel.sh/typesafe/v1/systemone` | [Public TypeSafe-compatible API](https://vercel.com/docs/ai-gateway/sdks-and-apis/typesafe) |
| Liquid | `d1` | `https://api.liquid.ai/decisions/v1/systemone` | [Current D1 API](https://docs.liquid.ai/lfm/models/d1) |

Saved Vercel `/v4/ai/evaluation-model` connections retain their old SDK headers
and body contract. New connections use the public typed API with `model` in JSON.
Saved `jev-1.13.0` and `d1:free` selections are preserved. Liquid's current `d1`
is a billed model; any promotional credits depend on the account.

## Speech recognition and external search

| Service | Default model / operation | Request URL | Official source |
| --- | --- | --- | --- |
| GLM / Z.AI | `glm-asr-2512` | `https://api.z.ai/api/paas/v4/audio/transcriptions` | [Audio transcriptions](https://docs.z.ai/api-reference/audio/audio-transcriptions) |
| GLM China | `glm-asr-2512` | `https://open.bigmodel.cn/api/paas/v4/audio/transcriptions` | [Audio transcriptions](https://docs.bigmodel.cn/api-reference/模型-api/语音转文本) |
| Alibaba international / China | `qwen-audio-3.1-asr-flash` | `https://dashscope-intl.aliyuncs.com/api/v1/services/aigc/multimodal-generation/generation` / `https://dashscope.aliyuncs.com/api/v1/services/aigc/multimodal-generation/generation` | [Current synchronous ASR API](https://www.alibabacloud.com/help/en/model-studio/fun-asr-flash-recorded-speech-recognition-http-api) |
| Alibaba Omni selections | Saved Omni model | Same regional host, `/compatible-mode/v1/chat/completions` | [Qwen Omni API](https://www.alibabacloud.com/help/en/model-studio/qwen-omni) |
| Google grounding | `gemini-3.5-flash-lite` | `https://generativelanguage.googleapis.com/v1beta/interactions` | [Stable model with search grounding](https://ai.google.dev/gemini-api/docs/models/gemini-3.5-flash-lite), [recommended REST example](https://ai.google.dev/gemini-api/docs/interactions-overview) |
| Exa | MCP search | `https://mcp.exa.ai/mcp` | [Exa MCP](https://exa.ai/docs/get-started/exa-mcp) |
| Tavily | Search | `https://api.tavily.com/search` | [Search API](https://docs.tavily.com/documentation/api-reference/endpoint/search) |
| Bing | Public web search | `https://www.bing.com/search` | Public search page; no model API |

Alibaba now recommends workspace-specific domains such as
`{WorkspaceId}.ap-southeast-1.maas.aliyuncs.com` or
`{WorkspaceId}.cn-beijing.maas.aliyuncs.com`. Its documentation explicitly keeps
the shared DashScope domains fully functional. Keep those shared defaults because
the application has no Workspace ID setting; a placeholder domain would fail.
ASR and search retain their documented native protocols rather than LLM Responses.
Qwen Audio 3.1 uses typed `input_audio` and its native transcription response;
saved Qwen3 ASR selections keep their original request and response format.
