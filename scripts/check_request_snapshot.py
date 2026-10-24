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
        body = source[source.index("pub async fn transcribe("):]
        body = body[:body.index("\n}")]
        before = body[:body.index(".bearer_auth") if ".bearer_auth" in body else body.index("transcribe_via_omni_chat(")]
        required = ["native_asr_owner.lock().await", "funasr_lifecycle_op.lock().await",
                    f'paths::read_engine_config() != "{engine}"', "let (api_key,"]
        if any(fragment not in before for fragment in required):
            raise ValueError(f"{path}: cloud key/config snapshot and ASR ownership required")
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
