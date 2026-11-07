"""Source conformance guard for provider/key snapshots; not a Rust execution proof."""

import argparse
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PATHS = {
    "src-tauri/src/services/ai_polish_service.rs": ["polish_text_with_overrides_detailed", "edit_text"],
    "src-tauri/src/services/assistant_service.rs": ["generate_content_inner"],
    "src-tauri/src/commands/profile.rs": ["extract_corrections_via_llm", "run_correction_validation"],
    "src-tauri/src/commands/selection.rs": ["run_llm_action"],
    "src-tauri/src/services/screen_vision_service.rs": ["describe_images"],
}


def check_cloud_asr_snapshot(source: str, engine: str) -> None:
    def function(signature: str) -> str:
        start = source.find(signature)
        if start < 0:
            raise ValueError(f"{engine}: missing {signature}")
        return " ".join(source[start:source.index("\n}", start)].split())

    glm = engine == "glm-asr"
    fields = "api_key, url" if glm else "api_key, base, model, region"
    endpoints = ('format!("{}{}", paths::read_online_asr_endpoint(), GLM_ASR_PATH)' if glm else
                 'paths::read_alibaba_endpoint(), paths::read_alibaba_model(), paths::read_alibaba_region()')
    entry = function("pub async fn transcribe(")
    before, _, rest = entry.partition(f"let ({fields}) = {{")
    snapshot, end, request = rest.partition("};")
    lock = "let _configuration = state.engine.funasr_lifecycle_op.lock().await;"
    if ("let _asr_guard = state.engine.native_asr_owner.lock().await;" not in before or not end
            or "drop(_asr_guard)" in entry or "drop(_configuration)" in snapshot
            or not snapshot.strip().startswith(f'{lock} if paths::read_engine_config() != "{engine}" {{ return Err(')
            or not snapshot.strip().endswith(f"( state.read_online_asr_api_key(), {endpoints}, )")):
        raise ValueError(f"{engine}: ASR ownership, engine check and locked credential snapshot required")

    def reject_live_reads(body: str) -> None:
        if "read_online_asr_" in body or "read_alibaba_" in body:
            raise ValueError(f"{engine}: live credential or endpoint read after snapshot")

    reject_live_reads(request)
    if glm and "fn transcribe_request(" not in source:
        # Earlier revisions sent directly from transcribe with owned strings.
        if ".post(&url) .bearer_auth(&api_key)" not in request:
            raise ValueError(f"{engine}: inline request must use the captured URL and API key")
        return

    transports = ["transcribe_request"] if glm else ["transcribe_via_omni_chat", "transcribe_via_dashscope_asr"]
    for name in transports:
        args = "state, audio_data, &url, &api_key" if glm else "state, &base, &model, &api_key, audio_wav"
        if f"{name}({args}).await" not in request:
            raise ValueError(f"{engine}: {name} must receive the captured endpoint and API key")
        transport = function(f"fn {name}(")
        reject_live_reads(transport)
        if glm:
            binding = "audio_data: Vec<u8>, url: &str, api_key: &str,"
            post = ".post(url)"
        else:
            binding = "base: &str, model: &str, api_key: &str, audio_wav: Vec<u8>,"
            suffix = "OMNI_CHAT_PATH" if name == "transcribe_via_omni_chat" else "ASR_PATH"
            if f'let url = format!("{{}}{{}}", base, {suffix});' not in transport:
                raise ValueError(f"{engine}: {name} must build the URL from the captured base")
            post = ".post(&url)"
        if binding not in transport or f"{post} .bearer_auth(api_key)" not in transport:
            raise ValueError(f"{engine}: {name} must POST using the captured endpoint and API key")


def check(revision: str | None = None) -> None:
    for path, functions in PATHS.items():
        source = subprocess.check_output(["git", "show", f"{revision}:{path}"], cwd=ROOT,
                                         text=True, encoding="utf-8") if revision else (ROOT / path).read_text(encoding="utf-8")
        for name in functions:
            start = source.index(f"fn {name}(")
            end = source.find("\n}", start)
            body = source[start:end]
            if path == "src-tauri/src/services/ai_polish_service.rs":
                if f"pub async fn {name}(" not in source:
                    raise ValueError(f"{path}:{name}: public entry point required")
                inner = {"polish_text_with_overrides_detailed": "polish_text_inner",
                         "edit_text": "edit_text_inner"}[name]
                if f"fn {inner}(" in source:
                    delegation = (rf"run_polish_operation\(\s*state,\s*session_id,\s*"
                                  rf"std::time::Duration::from_secs\(AI_POLISH_STREAM_TOTAL_TIMEOUT_SECS\),\s*"
                                  rf"{inner}\(")
                    if not re.search(delegation, body):
                        raise ValueError(f"{path}:{name}: shared operation budget must wrap {inner}")
                    start = source.index(f"fn {inner}(")
                    end = source.find("\n}", start)
                    body = source[start:end]
            resolve = body.index("codex_oauth_service::resolve_api_key_for_provider")
            before, after = body[:resolve], body[resolve:]
            required = ["let config = state.llm_provider_config();", "let endpoint =", "_for_config(&config)",
                        "load_api_key_for_provider(app_handle,"]
            if any(fragment not in before for fragment in required):
                raise ValueError(f"{path}:{name}: endpoint/key must be snapshotted before auth")
            if "state.llm_provider_config()" in after or "read_ai_polish_api_key()" in body or "read_assistant_api_key()" in body:
                raise ValueError(f"{path}:{name}: live configuration or unscoped key cache read")
            if not all(mode in after for mode in ["config.openai_auth_mode", "config.xai_auth_mode"]):
                raise ValueError(f"{path}:{name}: auth mode must use the same snapshot")
    for path, engine in [("src-tauri/src/services/alibaba_asr_service.rs", "alibaba-asr"),
                         ("src-tauri/src/services/glm_asr_service.rs", "glm-asr")]:
        source = subprocess.check_output(["git", "show", f"{revision}:{path}"], cwd=ROOT,
                                         text=True, encoding="utf-8") if revision else (ROOT / path).read_text(encoding="utf-8")
        check_cloud_asr_snapshot(source, engine)
    path = "src-tauri/src/commands/funasr.rs"
    source = subprocess.check_output(["git", "show", f"{revision}:{path}"], cwd=ROOT,
                                     text=True, encoding="utf-8") if revision else (ROOT / path).read_text(encoding="utf-8")
    body = source[source.index("pub async fn list_alibaba_asr_models("):]
    body = body[:body.index("\n}")]
    if "state.read_online_asr_api_key()" in body or not all(part in body for part in [
            "funasr_lifecycle_op.lock().await", "ALIBABA_ASR_CN_KEYRING_USER", "ALIBABA_ASR_INTL_KEYRING_USER"]):
        raise ValueError("Alibaba model discovery must read its own region's key slot")
    body = source[source.index("pub async fn set_online_asr_api_key("):]
    body = body[:body.index("\n}")]
    if body.index("funasr_lifecycle_op.lock().await") > body.index("save_or_delete_api_key"):
        raise ValueError("cloud key save must serialize storage and cache publication")
    for path in ["src-tauri/src/services/codex_oauth_service.rs", "src-tauri/src/services/grok_build_oauth_service.rs"]:
        source = subprocess.check_output(["git", "show", f"{revision}:{path}"], cwd=ROOT,
                                         text=True, encoding="utf-8") if revision else (ROOT / path).read_text(encoding="utf-8")
        body = source[source.index("fn save_session_to_storage("):]
        body = body[:body.index("\n}")]
        if body.index("write_session_meta(") < body.index("delete_keyring_password("):
            raise ValueError(f"{path}: valid metadata must commit after all credential mutations")
    def read(path: str) -> str:
        return subprocess.check_output(["git", "show", f"{revision}:{path}"], cwd=ROOT,
                                       text=True, encoding="utf-8") if revision else (ROOT / path).read_text(encoding="utf-8")

    catalog = read("src-tauri/src/commands/ai_polish.rs")
    catalog = catalog[catalog.index("pub async fn list_ai_models("):]
    catalog = catalog[:catalog.index("\n}")]
    resolve = catalog.index("codex_oauth_service::resolve_provider_auth_with_auth_mode")
    if "let config = state.llm_provider_config();" not in catalog[:resolve] or any(
        live in catalog[resolve:] for live in ["state.llm_provider_config()", "current_chatgpt_bearer_token"]
    ) or "auth.chatgpt_token" not in catalog:
        raise ValueError("model catalog must use captured configuration and paired resolved auth")
    resolver = read("src-tauri/src/services/codex_oauth_service.rs")
    resolver = resolver[resolver.index("pub async fn resolve_provider_auth_with_auth_mode("):]
    resolver = resolver[:resolver.index("\n}")]
    if "state.llm_provider_config()" in resolver or "auth_from_openai_session(session)" not in resolver:
        raise ValueError("auth resolution must preserve captured modes and pair one session's credentials")

    assistant = read("src-tauri/src/commands/assistant.rs")
    for name in ["set_web_search_api_key", "get_web_search_api_key"]:
        body = assistant[assistant.index(f"pub async fn {name}("):]
        body = body[:body.index("\n}")]
        gate = body.index("state.with_web_search_key_operation(||")
        storage = body.index("save_or_delete_api_key" if name.startswith("set_") else "let cached =")
        if gate > storage:
            raise ValueError(f"{name}: key storage/read and cache publication require common ownership")
    startup = read("src-tauri/src/lib.rs")
    startup = startup[startup.index("for search_provider in ["):]
    if startup.index("state.with_web_search_key_operation(||") > startup.index(".get_password("):
        raise ValueError("startup web key restore must use the same publication owner")
    print("PASS provider/key snapshot conformance: eight LLM/catalog and three cloud ASR paths; web key publication ownership")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--revision")
    check(parser.parse_args().revision)
