"""Offline CPU/CUDA build, actual packager CLI, pruning and archive contracts."""

import importlib.metadata

import contextlib
import fnmatch
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import sys
import tarfile
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

import build_r2t2_runtime as builder
from test_build_engine_atomicity import load_build_engine_module


class WindowsBuildEnvironmentTests(unittest.TestCase):
    def test_vcvars_overrides_inherited_environment_case_insensitively(self):
        inherited = {"PATH": r"C:\caller\bin", "INCLUDE": r"C:\old\include",
                     "VCTOOLSREDISTDIR": r"C:\old\redist", "CALLER_ONLY": "retained=value",
                     "EMPTY_VALUE": ""}
        inherited_before = inherited.copy()
        global_before = dict(os.environ)
        tool_path = r"C:\Visual Studio\VC\bin;C:\caller\bin"
        vcvars_values = {"Path": tool_path, "Include": r"C:\Visual Studio\VC\include",
                         "VCToolsRedistDir": r"C:\Visual Studio\VC\redist"}
        with tempfile.TemporaryDirectory() as directory:
            installation = Path(directory)
            vcvars = installation / "VC/Auxiliary/Build/vcvars64.bat"
            vcvars.parent.mkdir(parents=True)
            vcvars.write_text("@echo off\n", encoding="utf-8")
            responses = [
                SimpleNamespace(stdout=str(installation) + "\n"),
                SimpleNamespace(stdout="\n".join(f"{key}={value}" for key, value in vcvars_values.items())),
            ]
            # Simulate Windows without changing the host's os module or environment.
            with mock.patch.object(builder, "os", SimpleNamespace(name="nt", environ=inherited)), \
                 mock.patch.object(builder.subprocess, "run", side_effect=responses):
                environment, ninja = builder.build_environment()

        self.assertEqual(ninja, installation / "Common7/IDE/CommonExtensions/Microsoft/CMake/Ninja/ninja.exe")
        self.assertEqual(inherited, inherited_before)
        self.assertEqual(dict(os.environ), global_before)
        self.assertEqual(environment["CALLER_ONLY"], "retained=value")
        self.assertEqual(environment["EMPTY_VALUE"], "")
        # The CUDA caller indexes PATH; the packager indexes VCToolsRedistDir.
        self.assertEqual(environment["VCToolsRedistDir"], vcvars_values["VCToolsRedistDir"])
        with self.subTest(contract="unique Windows environment keys"):
            self.assertEqual(len(environment), len({key.upper() for key in environment}))
        with self.subTest(contract="canonical PATH retains compiler tools and caller path"):
            self.assertEqual(environment["PATH"], tool_path)
        for key, expected in vcvars_values.items():
            with self.subTest(contract="vcvars overrides inherited value", variable=key):
                self.assertEqual({value for name, value in environment.items() if name.upper() == key.upper()},
                                 {expected})


class CpuCudaNativeBuildTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.source, self.build, self.output, self.cuda, self.vulkan, redist = (
            self.root / name for name in ("source", "build", "output", "cuda", "vulkan", "redist")
        )
        for relative in ("LICENSE", "external/ggml/LICENSE", "external/sentencepiece/LICENSE"):
            path = self.source / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("synthetic license", encoding="utf-8")
        (self.build / "bin").mkdir(parents=True)
        (self.build / "bin/audiocpp.dll").write_bytes(b"synthetic native library")
        for name in ("msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll", "vcomp140.dll"):
            path = redist / "x64/CRT" / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"synthetic redistributable")
        for relative in ("bin/nvcc.exe", "bin/cublas64_12.dll", "bin/cublasLt64_12.dll", "bin/cudart64_12.dll", "LICENSE"):
            path = self.cuda / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"synthetic CUDA build input")
        # A complete SDK prevents obsolete Vulkan support from failing by accident.
        for relative in ("Include/vulkan/vulkan.h", "Lib/vulkan-1.lib", "Bin/glslc.exe"):
            path = self.vulkan / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"synthetic retired build input")
        self.environment = {"PATH": "existing-build-path", "VULKAN_SDK": str(self.vulkan),
                            "VCToolsRedistDir": str(redist)}
        self.enterContext(mock.patch.dict(os.environ, {"VULKAN_SDK": str(self.vulkan)}))
        self.prepare = self.enterContext(mock.patch.object(builder, "prepare_source"))
        self.build_environment = self.enterContext(mock.patch.object(
            builder, "build_environment", side_effect=lambda: (self.environment.copy(), self.root / "ninja.exe")
        ))
        self.configure_stdout = self.cuda_configure_log(
            "50-virtual;61-virtual;70-virtual;75-real;80-virtual;86-real;89-real;90-virtual;120a-real;121a-real"
        )
        self.configure_count = 0
        self.reconfigure_stdout = None
        self.reconfigure_failure = None

        def fake_run(command, **kwargs):
            captures_stdout = kwargs.get("capture_output") or kwargs.get("stdout") == builder.subprocess.PIPE
            stdout = self.configure_stdout if captures_stdout and "-S" in command else None
            if stdout is not None:
                self.configure_count += 1
                if self.configure_count > 1:
                    if self.reconfigure_failure is not None:
                        raise self.reconfigure_failure
                    requested = next((argument.partition("=")[2] for argument in command
                                      if argument.startswith("-DCMAKE_CUDA_ARCHITECTURES=")), None)
                    stdout = (self.reconfigure_stdout if self.reconfigure_stdout is not None
                              else self.cuda_configure_log(requested or ""))
            if stdout is not None and not any(kwargs.get(key) for key in ("text", "encoding", "universal_newlines")):
                stdout = stdout.encode("utf-8")
            return SimpleNamespace(returncode=0, stdout=stdout, stderr=None)

        self.subprocess = self.enterContext(mock.patch.object(
            builder.subprocess, "run", side_effect=fake_run
        ))
        self.enterContext(contextlib.redirect_stdout(io.StringIO()))
        self.enterContext(contextlib.redirect_stderr(io.StringIO()))

    def invoke(self, backend, extra=()):
        self.configure_count = 0
        arguments = ["build_r2t2_runtime.py", "--backend", backend,
                     "--source", str(self.source), "--build-dir", str(self.build),
                     "--output", str(self.output / backend), "--parallel", "2", *extra]
        if backend == "cuda":
            arguments += ["--cuda-root", str(self.cuda)]
        with mock.patch.object(sys, "argv", arguments):
            builder.main()

    @staticmethod
    def cuda_configure_log(architectures, native="89"):
        # Pinned root and ggml CMake both emit this summary; the host is separate.
        summary = (f"-- Using CMAKE_CUDA_ARCHITECTURES={architectures} "
                   f"CMAKE_CUDA_ARCHITECTURES_NATIVE={native}\n")
        return "-- Found CUDAToolkit\n" + summary + "-- Including CUDA backend\n" + summary

    def test_cuda_default_clears_cached_architectures_and_defers_to_upstream(self):
        (self.build / "CMakeCache.txt").write_text(
            "CMAKE_CUDA_ARCHITECTURES:STRING=89\n", encoding="utf-8"
        )
        self.invoke("cuda")
        command = self.subprocess.call_args_list[0].args[0]
        self.assertFalse(any(arg.startswith("-DCMAKE_CUDA_ARCHITECTURES") for arg in command), command)
        self.assertTrue(
            "-UCMAKE_CUDA_ARCHITECTURES" in command
            or any(command[index:index + 2] == ["-U", "CMAKE_CUDA_ARCHITECTURES"]
                   for index in range(len(command) - 1)),
            command,
        )

    def test_cuda_default_removes_ambient_architectures_case_insensitively(self):
        # build_environment merges os.environ and vcvars output into a plain dict.
        self.environment.update({"CUDAARCHS": "89", "cudaarchs": "native", "CuDaArChS": "80-real"})
        environment_before = self.environment.copy()
        with mock.patch.dict(os.environ, {"CUDAARCHS": "native"}):
            global_before = dict(os.environ)
            self.invoke("cuda")
            self.assertEqual(dict(os.environ), global_before)
        self.assertEqual(self.environment, environment_before)
        for call in self.subprocess.call_args_list:
            self.assertFalse(any(key.upper() == "CUDAARCHS" for key in call.kwargs["env"]), call)

    def test_cuda_manifest_records_resolved_portable_lists_for_each_toolchain(self):
        common = "75-real;80-virtual;86-real;89-real;90-virtual"
        legacy = "50-virtual;61-virtual;70-virtual;" + common
        # Synthetic summaries mirror the pinned source's toolkit-dependent lists.
        for toolkit, resolved in (
            ("12.0", legacy),
            ("12.8", legacy + ";120a-real"),
            ("12.9", legacy + ";120a-real;121a-real"),
            ("13.0", common + ";120a-real;121a-real"),
        ):
            with self.subTest(toolkit=toolkit):
                self.configure_stdout = self.cuda_configure_log(resolved)
                self.invoke("cuda")
                manifest = json.loads((self.output / "cuda/runtime-manifest.json").read_text(encoding="utf-8"))
                self.assertEqual(manifest["cuda_architectures"], resolved + ";80-real;90-real")

    def test_cuda_default_adds_only_missing_selected_generic_native_targets(self):
        cases = (
            ("80-virtual;90-virtual", "80-virtual;90-virtual;80-real;90-real"),
            ("90-virtual;80-virtual;100f-real;130a-virtual",
             "90-virtual;80-virtual;100f-real;130a-virtual;80-real;90-real"),
            ("80-virtual;80-virtual;86-real", "80-virtual;80-virtual;86-real;80-real"),
            ("90-virtual;89-real", "90-virtual;89-real;90-real"),
            ("80-virtual;90-virtual;90a-real", "80-virtual;90-virtual;90a-real;80-real;90-real"),
            ("80-virtual;80;90-virtual;90-real", "80-virtual;80;90-virtual;90-real"),
            ("80-virtual;80-real;80-real;90-virtual;90", "80-virtual;80-real;80-real;90-virtual;90"),
            ("80-real;90-real;130a-virtual", "80-real;90-real;130a-virtual"),
            ("90a-virtual;90a-real;100f-real;130a-virtual", "90a-virtual;90a-real;100f-real;130a-virtual"),
            ("80a-virtual;90f-virtual;89-real", "80a-virtual;90f-virtual;89-real"),
        )
        for upstream, expected in cases:
            with self.subTest(upstream=upstream):
                self.configure_stdout = self.cuda_configure_log(upstream)
                self.subprocess.reset_mock()
                self.invoke("cuda")
                commands = [call.args[0] for call in self.subprocess.call_args_list]
                configures = [command for command in commands if "-S" in command]
                self.assertEqual(len(configures), 2 if upstream != expected else 1,
                                 "default PTX-only generic targets require a second CMake configure")
                self.assertFalse(any(arg.startswith("-DCMAKE_CUDA_ARCHITECTURES=") for arg in configures[0]))
                if upstream != expected:
                    self.assertIn(f"-DCMAKE_CUDA_ARCHITECTURES={expected}", configures[1])
                    self.assertEqual(commands[:-1], configures)
                self.assertIn("--build", commands[-1])
                manifest = json.loads((self.output / "cuda/runtime-manifest.json").read_text(encoding="utf-8"))
                self.assertEqual(manifest["cuda_architectures"], expected)

    def test_cuda_reconfigure_must_report_exact_planned_list_before_compile_or_publish(self):
        upstream = "80-virtual;90-virtual;100f-real;130a-virtual"
        planned = upstream + ";80-real;90-real"
        reports = {
            "stale": self.cuda_configure_log(upstream),
            "lost-upstream-ptx": self.cuda_configure_log("100f-real;130a-virtual;80-real;90-real"),
            "unexpected-target": self.cuda_configure_log(planned + ";140-real"),
            "reordered": self.cuda_configure_log("90-virtual;80-virtual;100f-real;130a-virtual;80-real;90-real"),
            "missing": "-- CMake configure complete\n",
            "conflicting": self.cuda_configure_log(planned) + self.cuda_configure_log(upstream),
            "invalid": self.cuda_configure_log("native"),
        }
        directory = self.output / "cuda"
        directory.mkdir(parents=True)
        (directory / "runtime-manifest.json").write_bytes(b'{"backend":"cuda","build_id":"previous"}\n')
        (directory / "audiocpp.dll").write_bytes(b"previous native library")
        before = {path.name: path.read_bytes() for path in directory.iterdir()}
        self.configure_stdout = self.cuda_configure_log(upstream)
        for case, report in reports.items():
            with self.subTest(case=case):
                self.reconfigure_stdout = report
                self.subprocess.reset_mock()
                with self.assertRaises((RuntimeError, SystemExit)) as failure:
                    self.invoke("cuda")
                if isinstance(failure.exception, SystemExit):
                    self.assertNotEqual(failure.exception.code, 0)
                self.assertEqual(self.subprocess.call_count, 2)
                self.assertFalse(any("--build" in call.args[0] for call in self.subprocess.call_args_list))
                self.assertEqual({path.name: path.read_bytes() for path in directory.iterdir()}, before)

    def test_cuda_reconfigure_failure_stops_before_compile_or_publish(self):
        self.configure_stdout = self.cuda_configure_log("80-virtual;90-virtual")
        self.reconfigure_failure = builder.subprocess.CalledProcessError(
            1, ["cmake"], output="synthetic second configure failure\n"
        )
        with self.assertRaises(builder.subprocess.CalledProcessError):
            self.invoke("cuda")
        self.assertEqual(self.subprocess.call_count, 2)
        self.assertFalse(any("--build" in call.args[0] for call in self.subprocess.call_args_list))
        self.assertFalse(self.output.exists())

    def test_cuda_explicit_architectures_override_environment_and_record_normalization(self):
        requested = "80-virtual;90a;100f-real;120;121-real"
        resolved = "80-virtual;90a;100f-real;120a;121a-real"
        self.environment["CUDAARCHS"] = "native"
        self.configure_stdout = "-- Replacing 120 in CMAKE_CUDA_ARCHITECTURES with 120a\n" + self.cuda_configure_log(resolved)
        self.invoke("cuda", ("--cuda-architectures", requested))
        self.assertEqual(self.configure_count, 1, "explicit architecture overrides must not force native targets")
        command = self.subprocess.call_args_list[0].args[0]
        self.assertIn(f"-DCMAKE_CUDA_ARCHITECTURES={requested}", command)
        assignment_index = command.index(f"-DCMAKE_CUDA_ARCHITECTURES={requested}")
        for index, argument in enumerate(command):
            if argument == "-U":
                pattern = command[index + 1]
            elif argument.startswith("-U"):
                pattern = argument[2:]
            else:
                continue
            if fnmatch.fnmatchcase("CMAKE_CUDA_ARCHITECTURES", pattern):
                self.assertLess(index, assignment_index, "cache clearing erases the explicit architecture override")
        manifest = json.loads((self.output / "cuda/runtime-manifest.json").read_text(encoding="utf-8"))
        self.assertEqual(manifest["cuda_architectures"], resolved)

    def test_cuda_manifest_accepts_concrete_architectures_without_a_host_gpu(self):
        resolved = "75-real;80-virtual;86-real;89-real;90-virtual"
        self.configure_stdout = self.cuda_configure_log(resolved, native="")
        self.invoke("cuda")
        manifest = json.loads((self.output / "cuda/runtime-manifest.json").read_text(encoding="utf-8"))
        self.assertEqual(manifest["cuda_architectures"], resolved + ";80-real;90-real")

    def test_cuda_explicit_plain_numeric_architectures_remain_supported(self):
        requested = "75;80;86;89;90"
        self.configure_stdout = self.cuda_configure_log(requested)
        self.invoke("cuda", ("--cuda-architectures", requested))
        command = self.subprocess.call_args_list[0].args[0]
        self.assertIn(f"-DCMAKE_CUDA_ARCHITECTURES={requested}", command)
        manifest = json.loads((self.output / "cuda/runtime-manifest.json").read_text(encoding="utf-8"))
        self.assertEqual(manifest["cuda_architectures"], requested)

    def test_cuda_invalid_or_conflicting_resolution_stops_before_compile_or_publish(self):
        valid = self.cuda_configure_log("89-real;90-virtual")
        invalid_logs = {
            "missing": "-- Using CMAKE_CUDA_ARCHITECTURES_NATIVE=89\n",
            "conflicting": valid + self.cuda_configure_log("80-real"),
            "valid_then_empty": valid + self.cuda_configure_log(""),
            "invalid_then_valid": self.cuda_configure_log("native") + valid,
        }
        for value in ("", "native", "all", "all-major", "OFF", "89;native", "89;all",
                      "89-real;bogus", "89,90", "89 90", "89-real; 90-virtual",
                      ";89", "89;", "89;;90", "89-REAL", "90aa-real"):
            invalid_logs[f"invalid:{value}"] = self.cuda_configure_log(value)
        directory = self.output / "cuda"
        directory.mkdir(parents=True)
        for selection in ((), ("--cuda-architectures", "89")):
            for case, configure_stdout in invalid_logs.items():
                with self.subTest(case=case, selection=selection):
                    (directory / "runtime-manifest.json").write_bytes(b'{"backend":"cuda","build_id":"previous"}\n')
                    (directory / "audiocpp.dll").write_bytes(b"previous native library")
                    (directory / "retained.dat").write_bytes(b"previous package data")
                    before = {path.name: path.read_bytes() for path in directory.iterdir()}
                    self.configure_stdout = configure_stdout
                    self.subprocess.reset_mock()
                    environment_before = dict(os.environ)
                    with self.assertRaises((RuntimeError, SystemExit)) as failure:
                        self.invoke("cuda", selection)
                    if isinstance(failure.exception, SystemExit):
                        self.assertNotEqual(failure.exception.code, 0)
                    self.assertEqual(self.subprocess.call_count, 1, "invalid resolution reached compilation")
                    self.assertNotIn("--build", self.subprocess.call_args.args[0])
                    self.assertEqual({path.name: path.read_bytes() for path in directory.iterdir()}, before)
                    self.assertEqual(dict(os.environ), environment_before)

    def test_cpu_ignores_cuda_resolution_and_preserves_ambient_architectures(self):
        self.configure_stdout = "-- CPU configure complete\n"
        self.environment["CUDAARCHS"] = "native"
        self.invoke("cpu")
        configure, compile_call = self.subprocess.call_args_list
        self.assertEqual(configure.kwargs["env"]["CUDAARCHS"], "native")
        self.assertEqual(compile_call.kwargs["env"]["CUDAARCHS"], "native")
        self.assertFalse(any("CMAKE_CUDA_ARCHITECTURES" in arg for arg in configure.args[0]))
        manifest = json.loads((self.output / "cpu/runtime-manifest.json").read_text(encoding="utf-8"))
        self.assertIsNone(manifest["cuda_architectures"])

    def test_cpu_and_cuda_explicitly_disable_vulkan_and_keep_runtime_manifest_hashes(self):
        for backend in ("cpu", "cuda"):
            with self.subTest(backend=backend):
                self.subprocess.reset_mock()
                before = dict(os.environ)
                self.invoke(backend)
                configure, *_, compile_call = self.subprocess.call_args_list
                command = configure.args[0]
                self.assertIn("-DENGINE_ENABLE_VULKAN=OFF", command)
                self.assertNotIn("-DENGINE_ENABLE_VULKAN=ON", command)
                self.assertIn(f"-DENGINE_ENABLE_CUDA={'ON' if backend == 'cuda' else 'OFF'}", command)
                self.assertEqual(compile_call.args[0], [
                    "cmake", "--build", str(self.build), "--target", "audiocpp", "--parallel", "2",
                ])
                self.assertEqual(dict(os.environ), before)
                self.assertEqual(configure.kwargs["env"]["VULKAN_SDK"], str(self.vulkan))
                self.assertNotIn(str(self.vulkan / "Bin"), configure.kwargs["env"]["PATH"].split(os.pathsep))
                directory = self.output / backend
                manifest = json.loads((directory / "runtime-manifest.json").read_text(encoding="utf-8"))
                self.assertEqual(manifest["backend"], backend)
                self.assertEqual(set(manifest["files"]), {path.name for path in directory.iterdir()} - {"runtime-manifest.json"})
                for name, metadata in manifest["files"].items():
                    data = (directory / name).read_bytes()
                    self.assertEqual(metadata, {"size": len(data), "sha256": hashlib.sha256(data).hexdigest()})

    def assert_cli_rejected(self, backend, extra=()):
        with self.assertRaises(SystemExit) as failure:
            self.invoke(backend, extra)
        self.assertNotEqual(failure.exception.code, 0)
        self.prepare.assert_not_called()
        self.build_environment.assert_not_called()
        self.subprocess.assert_not_called()
        self.assertFalse(self.output.exists())

    def test_vulkan_backend_rejects_before_source_or_build_work(self):
        self.assert_cli_rejected("vulkan")

    def test_auto_backend_rejects_before_source_or_build_work(self):
        self.assert_cli_rejected("auto")

    def test_retired_vulkan_root_cli_rejects_before_source_or_build_work(self):
        self.assert_cli_rejected("cpu", ("--vulkan-root", str(self.vulkan)))


class CpuCudaPackageTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.resources = self.root / "resources"
        self.resources.mkdir()
        self.module = load_build_engine_module()
        self.module.PROJECT_ROOT = self.root
        self.module.RESOURCES_DIR = self.resources
        self.module.ADD_DATA_FILES = ["engine.py"]
        self.module.ENTRY_SCRIPT = self.resources / "engine.py"
        self.module.ENTRY_SCRIPT.write_bytes(b"# synthetic engine\n")
        self.module.DIST_DIR = self.root / "dist"
        self.module.OUTPUT_ARCHIVE = self.resources / "engine.tar.xz"
        self.module.WINDOWS_MANIFEST = self.root / "missing-manifest.xml"
        self.archive = self.root / "reused.tar.xz"
        self.members = {"_internal/engine.py": self.module.ENTRY_SCRIPT.read_bytes()}
        for backend in ("cpu", "cuda"):
            self.members.update(self.native_members(backend, write_source=True))
        self.enterContext(mock.patch.object(importlib.metadata, "version", return_value="0.1.3"))
        self.enterContext(contextlib.redirect_stdout(io.StringIO()))
        self.enterContext(contextlib.redirect_stderr(io.StringIO()))

    def native_members(self, backend, *, write_source=False):
        library = f"synthetic {backend} library".encode()
        manifest = {
            "backend": backend, "build_id": self.module.R2T2_BUILD_ID,
            "revision": self.module.R2T2_REVISION,
            "patch_sha256": hashlib.sha256(self.module.R2T2_PATCH.read_bytes()).hexdigest(),
            "files": {"audiocpp.dll": {"size": len(library), "sha256": hashlib.sha256(library).hexdigest()}},
        }
        files = {"audiocpp.dll": library, "runtime-manifest.json": json.dumps(manifest).encode()}
        if write_source:
            directory = self.resources / "r2t2-native" / backend
            directory.mkdir(parents=True)
            for name, data in files.items():
                (directory / name).write_bytes(data)
        return {f"_internal/r2t2-native/{backend}/{name}": data for name, data in files.items()}

    def package(self, members=None):
        with tarfile.open(self.archive, "w:xz") as archive:
            for name, data in (self.members if members is None else members).items():
                member = tarfile.TarInfo(name)
                member.size = len(data)
                archive.addfile(member, io.BytesIO(data))

    def test_cpu_cuda_sources_validate_without_vulkan(self):
        try:
            actual = self.module.validate_r2t2_runtime()
        except RuntimeError as error:
            self.fail(f"Current CPU/CUDA-only source package rejected: {error}")
        self.assertEqual(actual, self.resources / "r2t2-native")

    def test_stale_extra_local_vulkan_directory_does_not_block_cpu_cuda_validation(self):
        directory = self.resources / "r2t2-native/vulkan"
        directory.mkdir()
        (directory / "runtime-manifest.json").write_text('{"backend":"retired"}', encoding="utf-8")
        (directory / "audiocpp.dll").write_bytes(b"old local native package")
        try:
            actual = self.module.validate_r2t2_runtime()
        except RuntimeError as error:
            self.fail(f"Unselected retired local package blocked CPU/CUDA: {error}")
        self.assertEqual(actual, directory.parent)

    def test_current_cpu_cuda_archive_passes_without_retired_backend(self):
        self.package()
        try:
            self.module.verify_archive_sources(self.archive)
        except RuntimeError as error:
            self.fail(f"Current CPU/CUDA-only archive rejected: {error}")

    def test_old_three_backend_archive_is_rejected_even_with_valid_vulkan_manifest(self):
        self.package({**self.members, **self.native_members("vulkan")})
        with self.assertRaises(RuntimeError):
            self.module.verify_archive_sources(self.archive)

    def test_retired_subtrees_and_dlls_are_rejected_with_normalized_slashes_and_case(self):
        self.package()
        try:
            self.module.verify_archive_sources(self.archive)
        except RuntimeError as error:
            self.fail(f"Valid CPU/CUDA archive must pass before forbidden-member probes: {error}")
        for name in (
            "_internal/r2t2-native/vulkan/retired.dat",
            r".\_INTERNAL\R2T2-NATIVE\VULKAN\retired.dat",
            r".\_INTERNAL\TRANSCRIBE_CPP_NATIVE_CU12\_NATIVE\GGML-VULKAN.DLL",
            "_internal/vulkan-1.dll",
            r"plugins\nested\VuLkAn-1.DlL",
        ):
            with self.subTest(member=name):
                self.package({**self.members, name: b"retired library"})
                with self.assertRaises(RuntimeError):
                    self.module.verify_archive_sources(self.archive)

    def test_main_selects_each_backend_directory_without_collecting_stale_vulkan_tree(self):
        # Valid old files let this test reach the real CLI seam before implementation.
        self.native_members("vulkan", write_source=True)
        with mock.patch.object(self.module.subprocess, "run", return_value=SimpleNamespace(returncode=23)) as run:
            with self.assertRaises(SystemExit) as stopped:
                self.module.main()
        self.assertEqual(stopped.exception.code, 23)
        command = run.call_args.args[0]
        selected = [command[index + 1] for index, arg in enumerate(command) if arg == "--add-data"]
        native = [(Path(source), destination.replace("\\", "/"))
                  for value in selected if "r2t2-native" in value
                  for source, destination in [value.rsplit(os.pathsep, 1)]]
        self.assertCountEqual(native, [
            (self.resources / "r2t2-native" / backend, f"r2t2-native/{backend}")
            for backend in ("cpu", "cuda")
        ])

    def test_main_prunes_only_retired_collected_dlls_before_compression_preserving_source_wheel(self):
        # Fake COLLECT copies a real fixture tree. Source wheel remains independent.
        self.native_members("vulkan", write_source=True)
        wheel = self.root / "source-wheel"
        retained = {
            "engine.exe": b"synthetic executable",
            "_internal/r2t2-native/cpu/audiocpp.dll": b"CPU library",
            "_internal/r2t2-native/cuda/audiocpp.dll": b"CUDA library",
            "_internal/onnxruntime/capi/onnxruntime_pybind11_state.pyd": b"ORT pybind",
            "_internal/onnxruntime/capi/onnxruntime_providers_shared.dll": b"ORT shared provider",
            "_internal/onnxruntime/capi/onnxruntime_providers_future.dll": b"future ORT provider",
            "_internal/kaldi_native_fbank/LICENSE": b"Kaldi license",
            "_internal/data/vulkan_settings.json": b"ordinary data",
        }
        retired = {
            "_internal/plugins/ggml-vulkan.dll": b"retired Vulkan",
            "_internal/plugins/VULKAN-1.DLL": b"retired Vulkan loader",
            "_internal/plugins/nested/GGML-VULKAN.DLL": b"nested retired copy",
            "_internal/vulkan-1.dll": b"root retired loader",
        }
        for relative, data in {**retained, **retired}.items():
            path = wheel / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)

        def snapshot(directory):
            return {path.relative_to(directory).as_posix(): path.read_bytes()
                    for path in directory.rglob("*") if path.is_file()}

        before = snapshot(wheel)
        captured = {}

        def fake_collect(_command):
            shutil.copytree(wheel, self.module.DIST_DIR / "engine")
            return SimpleNamespace(returncode=0)

        def capture_compression(directory, output):
            captured.update(snapshot(directory))
            output.write_bytes(b"synthetic nonempty archive")
            return 1.0

        with mock.patch.object(self.module.subprocess, "run", side_effect=fake_collect), \
             mock.patch.object(self.module, "create_tar_xz", side_effect=capture_compression):
            self.module.main()
        self.assertEqual(captured, retained)
        self.assertEqual(snapshot(wheel), before, "package pruning modified source wheel files")


if __name__ == "__main__":
    unittest.main()
