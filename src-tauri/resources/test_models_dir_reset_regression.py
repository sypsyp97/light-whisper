import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
FUNASR_COMMANDS = REPO_ROOT / "src-tauri" / "src" / "commands" / "funasr.rs"
SETTINGS_PAGE = REPO_ROOT / "src" / "pages" / "SettingsPage.tsx"


def extract_braced_block(source: str, opening_brace: int) -> str:
    depth = 0
    for index in range(opening_brace, len(source)):
        char = source[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return source[opening_brace : index + 1]
    raise AssertionError("unterminated Rust block")


class ModelDirResetLifecycleRegressionTests(unittest.TestCase):
    def test_engine_config_commits_before_old_runtime_is_stopped(self):
        source = FUNASR_COMMANDS.read_text(encoding="utf-8")
        function_start = source.index("pub async fn set_engine(")
        function_brace = source.index("{", function_start)
        function_block = extract_braced_block(source, function_brace)

        write_position = function_block.index("paths::write_engine_config")
        stop_position = function_block.index("funasr_service::stop_server")
        self.assertLess(
            write_position,
            stop_position,
            "A configuration write failure must leave the old runtime untouched; commit "
            "the new engine config before stopping the working server.",
        )

    def test_restore_default_does_not_return_after_only_writing_config(self):
        source = FUNASR_COMMANDS.read_text(encoding="utf-8")
        function_start = source.index("pub async fn set_models_dir(")
        function_brace = source.index("{", function_start)
        function_block = extract_braced_block(source, function_brace)

        self.assertIn("paths::get_default_models_dir()", function_block)
        self.assertIn("restore_default", function_block)
        selection_end = function_block.index("// canonicalize")
        selection_block = function_block[:selection_end]
        self.assertNotIn(
            "return Ok(",
            selection_block,
            "Restoring the default model directory currently returns immediately after "
            "writing engine.json. The running local ASR process keeps the old "
            "HF_HUB_CACHE until its lifecycle is explicitly reloaded.",
        )

    def test_restore_default_has_an_explicit_runtime_reload_owner(self):
        rust_source = FUNASR_COMMANDS.read_text(encoding="utf-8")
        function_start = rust_source.index("pub async fn set_models_dir(")
        function_brace = rust_source.index("{", function_start)
        function_block = extract_braced_block(rust_source, function_brace)

        settings_source = SETTINGS_PAGE.read_text(encoding="utf-8")
        frontend_reset = settings_source.index("await setModelsDir(null, false);")
        frontend_reset_block = settings_source[frontend_reset : frontend_reset + 800]

        backend_owns_reload = (
            "funasr_service::stop_server" in function_block
            and "funasr_service::start_server" in function_block
        )
        frontend_owns_reload = "restartFunASR(" in frontend_reset_block

        self.assertTrue(
            backend_owns_reload or frontend_owns_reload,
            "Neither set_models_dir nor the restore-default handler explicitly restarts the "
            "local ASR runtime. retryModel() is not sufficient because checkStatus() returns "
            "early for an already-running ready process.",
        )


if __name__ == "__main__":
    unittest.main()
