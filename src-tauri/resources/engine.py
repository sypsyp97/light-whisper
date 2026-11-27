#!/usr/bin/env python3
"""Run the Python reference engine for development and parity tests."""

import argparse


def cmd_serve(engine: str):
    if engine == "confucius4-r2t2":
        from r2t2_asr_server import R2T2ASRServer
        server = R2T2ASRServer()
    elif engine.startswith("qwen3-asr-"):
        from qwen3_asr_server import Qwen3ASRServer
        server = Qwen3ASRServer(engine=engine)
    else:
        raise ValueError(f"不支持的本地 ASR 引擎: {engine}")
    server.run()


def cmd_download(engine: str):
    from download_models import main
    main(engine=engine)


def main():
    parser = argparse.ArgumentParser(prog="engine")
    sub = parser.add_subparsers(dest="command", required=True)

    serve_p = sub.add_parser("serve")
    engine_choices = ["qwen3-asr-0.6b", "confucius4-r2t2"]
    serve_p.add_argument("--engine", required=True, choices=engine_choices)

    dl_p = sub.add_parser("download")
    dl_p.add_argument("--engine", required=True, choices=engine_choices)

    args = parser.parse_args()

    if args.command == "serve":
        cmd_serve(args.engine)
    else:
        cmd_download(args.engine)


if __name__ == "__main__":
    main()
