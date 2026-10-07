# Privacy policy

This describes the Windows desktop application, not the privacy practices of
services you choose to connect. Last reviewed: October 7, 2026.

## Data processed on your computer

Local speech recognition processes microphone audio on your computer. Local
model weights are downloaded separately. Selecting a local engine does not
make optional cloud processing local: AI polish, translation, assistants,
automatic decisions and web search can still contact their configured services.

Settings, hotwords, correction rules and application rules are stored locally.
History and audio saving are off by default. When enabled, history is stored in
a local database; audio saving requires history to be enabled. Retention is
configurable, with a default of 90 days and an option to keep history indefinitely.
You can delete history and saved audio through the application.

Provider API keys and supported account credentials use the Windows credential
store. Account metadata and application settings are also stored locally.
Text is inserted into the active application through simulated input or the
clipboard, depending on your setting; the receiving application and operating
system clipboard features have their own data handling.

Application and recognition logs are local files. They may contain operational
metadata, application names, file paths and error details. They are not
automatically uploaded to the project maintainer. Review logs and exports before
sharing them. Data and log locations are documented in
[Development](development.md#troubleshooting).

## Network requests

| Feature | Data sent and destination |
|:--|:--|
| Local model download | Model identifiers and ordinary download request metadata go to Hugging Face. Unless `HF_ENDPOINT` is set, a failed download can retry through `hf-mirror.com`. Microphone recordings are not part of these downloads. |
| Cloud recognition | Recorded audio and recognition settings go to the recognition provider and region you select, currently GLM-ASR or Alibaba DashScope. |
| AI polish, translation and assistants | Text, instructions, conversation context and enabled application context go to the configured provider or custom endpoint. Enabled selection or screenshot context may include selected text and screen images. |
| Automatic decisions and optional checks | Text and processing requirements go to the configured decision provider. Selecting Auto can make these requests even when the decision ultimately skips further processing. |
| Web search | Queries and related request context go to the selected search service: Bing, Exa, Tavily, Google or a model provider's native search. ChatGPT/Codex search uses Exa. |
| Account login and model discovery | The selected provider receives login/authorization requests, account credentials or tokens needed for that service, and model-catalog requests. Browser login is governed by the provider's policies. |
| Check for updates | Clicking Check for Updates requests release metadata from GitHub, including an application/version user-agent and normal network metadata. |

Network services can receive your IP address and request metadata and apply
their own retention and processing rules. Requests are made directly to the
configured services, not through a Light-Whisper maintainer server. Custom
providers and download endpoints can change the destination.

To keep speech content on your computer, use a local engine and disable cloud
polish, translation, assistants, automatic decisions, context sharing and search.
Previously downloaded local models can be used without downloading them again.
Disabling a feature does not erase information already sent to a provider; use
that provider's account controls for deletion requests.

## Third-party privacy policies

Use the policy for your selected service, account and region:

- Downloads and releases: [Hugging Face](https://huggingface.co/privacy),
  [GitHub](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement).
  `hf-mirror.com` is a separate mirror; set `HF_ENDPOINT=https://huggingface.co`
  to prevent the application's automatic mirror fallback.
- AI providers: [OpenAI](https://openai.com/policies/privacy-policy/),
  [xAI](https://x.ai/legal/privacy-policy),
  [DeepSeek](https://cdn.deepseek.com/policies/en-US/deepseek-privacy-policy.html),
  [Cerebras](https://www.cerebras.ai/privacy-policy),
  [SiliconFlow](https://docs.siliconflow.cn/docs/legals/privacy-policy).
- Decision services: [TypeSafe](https://typesafe.ai/legal/privacy-policy),
  [OpenRouter](https://openrouter.ai/privacy),
  [Vercel](https://vercel.com/legal/privacy-notice),
  [Liquid AI](https://www.liquid.ai/privacy-policy).
- Search: [Microsoft/Bing](https://www.microsoft.com/en-us/privacy/privacystatement),
  [Exa](https://exa.ai/privacy-policy), [Tavily](https://www.tavily.com/privacy),
  [Google](https://policies.google.com/privacy).
- Cloud recognition: [Z.ai's privacy policy and API data processing addendum](https://docs.z.ai/legal-agreement/privacy-policy)
  and [Alibaba Cloud's privacy policy](https://www.alibabacloud.com/help/en/legal/latest/privacy-policy).
  Use the terms for the region you select; domestic BigModel and Alibaba Cloud
  services may have different terms from their international services.
- Custom providers: consult the operator of the endpoint you configure; its
  policy can differ from that of the model's original developer.

## Questions and voluntary reports

The project maintainer is [sypsyp97](https://github.com/sypsyp97). You can contact
the maintainer through the public repository. GitHub issues and attachments are
public: do not include credentials, private recordings or personal transcripts.
Information you voluntarily share there is subject to GitHub's policies.
