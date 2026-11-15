import os

if os.environ.get("COVERAGE_PROCESS_START"):
    import coverage
    coverage.process_startup()
    coverage.Coverage.current().switch_context(
        os.environ.get("LIGHT_WHISPER_COVERAGE_CONTEXT", "<outside-test>"))
