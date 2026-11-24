"""Native-only staging excludes the Python wheel runtime and preserves different CRT builds."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from native_package_fixture import EngineFixture, metadata
from test_build_engine_atomicity import load_build_engine_module


class DependencyStagingTests(unittest.TestCase):
    def setUp(self):
        tmp=tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root=Path(tmp.name)
        self.builder=load_build_engine_module()
        self.fixture=EngineFixture(self,self.builder,self.root)

    def test_only_required_onnx_fbank_and_soundfile_dlls_are_staged(self):
        # A wheel with pybind and an extra copy of the Kaldi DLL can still be used by build tooling.
        extras=["onnxruntime/capi/OnNxRuNtImE.DlL","onnxruntime/capi/onnxruntime_pybind11_state.pyd",
                "kaldi_native_fbank/lib/kaldi-native-fbank-core.dll","legacy/hf_xet.pyd"]
        for name in extras:
            path=self.fixture.resources/name
            path.parent.mkdir(parents=True,exist_ok=True)
            path.write_bytes(b"unselected wheel bytes")
        stage=self.root/"stage"
        self.builder.stage_engine(stage)
        chosen={p.relative_to(stage).as_posix() for p in stage.rglob("*") if p.is_file()}
        self.assertFalse(set(extras)&chosen)
        for name in ("onnxruntime.dll","kaldi-native-fbank-core.dll","_internal/_soundfile_data/libsndfile_x64.dll"):
            self.assertIn(name,chosen)
            self.assertEqual(sum(Path(member).name.lower()==Path(name).name.lower() for member in chosen),1)
        for name in extras:
            self.assertEqual((self.fixture.resources/name).read_bytes(),b"unselected wheel bytes")

    def test_different_same_size_crt_build_remains_backend_local(self):
        cuda=self.fixture.resources/"r2t2-native/cuda"
        name="msvcp140.dll"
        current=(cuda/name).read_bytes()
        changed=current[:-1]+bytes([current[-1]^1])
        (cuda/name).write_bytes(changed)
        manifest=cuda/"runtime-manifest.json"
        data=json.loads(manifest.read_bytes())
        data["files"][name]=metadata(changed)
        manifest.write_text(json.dumps(data))
        stage=self.root/"stage"
        self.builder.stage_engine(stage)
        self.assertEqual((stage/name).read_bytes(),current)
        self.assertEqual((stage/"_internal/r2t2-native/cuda"/name).read_bytes(),changed)
        self.assertEqual((cuda/name).read_bytes(),changed)
        packed=json.loads((stage/"_internal/r2t2-native/cuda/runtime-manifest.json").read_bytes())
        self.assertIn(name,packed["files"])
        self.assertNotIn(name,packed["shared_files"])


if __name__=="__main__":
    unittest.main()
