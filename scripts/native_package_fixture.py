"""Independent AMD64 PE and documented native build stamp fixtures for packaging tests."""
import hashlib
import struct


def pe_image():
    image = bytearray(1024)
    image[:2] = b"MZ"
    struct.pack_into("<I", image, 0x3c, 128)
    image[128:132] = b"PE\0\0"
    struct.pack_into("<HHIIIHH", image, 132, 0x8664, 1, 0, 0, 0, 240, 0x22)
    opt = 152
    struct.pack_into("<H", image, opt, 0x20b)
    struct.pack_into("<I", image, opt+4, 512)
    struct.pack_into("<IIQII", image, opt+16, 0x1000, 0x1000, 0x140000000, 4096, 512)
    struct.pack_into("<II", image, opt+56, 8192, 512)
    struct.pack_into("<H", image, opt+68, 3)
    struct.pack_into("<I", image, opt+108, 16)
    section = opt+240
    struct.pack_into("<8sIIIIIIHHI",image,section,b".text",1,4096,512,512,0,0,0,0,0x60000020)
    image[512] = 0xc3
    return bytes(image)


def executable(component, fingerprint, payload=None):
    payload = pe_image() if payload is None else bytes(payload)
    return (payload + b"LIGHT_WHISPER_RUST_BUILD_V1\0" + component.encode("ascii") + b"\0"
            + bytes.fromhex(fingerprint) + hashlib.sha256(payload).digest())

import io
import json
from pathlib import Path
import tarfile
from unittest import mock


def metadata(data):
    return {"size":len(data),"sha256":hashlib.sha256(data).hexdigest()}


class EngineFixture:
    """Independent metadata for tiny two-backend release inputs, with actual packaged structure."""
    fingerprint = "3f"*32

    def __init__(self,case,module,root):
        self.root=root
        self.module=module
        self.resources=root/"resources"
        self.resources.mkdir(exist_ok=True)
        original_resources=module.RESOURCES_DIR
        for name in getattr(module,"ADD_DATA_FILES",()):
            if name.endswith(".py"):
                (self.resources/name).write_bytes((original_resources/name).read_bytes())
        module.RESOURCES_DIR=self.resources
        self.archive=root/"fixture.tar.xz"
        case.enterContext(mock.patch.dict("os.environ",LW_NATIVE_RESOURCES=str(self.resources)))
        self.sources={}
        dependencies=("onnxruntime.dll","kaldi-native-fbank-core.dll","_internal/_soundfile_data/libsndfile_x64.dll",
                      "_internal/fireredvad_vad.onnx","_internal/fireredvad_cmvn.json")
        source_paths={}
        for name in dependencies:
            data=pe_image() if name.endswith(".dll") else f"static fixture {name}".encode()
            file=root/"SDK"/name
            file.parent.mkdir(parents=True,exist_ok=True)
            file.write_bytes(data)
            source_paths[name]=file
            self.sources[name]=data
        self.sources["engine.exe"]=executable("engine",self.fingerprint)
        self.native={}
        for backend in ("cpu","cuda"):
            self.native.update(self.make_backend(backend,True))
        case.enterContext(mock.patch.object(module,"source_fingerprint",create=True,side_effect=lambda _:self.fingerprint))
        case.enterContext(mock.patch.object(module,"static_sources",create=True,return_value=source_paths))
        case.enterContext(mock.patch.object(module,"sdk_sources",create=True,return_value={}))
        case.enterContext(mock.patch.object(module,"static_pins",create=True,return_value={
            name: metadata(self.sources[name])["sha256"] for name in dependencies}))
        def build_binary(component,stage):
            dest=stage/"engine.exe"
            dest.write_bytes(executable(component,self.fingerprint))
            return dest
        case.enterContext(mock.patch.object(module,"build_binary",create=True,side_effect=build_binary))

    def make_backend(self,backend,write_source=False):
        from build_reuse_runtime import source_fingerprint
        prefix=f"_internal/r2t2-native/{backend}/"
        from build_r2t2_runtime import BUILD_ID, SOURCE_REVISION, PATCH
        crt=["msvcp140.dll","vcruntime140.dll","vcruntime140_1.dll","vcomp140.dll"]
        library_names=["audiocpp.dll","audio.cpp-LICENSE.txt","ggml-LICENSE.txt",
                       "sentencepiece-LICENSE.txt",*crt]
        if backend!="vulkan":
            library_names.append("reuse.dll")
        if backend=="cuda":
            library_names += ["cublas64_12.dll","cublasLt64_12.dll","cudart64_12.dll","CUDA-LICENSE.txt"]
        blobs={name:pe_image()+f"fixture {backend}/{name}".encode() for name in library_names}
        for name in crt:
            blobs[name]=pe_image()+name.encode()
        manifest={"source":"https://github.com/0xShug0/audio.cpp.git","backend":backend,
                  "build_id":BUILD_ID,"revision":SOURCE_REVISION,
                  "patch_sha256":hashlib.sha256(PATCH.read_bytes()).hexdigest(),
                  "reuse_source_sha256":None if backend=="vulkan" else source_fingerprint(),
                  "files":{n:metadata(v) for n,v in blobs.items()}}
        if write_source:
            directory=self.resources/"r2t2-native"/backend
            directory.mkdir(parents=True,exist_ok=True)
            for n,v in blobs.items():
                (directory/n).write_bytes(v)
            (directory/"runtime-manifest.json").write_text(json.dumps(manifest),encoding="utf-8")
        packed={}
        for name,value in blobs.items():
            if name in crt or name in ("cublas64_12.dll","cublasLt64_12.dll","cudart64_12.dll"):
                manifest.setdefault("shared_files",{})[name]=manifest["files"].pop(name)
                packed[name]=value
            else:
                packed[prefix+name]=value
        packed[prefix+"runtime-manifest.json"]=json.dumps(manifest).encode()
        return packed

    def package(self,members=None,*,extra=(),prepend=()):
        members=dict(self.sources|self.native if members is None else members)
        dist={"format":1,"source_sha256":self.fingerprint,"files":{k:metadata(v) for k,v in members.items()}}
        members["engine-distribution.json"]=json.dumps(dist).encode()
        with tarfile.open(self.archive,"w:xz") as archive:
            for member,data in prepend:
                archive.addfile(member,None if data is None else io.BytesIO(data))
            for name,data in members.items():
                item=tarfile.TarInfo(name)
                item.size=len(data)
                archive.addfile(item,io.BytesIO(data))
            for member,data in extra:
                archive.addfile(member,None if data is None else io.BytesIO(data))
