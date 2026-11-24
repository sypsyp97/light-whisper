

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
from build_reuse_runtime import source_fingerprint
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

            with mock.patch.object(builder, "os", SimpleNamespace(name="nt", environ=inherited)), \
                 mock.patch.object(builder.subprocess, "run", side_effect=responses):
                environment, ninja = builder.build_environment()

        self.assertEqual(ninja, installation / "Common7/IDE/CommonExtensions/Microsoft/CMake/Ninja/ninja.exe")
        self.assertEqual(inherited, inherited_before)
        self.assertEqual(dict(os.environ), global_before)
        self.assertEqual(environment["CALLER_ONLY"], "retained=value")
        self.assertEqual(environment["EMPTY_VALUE"], "")

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
        self.enterContext(mock.patch.object(builder, "ROOT", self.root))

        def fake_reuse_build(backend, _source, _build, output):
            output.mkdir(parents=True, exist_ok=True)
            (output / "reuse.dll").write_bytes(f"synthetic {backend} enhancement library".encode())

        self.enterContext(mock.patch("build_reuse_runtime.build", side_effect=fake_reuse_build))
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
        from native_package_fixture import EngineFixture
        tmp=tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root=Path(tmp.name)
        self.module=load_build_engine_module()
        self.fixture=EngineFixture(self,self.module,self.root)
        self.resources=self.fixture.resources
        self.archive=self.fixture.archive
        self.members=self.fixture.sources|self.fixture.native

    def native_members(self,backend,*,write_source=False):
        return self.fixture.make_backend(backend,write_source)

    def package(self,members=None):
        self.fixture.package(self.members if members is None else members)

    def test_cpu_cuda_sources_validate_without_vulkan(self):
        stage=self.root/"stage"
        self.module.copy_native(stage)
        self.assertEqual({p.name for p in (stage/"_internal/r2t2-native").iterdir()},{"cpu","cuda"})

    def test_unselected_stale_local_vulkan_tree_is_neither_published_nor_mutated(self):
        old=self.resources/"r2t2-native/vulkan/retired.dll"
        old.parent.mkdir()
        old.write_bytes(b"old independent payload")
        stage=self.root/"stage"
        self.module.stage_engine(stage)
        self.assertFalse((stage/"_internal/r2t2-native/vulkan").exists())
        self.assertEqual(old.read_bytes(),b"old independent payload")
        self.assertTrue(all((stage/"_internal/r2t2-native"/b/"audiocpp.dll").is_file() for b in ("cpu","cuda")))

    def test_current_cpu_cuda_archive_passes_without_retired_backend(self):
        self.package()
        try:
            self.module.verify_archive_sources(self.archive)
        except RuntimeError as error:
            self.fail(f"Current CPU/CUDA-only archive rejected: {error}")

    def test_old_three_backend_archive_is_rejected_even_with_valid_vulkan_manifest(self):
        self.package({**self.members, **self.native_members("vulkan")})
        with self.assertRaises((RuntimeError,ValueError)):
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
                with self.assertRaises((RuntimeError,ValueError)):
                    self.module.verify_archive_sources(self.archive)

    def test_native_assembly_leaves_source_packages_intact(self):
        snapshot=lambda:{p.relative_to(self.resources):p.read_bytes() for p in self.resources.rglob("*") if p.is_file()}
        before=snapshot()
        stage=self.root/"stage"
        self.module.stage_engine(stage)
        self.assertEqual(snapshot(),before)
        self.assertTrue((stage/"engine.exe").is_file())
        self.assertFalse(any(p.suffix in (".py",".pyd") for p in stage.rglob("*")))


if __name__=="__main__":
    unittest.main()
