#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
PyInstaller 打包入口脚本。

通过子命令分发到不同功能：
  engine serve --engine local      → 启动本地 MLX ASR 服务器
  engine download --engine local   → 下载本地 MLX 模型
"""

import sys
import os
import argparse


def _setup_frozen_paths():
    """PyInstaller frozen 环境下，将 _internal/ 加入 sys.path"""
    if getattr(sys, "frozen", False):
        base = os.path.dirname(sys.executable)
        internal = os.path.join(base, "_internal")
        if os.path.isdir(internal) and internal not in sys.path:
            sys.path.insert(0, internal)


def cmd_serve(engine: str):
    from whisper_server import WhisperServer

    _ = engine
    server = WhisperServer()
    server.run()


def cmd_download(engine: str):
    from download_models import main
    main(engine=engine)


def main():
    _setup_frozen_paths()

    parser = argparse.ArgumentParser(prog="engine")
    sub = parser.add_subparsers(dest="command", required=True)

    serve_p = sub.add_parser("serve")
    serve_p.add_argument("--engine", required=True, choices=["local"])

    dl_p = sub.add_parser("download")
    dl_p.add_argument("--engine", required=True, choices=["local"])

    args = parser.parse_args()

    if args.command == "serve":
        cmd_serve(args.engine)
    elif args.command == "download":
        cmd_download(args.engine)


if __name__ == "__main__":
    main()
