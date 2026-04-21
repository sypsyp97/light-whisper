# openai_endpoint()

> God node · 16 connections · `C:\Users\sun\Downloads\light-whisper\src-tauri\src\services\llm_client.rs`

## Connections by Relation

### calls
- [[chatgpt_backend_injects_service_tier_priority_when_fast_mode_enabled()]] `EXTRACTED`
- [[injected_service_tier_is_in_the_openai_responses_api_whitelist()]] `EXTRACTED`
- [[chatgpt_backend_omits_service_tier_when_fast_mode_disabled()]] `EXTRACTED`
- [[wrapped_oauth_api_key_gets_service_tier_without_chatgpt_backend_fields()]] `EXTRACTED`
- [[chat_completions_chatgpt_backend_also_gets_service_tier_when_enabled()]] `EXTRACTED`
- [[chatgpt_backend_keeps_max_output_tokens_by_default()]] `EXTRACTED`
- [[chatgpt_backend_responses_json_output_forces_stream_transport()]] `EXTRACTED`
- [[chatgpt_backend_responses_gpt5_reasoning_off_forces_stream_transport()]] `EXTRACTED`
- [[plain_openai_api_key_never_gets_service_tier_even_if_fast_mode_true()]] `EXTRACTED`
- [[chat_body_sets_max_tokens_for_openai_compat()]] `EXTRACTED`
- [[responses_body_sets_max_output_tokens()]] `EXTRACTED`
- [[responses_body_uses_stream_without_forcing_reasoning()]] `EXTRACTED`
- [[chat_body_keeps_provider_default_reasoning()]] `EXTRACTED`
- [[openai_chat_body_maps_reasoning_mode_to_effort()]] `EXTRACTED`
- [[api_error_message_falls_back_to_openai_compat_parser()]] `EXTRACTED`

### contains
- [[llm_client.rs]] `EXTRACTED`

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*