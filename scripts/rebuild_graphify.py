#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
重建 Light-Whisper 的 graphify 知识图谱。

流程：
1. 检测语料边界（遵循仓库根目录的 .graphifyignore）
2. 对代码文件做 graphify AST 提取
3. 注入基于 README / RELEASE_GUIDE 的项目语义层
4. 生成 graph.json、GRAPH_REPORT.md、graph.html、wiki/

必须在项目 .venv 环境中运行。
"""

from __future__ import annotations

import json
import re
from collections import Counter
from pathlib import Path

from graphify.analyze import god_nodes, surprising_connections, suggest_questions
from graphify.build import build_from_json
from graphify.cluster import cluster, score_all
from graphify.detect import detect
from graphify.export import to_html, to_json
from graphify.extract import extract
from graphify.report import generate
from graphify.wiki import to_wiki

PROJECT_ROOT = Path(__file__).resolve().parent.parent
OUT_DIR = PROJECT_ROOT / "graphify-out"

DETECT_PATH = OUT_DIR / ".graphify_detect.json"
AST_PATH = OUT_DIR / ".graphify_ast.json"
SEMANTIC_PATH = OUT_DIR / ".graphify_semantic.json"
EXTRACT_PATH = OUT_DIR / ".graphify_extract.json"
ANALYSIS_PATH = OUT_DIR / ".graphify_analysis.json"
LABELS_PATH = OUT_DIR / ".graphify_labels.json"
GRAPH_JSON_PATH = OUT_DIR / "graph.json"
GRAPH_HTML_PATH = OUT_DIR / "graph.html"
GRAPH_REPORT_PATH = OUT_DIR / "GRAPH_REPORT.md"
WIKI_DIR = OUT_DIR / "wiki"


def make_node(
    node_id: str,
    label: str,
    source_file: str,
    source_location: str | None,
) -> dict:
    return {
        "id": node_id,
        "label": label,
        "file_type": "document",
        "source_file": source_file,
        "source_location": source_location,
        "source_url": None,
        "captured_at": None,
        "author": None,
        "contributor": None,
    }


def make_edge(
    source: str,
    target: str,
    relation: str,
    confidence: str,
    confidence_score: float,
    source_file: str,
    source_location: str | None,
) -> dict:
    return {
        "source": source,
        "target": target,
        "relation": relation,
        "confidence": confidence,
        "confidence_score": confidence_score,
        "source_file": source_file,
        "source_location": source_location,
        "weight": 1.0,
    }


def make_hyperedge(
    hyperedge_id: str,
    label: str,
    nodes: list[str],
    relation: str,
    confidence: str,
    confidence_score: float,
    source_file: str,
) -> dict:
    return {
        "id": hyperedge_id,
        "label": label,
        "nodes": nodes,
        "relation": relation,
        "confidence": confidence,
        "confidence_score": confidence_score,
        "source_file": source_file,
    }


def detect_corpus() -> dict:
    OUT_DIR.mkdir(exist_ok=True)
    result = detect(PROJECT_ROOT)
    DETECT_PATH.write_text(
        json.dumps(result, indent=2, ensure_ascii=False),
        encoding="utf-8",
    )
    return result


def build_ast_extraction(detection: dict) -> dict:
    code_files = [Path(file_path) for file_path in detection["files"]["code"]]
    result = extract(code_files)
    AST_PATH.write_text(
        json.dumps(result, indent=2, ensure_ascii=False),
        encoding="utf-8",
    )
    return result


def build_semantic_layer() -> dict:
    nodes = [
        make_node("doc_light_whisper_overview", "Light-Whisper overview", "README.md", "L1-L170"),
        make_node("doc_light_whisper_overview_zh", "Light-Whisper 中文说明", "README.zh-CN.md", "L1-L170"),
        make_node("doc_release_workflow", "Release workflow", "RELEASE_GUIDE.md", "L1-L80"),
        make_node("feature_one_key_dictation", "One-key dictation", "README.md", "L30-L70"),
        make_node("feature_voice_assistant", "Voice assistant", "README.md", "L40-L70"),
        make_node("feature_selected_text_editing", "Selected text editing", "README.md", "L52-L60"),
        make_node("feature_real_time_translation", "Real-time translation", "README.md", "L55-L60"),
        make_node("feature_ai_polish", "AI polish", "README.md", "L36-L50"),
        make_node("feature_adaptive_learning", "Adaptive learning", "README.md", "L38-L52"),
        make_node("feature_subtitle_overlay", "Subtitle overlay", "README.md", "L42-L60"),
        make_node("feature_web_search_context", "Web search context", "README.md", "L44-L48"),
        make_node("feature_screen_context", "Screen context capture", "README.md", "L44-L48"),
        make_node("feature_local_online_asr", "Hybrid ASR engine layer", "README.md", "L32-L44"),
        make_node("architecture_react_ui", "React UI", "README.md", "L150-L168"),
        make_node("architecture_rust_core", "Rust core", "README.md", "L150-L168"),
        make_node("architecture_python_asr_runtime", "Python ASR runtime", "README.md", "L150-L168"),
        make_node("architecture_llm_integration", "LLM integration layer", "README.md", "L150-L168"),
        make_node("engine_sensevoice", "SenseVoice engine", "README.md", "L74-L92"),
        make_node("engine_faster_whisper", "Faster Whisper engine", "README.md", "L74-L92"),
        make_node("engine_glm_asr", "GLM-ASR engine", "README.md", "L74-L92"),
        make_node("release_engine_packaging", "Python engine packaging", "RELEASE_GUIDE.md", "L7-L40"),
        make_node("release_installer_build", "Installer build", "RELEASE_GUIDE.md", "L7-L40"),
        make_node("release_github_release", "GitHub release publishing", "RELEASE_GUIDE.md", "L7-L65"),
        make_node("feature_codex_oauth_polish", "Codex OAuth polish support", "RELEASE_GUIDE.md", "L15-L24"),
    ]

    edges = [
        make_edge("doc_light_whisper_overview", "feature_one_key_dictation", "references", "EXTRACTED", 1.0, "README.md", "L30-L70"),
        make_edge("doc_light_whisper_overview", "feature_voice_assistant", "references", "EXTRACTED", 1.0, "README.md", "L30-L70"),
        make_edge("doc_light_whisper_overview", "feature_selected_text_editing", "references", "EXTRACTED", 1.0, "README.md", "L52-L60"),
        make_edge("doc_light_whisper_overview", "feature_real_time_translation", "references", "EXTRACTED", 1.0, "README.md", "L55-L60"),
        make_edge("doc_light_whisper_overview", "feature_ai_polish", "references", "EXTRACTED", 1.0, "README.md", "L36-L50"),
        make_edge("doc_light_whisper_overview", "feature_adaptive_learning", "references", "EXTRACTED", 1.0, "README.md", "L38-L52"),
        make_edge("doc_light_whisper_overview", "feature_subtitle_overlay", "references", "EXTRACTED", 1.0, "README.md", "L42-L60"),
        make_edge("doc_light_whisper_overview", "feature_web_search_context", "references", "EXTRACTED", 1.0, "README.md", "L44-L48"),
        make_edge("doc_light_whisper_overview", "feature_screen_context", "references", "EXTRACTED", 1.0, "README.md", "L44-L48"),
        make_edge("doc_light_whisper_overview", "feature_local_online_asr", "references", "EXTRACTED", 1.0, "README.md", "L32-L44"),
        make_edge("doc_light_whisper_overview", "architecture_react_ui", "references", "EXTRACTED", 1.0, "README.md", "L150-L168"),
        make_edge("doc_light_whisper_overview", "architecture_rust_core", "references", "EXTRACTED", 1.0, "README.md", "L150-L168"),
        make_edge("doc_light_whisper_overview", "architecture_python_asr_runtime", "references", "EXTRACTED", 1.0, "README.md", "L150-L168"),
        make_edge("doc_light_whisper_overview", "architecture_llm_integration", "references", "EXTRACTED", 1.0, "README.md", "L150-L168"),
        make_edge("doc_light_whisper_overview", "engine_sensevoice", "references", "EXTRACTED", 1.0, "README.md", "L74-L92"),
        make_edge("doc_light_whisper_overview", "engine_faster_whisper", "references", "EXTRACTED", 1.0, "README.md", "L74-L92"),
        make_edge("doc_light_whisper_overview", "engine_glm_asr", "references", "EXTRACTED", 1.0, "README.md", "L74-L92"),
        make_edge("doc_light_whisper_overview_zh", "doc_light_whisper_overview", "conceptually_related_to", "EXTRACTED", 1.0, "README.zh-CN.md", "L1-L20"),
        make_edge("doc_release_workflow", "release_engine_packaging", "references", "EXTRACTED", 1.0, "RELEASE_GUIDE.md", "L7-L40"),
        make_edge("doc_release_workflow", "release_installer_build", "references", "EXTRACTED", 1.0, "RELEASE_GUIDE.md", "L7-L40"),
        make_edge("doc_release_workflow", "release_github_release", "references", "EXTRACTED", 1.0, "RELEASE_GUIDE.md", "L7-L65"),
        make_edge("doc_release_workflow", "feature_codex_oauth_polish", "references", "EXTRACTED", 1.0, "RELEASE_GUIDE.md", "L15-L24"),
        make_edge("feature_voice_assistant", "feature_web_search_context", "shares_data_with", "INFERRED", 0.93, "README.md", "L44-L48"),
        make_edge("feature_voice_assistant", "feature_screen_context", "shares_data_with", "INFERRED", 0.93, "README.md", "L44-L48"),
        make_edge("feature_selected_text_editing", "feature_ai_polish", "conceptually_related_to", "INFERRED", 0.91, "README.md", "L36-L60"),
        make_edge("feature_one_key_dictation", "feature_local_online_asr", "shares_data_with", "INFERRED", 0.92, "README.md", "L30-L44"),
        make_edge("feature_ai_polish", "architecture_llm_integration", "conceptually_related_to", "INFERRED", 0.90, "README.md", "L36-L50"),
        make_edge("feature_voice_assistant", "architecture_llm_integration", "conceptually_related_to", "INFERRED", 0.90, "README.md", "L40-L50"),
        make_edge("feature_local_online_asr", "engine_sensevoice", "conceptually_related_to", "EXTRACTED", 1.0, "README.md", "L74-L92"),
        make_edge("feature_local_online_asr", "engine_faster_whisper", "conceptually_related_to", "EXTRACTED", 1.0, "README.md", "L74-L92"),
        make_edge("feature_local_online_asr", "engine_glm_asr", "conceptually_related_to", "EXTRACTED", 1.0, "README.md", "L74-L92"),
        make_edge("audio_service_do_final_asr", "feature_one_key_dictation", "implements", "INFERRED", 0.96, "src-tauri\\src\\services\\audio_service.rs", None),
        make_edge("hotkey_register_custom_hotkey", "feature_one_key_dictation", "implements", "INFERRED", 0.91, "src-tauri\\src\\commands\\hotkey.rs", None),
        make_edge("assistant_service_generate_content", "feature_voice_assistant", "implements", "INFERRED", 0.97, "src-tauri\\src\\services\\assistant_service.rs", None),
        make_edge("hotkey_register_assistant_hotkey", "feature_voice_assistant", "implements", "INFERRED", 0.90, "src-tauri\\src\\commands\\hotkey.rs", None),
        make_edge("ai_polish_service_edit_text", "feature_selected_text_editing", "implements", "INFERRED", 0.96, "src-tauri\\src\\services\\ai_polish_service.rs", None),
        make_edge("hotkey_register_translation_hotkey", "feature_real_time_translation", "implements", "INFERRED", 0.90, "src-tauri\\src\\commands\\hotkey.rs", None),
        make_edge("profile_set_translation_target", "feature_real_time_translation", "implements", "INFERRED", 0.88, "src-tauri\\src\\commands\\profile.rs", None),
        make_edge("ai_polish_service_polish_text", "feature_ai_polish", "implements", "INFERRED", 0.97, "src-tauri\\src\\services\\ai_polish_service.rs", None),
        make_edge("profile_service_promote_vocab_to_hot_words", "feature_adaptive_learning", "implements", "INFERRED", 0.94, "src-tauri\\src\\services\\profile_service.rs", None),
        make_edge("profile_service_learn_from_correction", "feature_adaptive_learning", "implements", "INFERRED", 0.94, "src-tauri\\src\\services\\profile_service.rs", None),
        make_edge("window_create_subtitle_window", "feature_subtitle_overlay", "implements", "INFERRED", 0.95, "src-tauri\\src\\commands\\window.rs", None),
        make_edge("subtitleoverlay_subtitleoverlay", "feature_subtitle_overlay", "implements", "INFERRED", 0.94, "src\\pages\\SubtitleOverlay.tsx", None),
        make_edge("assistant_service_run_third_party_search", "feature_web_search_context", "implements", "INFERRED", 0.96, "src-tauri\\src\\services\\assistant_service.rs", None),
        make_edge("web_search_service_render_search_context", "feature_web_search_context", "implements", "INFERRED", 0.95, "src-tauri\\src\\services\\web_search_service.rs", None),
        make_edge("screen_capture_service_capture_full_screen_context", "feature_screen_context", "implements", "INFERRED", 0.96, "src-tauri\\src\\services\\screen_capture_service.rs", None),
        make_edge("foreground_format_prompt_context", "feature_screen_context", "implements", "INFERRED", 0.85, "src-tauri\\src\\utils\\foreground.rs", None),
        make_edge("funasr_server_funasrserver", "engine_sensevoice", "implements", "INFERRED", 0.98, "src-tauri\\resources\\funasr_server.py", None),
        make_edge("whisper_server_whisperserver", "engine_faster_whisper", "implements", "INFERRED", 0.98, "src-tauri\\resources\\whisper_server.py", None),
        make_edge("src_tauri_src_services_glm_asr_service_rs", "engine_glm_asr", "implements", "INFERRED", 0.97, "src-tauri\\src\\services\\glm_asr_service.rs", None),
        make_edge("src_pages_mainpage_tsx", "architecture_react_ui", "implements", "INFERRED", 0.92, "src\\pages\\MainPage.tsx", None),
        make_edge("src_pages_settingspage_tsx", "architecture_react_ui", "implements", "INFERRED", 0.92, "src\\pages\\SettingsPage.tsx", None),
        make_edge("main_main", "architecture_rust_core", "implements", "INFERRED", 0.90, "src-tauri\\src\\main.rs", None),
        make_edge("server_common_baseasrserver", "architecture_python_asr_runtime", "implements", "INFERRED", 0.97, "src-tauri\\resources\\server_common.py", None),
        make_edge("llm_client_send_llm_request", "architecture_llm_integration", "implements", "INFERRED", 0.96, "src-tauri\\src\\services\\llm_client.rs", None),
        make_edge("llm_provider_endpoint_for_config", "architecture_llm_integration", "implements", "INFERRED", 0.95, "src-tauri\\src\\services\\llm_provider.rs", None),
        make_edge("build_engine_create_tar_xz_with_python", "release_engine_packaging", "implements", "INFERRED", 0.97, "scripts\\build_engine.py", None),
        make_edge("build_engine_main", "release_engine_packaging", "implements", "INFERRED", 0.95, "scripts\\build_engine.py", None),
        make_edge("build_main", "release_installer_build", "implements", "INFERRED", 0.78, "src-tauri\\build.rs", None),
        make_edge("codex_oauth_service_login", "feature_codex_oauth_polish", "implements", "INFERRED", 0.97, "src-tauri\\src\\services\\codex_oauth_service.rs", None),
        make_edge("ai_polish_service_polish_text", "feature_codex_oauth_polish", "conceptually_related_to", "INFERRED", 0.82, "RELEASE_GUIDE.md", None),
    ]

    hyperedges = [
        make_hyperedge(
            "application_stack",
            "Application stack",
            [
                "architecture_react_ui",
                "architecture_rust_core",
                "architecture_python_asr_runtime",
                "architecture_llm_integration",
            ],
            "form",
            "EXTRACTED",
            1.0,
            "README.md",
        ),
        make_hyperedge(
            "speech_engines",
            "Speech engines",
            ["engine_sensevoice", "engine_faster_whisper", "engine_glm_asr"],
            "form",
            "EXTRACTED",
            1.0,
            "README.md",
        ),
        make_hyperedge(
            "assistant_context_stack",
            "Assistant context stack",
            [
                "feature_voice_assistant",
                "feature_web_search_context",
                "feature_screen_context",
                "architecture_llm_integration",
            ],
            "participate_in",
            "INFERRED",
            0.90,
            "README.md",
        ),
        make_hyperedge(
            "release_pipeline",
            "Release pipeline",
            [
                "release_engine_packaging",
                "release_installer_build",
                "release_github_release",
            ],
            "form",
            "EXTRACTED",
            1.0,
            "RELEASE_GUIDE.md",
        ),
    ]

    semantic = {
        "nodes": nodes,
        "edges": edges,
        "hyperedges": hyperedges,
        "input_tokens": 0,
        "output_tokens": 0,
    }
    SEMANTIC_PATH.write_text(
        json.dumps(semantic, indent=2, ensure_ascii=False),
        encoding="utf-8",
    )
    return semantic


def merge_extractions(ast_result: dict, semantic_result: dict) -> dict:
    seen = {node["id"] for node in ast_result["nodes"]}
    merged_nodes = list(ast_result["nodes"])
    for node in semantic_result["nodes"]:
        if node["id"] not in seen:
            merged_nodes.append(node)
            seen.add(node["id"])

    merged = {
        "nodes": merged_nodes,
        "edges": list(ast_result["edges"]) + list(semantic_result["edges"]),
        "hyperedges": list(semantic_result.get("hyperedges", [])),
        "input_tokens": semantic_result.get("input_tokens", 0),
        "output_tokens": semantic_result.get("output_tokens", 0),
    }
    EXTRACT_PATH.write_text(
        json.dumps(merged, indent=2, ensure_ascii=False),
        encoding="utf-8",
    )
    return merged


def titleize(text: str) -> str:
    text = text.replace("-", " ").replace("_", " ")
    text = re.sub(r"(?<!^)(?=[A-Z])", " ", text)
    text = re.sub(r"\s+", " ", text).strip()
    acronyms = {
        "llm": "LLM",
        "asr": "ASR",
        "hf": "HF",
        "ui": "UI",
        "ipc": "IPC",
        "oauth": "OAuth",
    }
    words = []
    for word in text.split():
        words.append(acronyms.get(word.lower(), word.capitalize()))
    return " ".join(words)


def build_labels(communities: dict[int, list[str]], extraction: dict) -> dict[int, str]:
    node_map = {node["id"]: node for node in extraction["nodes"]}

    source_overrides = {
        "src-tauri/src/commands/hotkey.rs": "Hotkey Control",
        "src-tauri/resources/server_common.py": "Python ASR Runtime",
        "src-tauri/src/services/llm_provider.rs": "LLM Provider Routing",
        "src/api/tauri.ts": "Frontend IPC Bridge",
        "src-tauri/src/services/funasr_service.rs": "Speech Engine Service",
        "src-tauri/src/state/app_state.rs": "Shared App State",
        "src-tauri/src/services/codex_oauth_service.rs": "Codex OAuth Flow",
        "src-tauri/src/services/llm_client.rs": "LLM Client",
        "src-tauri/src/services/web_search_service.rs": "Assistant Search Layer",
        "src-tauri/src/services/profile_service.rs": "Adaptive Learning Profile",
        "src-tauri/src/services/audio_service.rs": "Recording Pipeline",
        "src-tauri/src/state/user_profile.rs": "User Profile Schema",
        "scripts/build_engine.py": "Release Packaging",
        "src-tauri/src/services/ai_polish_service.rs": "AI Polish Service",
        "src-tauri/src/utils/paths.rs": "Runtime Paths",
        "src-tauri/src/commands/profile.rs": "Profile Commands",
        "src/pages/SettingsPage.tsx": "React Settings UI",
        "src-tauri/src/commands/funasr.rs": "Model Management Commands",
        "src-tauri/src/utils/foreground.rs": "Foreground Context",
        "src-tauri/src/commands/window.rs": "Subtitle Window System",
        "src-tauri/src/commands/audio.rs": "Audio Commands",
        "src-tauri/src/lib.rs": "App Bootstrap",
        "src-tauri/src/commands/ai_polish.rs": "AI Polish Commands",
        "src-tauri/src/commands/clipboard.rs": "Clipboard Bridge",
        "src-tauri/src/commands/updater.rs": "Updater Commands",
        "src-tauri/src/services/download_service.rs": "Model Download Service",
        "src-tauri/resources/download_models.py": "Model Download Script",
        "src-tauri/src/commands/assistant.rs": "Assistant Commands",
        "src-tauri/src/utils/sound.rs": "Sound Effects",
        "src-tauri/resources/hf_cache_utils.py": "HF Cache Utilities",
        "src/components/StatusIndicator.tsx": "Status Indicator UI",
        "src/hooks/useModelStatus.ts": "Model Status Hook",
        "src/hooks/useTheme.ts": "Theme Hook",
        "src/main.tsx": "Frontend Entry",
        "src/hooks/useHotkey.ts": "Hotkey Storage Hook",
        "src/lib/hotkey.ts": "Hotkey Normalization",
        "src-tauri/src/commands/codex_oauth.rs": "Codex OAuth Commands",
        "src-tauri/src/utils/error.rs": "App Errors",
        "promo/src/components/AppWindow.tsx": "Promo App Window",
        "src/contexts/RecordingContext.tsx": "Recording Context",
        "src/hooks/useRecording.ts": "Recording Hook",
        "src/lib/storage.ts": "Local Storage Helpers",
        "promo/src/Root.tsx": "Promo Root",
        "promo/src/components/Cursor.tsx": "Promo Cursor",
        "src/components/TitleBar.tsx": "Title Bar",
        "src/hooks/useDebouncedCallback.ts": "Debounced Callback Hook",
        "src/hooks/useExclusivePicker.ts": "Exclusive Picker Hook",
        "src/hooks/useHotkeyCapture.ts": "Hotkey Capture Hook",
        "vite.config.ts": "Vite Config",
        "promo/remotion.config.ts": "Remotion Config",
        "promo/src/index.ts": "Promo Src Index",
        "src/i18n/index.ts": "I18n Index",
        "promo/src/ShowcaseVideo.tsx": "Promo Showcase Video",
        "promo/src/theme.ts": "Promo Theme",
        "promo/src/components/Cinematic.tsx": "Promo Cinematic Scene",
        "promo/src/components/MicButton.tsx": "Promo Mic Button",
        "promo/src/components/ResultCard.tsx": "Promo Result Card",
        "promo/src/components/SubtitleCapsule.tsx": "Promo Subtitle Capsule",
        "promo/src/scenes/Assistant.tsx": "Promo Assistant Scene",
        "promo/src/scenes/Dictation.tsx": "Promo Dictation Scene",
        "promo/src/scenes/EditMode.tsx": "Promo Edit Scene",
        "promo/src/scenes/Intro.tsx": "Promo Intro Scene",
        "promo/src/scenes/Outro.tsx": "Promo Outro Scene",
        "promo/src/scenes/Translation.tsx": "Promo Translation Scene",
        "src/vite-env.d.ts": "Vite Env Types",
        "src/components/RecordingButton.tsx": "Recording Button UI",
        "src/components/TranscriptionHistory.tsx": "Transcription History UI",
        "src/components/TranscriptionResult.tsx": "Transcription Result UI",
        "src/i18n/en.ts": "English I18n",
        "src/i18n/zh.ts": "Chinese I18n",
        "src/lib/constants.ts": "App Constants",
        "src/types/index.ts": "Types Index",
        "src-tauri/src/commands/mod.rs": "Commands Module",
        "src-tauri/src/services/mod.rs": "Services Module",
        "src-tauri/src/state/mod.rs": "State Module",
        "src-tauri/src/utils/mod.rs": "Utils Module",
    }

    special_stems = {
        "storage": "Local Storage Helpers",
        "Root": "Promo Root",
        "Cursor": "Promo Cursor",
        "TitleBar": "Title Bar",
        "useDebouncedCallback": "Debounced Callback Hook",
        "useExclusivePicker": "Exclusive Picker Hook",
        "useHotkeyCapture": "Hotkey Capture Hook",
        "vite.config": "Vite Config",
        "remotion.config": "Remotion Config",
        "ShowcaseVideo": "Promo Showcase Video",
        "theme": "Promo Theme",
        "Cinematic": "Promo Cinematic Scene",
        "MicButton": "Promo Mic Button",
        "ResultCard": "Promo Result Card",
        "SubtitleCapsule": "Promo Subtitle Capsule",
        "Assistant": "Promo Assistant Scene",
        "Dictation": "Promo Dictation Scene",
        "EditMode": "Promo Edit Scene",
        "Intro": "Promo Intro Scene",
        "Outro": "Promo Outro Scene",
        "Translation": "Promo Translation Scene",
        "vite-env.d": "Vite Env Types",
        "RecordingButton": "Recording Button UI",
        "TranscriptionHistory": "Transcription History UI",
        "TranscriptionResult": "Transcription Result UI",
        "en": "English I18n",
        "zh": "Chinese I18n",
        "constants": "App Constants",
    }

    def derive_label(community_id: int, nodes: list[str]) -> str:
        source_counts: Counter[str] = Counter()
        for node_id in nodes:
            source_file = node_map.get(node_id, {}).get("source_file")
            if source_file:
                source_counts[source_file] += 1

        if source_counts:
            top_source = source_counts.most_common(1)[0][0]
            normalized = top_source.replace("\\", "/")
            if normalized in source_overrides:
                return source_overrides[normalized]
            path = Path(top_source)
            stem = path.stem
            parent = path.parent.name
            if stem in special_stems:
                return special_stems[stem]
            if stem == "index":
                return f"{titleize(parent)} Index"
            if stem == "mod":
                return f"{titleize(parent)} Module"
            if stem == "main":
                return f"{titleize(parent)} Main"
            return titleize(stem)

        for node_id in nodes:
            label = node_map.get(node_id, {}).get("label", "").strip()
            label = re.sub(r"\(\)$", "", label)
            if label:
                return titleize(label)

        return f"Community {community_id}"

    labels: dict[int, str] = {}
    used: set[str] = set()
    for community_id, nodes in sorted(communities.items()):
        label = derive_label(community_id, nodes)
        if label in used:
            label = f"{label} {community_id}"
        used.add(label)
        labels[community_id] = label
    return labels


def build_outputs(extraction: dict, detection: dict) -> dict:
    graph = build_from_json(extraction)
    communities = cluster(graph)
    cohesion = score_all(graph, communities)
    gods = god_nodes(graph)
    surprises = surprising_connections(graph, communities)
    labels = build_labels(communities, extraction)
    questions = suggest_questions(graph, communities, labels)
    tokens = {
        "input": extraction.get("input_tokens", 0),
        "output": extraction.get("output_tokens", 0),
    }

    report = generate(
        graph,
        communities,
        cohesion,
        labels,
        gods,
        surprises,
        detection,
        tokens,
        ".",
        suggested_questions=questions,
    )
    GRAPH_REPORT_PATH.write_text(report, encoding="utf-8")
    LABELS_PATH.write_text(
        json.dumps({str(key): value for key, value in labels.items()}, indent=2, ensure_ascii=False),
        encoding="utf-8",
    )
    to_json(graph, communities, str(GRAPH_JSON_PATH))
    to_html(graph, communities, str(GRAPH_HTML_PATH), community_labels=labels)
    article_count = to_wiki(
        graph,
        communities,
        str(WIKI_DIR),
        community_labels=labels,
        cohesion=cohesion,
        god_nodes_data=gods,
    )

    analysis = {
        "communities": {str(key): value for key, value in communities.items()},
        "cohesion": {str(key): value for key, value in cohesion.items()},
        "gods": gods,
        "surprises": surprises,
        "questions": questions,
    }
    ANALYSIS_PATH.write_text(
        json.dumps(analysis, indent=2, ensure_ascii=False),
        encoding="utf-8",
    )

    return {
        "graph": graph,
        "communities": communities,
        "article_count": article_count,
    }


def main() -> None:
    detection = detect_corpus()
    if detection.get("total_files", 0) == 0:
        raise SystemExit("No supported files found after applying .graphifyignore.")

    ast_result = build_ast_extraction(detection)
    semantic_result = build_semantic_layer()
    extraction = merge_extractions(ast_result, semantic_result)
    result = build_outputs(extraction, detection)

    print(
        "Graph rebuilt:",
        f"{result['graph'].number_of_nodes()} nodes,",
        f"{result['graph'].number_of_edges()} edges,",
        f"{len(result['communities'])} communities",
    )
    print(f"Report: {GRAPH_REPORT_PATH}")
    print(f"HTML:   {GRAPH_HTML_PATH}")
    print(f"Wiki:   {WIKI_DIR} ({result['article_count']} articles)")


if __name__ == "__main__":
    main()
