

from pathlib import PureWindowsPath

from PyInstaller.utils.hooks import collect_dynamic_libs



binaries = [
    (source, destination)
    for source, destination in collect_dynamic_libs("onnxruntime")
    if not (
        PureWindowsPath(source).name.casefold() == "onnxruntime.dll"
        and PureWindowsPath(destination).as_posix().casefold() == "onnxruntime/capi"
    )
]
