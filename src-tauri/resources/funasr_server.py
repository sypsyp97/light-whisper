#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
FunASR模型服务器
保持模型在内存中，通过stdin/stdout进行通信
"""

import sys
import json
import os
import logging
import traceback
import signal
import contextlib
import threading

# 设置日志
import tempfile


# 获取日志文件路径
def get_log_path():
    # 尝试从环境变量获取用户数据目录
    if "QUQU_DATA_DIR" in os.environ:
        log_dir = os.path.join(os.environ["QUQU_DATA_DIR"], "logs")
    elif "ELECTRON_USER_DATA" in os.environ:
        log_dir = os.path.join(os.environ["ELECTRON_USER_DATA"], "logs")
    else:
        # 回退到临时目录
        log_dir = os.path.join(tempfile.gettempdir(), "ququ_logs")

    # 确保日志目录存在
    os.makedirs(log_dir, exist_ok=True)
    return os.path.join(log_dir, "funasr_server.log")


log_file_path = get_log_path()

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s - %(levelname)s - %(message)s",
    handlers=[
        logging.FileHandler(log_file_path, encoding="utf-8"),
        logging.StreamHandler(),  # 同时输出到控制台
    ],
)
logger = logging.getLogger(__name__)

# 记录日志文件位置
logger.info(f"FunASR服务器日志文件: {log_file_path}")

_stdout_lock = threading.Lock()
_stdout_refcount = 0
_stdout_original = None
_stdout_devnull = None


@contextlib.contextmanager
def suppress_stdout():
    """上下文管理器：临时重定向stdout到devnull，避免FunASR库的非JSON输出干扰IPC通信"""
    global _stdout_refcount, _stdout_original, _stdout_devnull
    with _stdout_lock:
        if _stdout_refcount == 0:
            _stdout_original = sys.stdout
            _stdout_devnull = open(os.devnull, "w")
            sys.stdout = _stdout_devnull
        _stdout_refcount += 1
    try:
        yield
    finally:
        with _stdout_lock:
            _stdout_refcount -= 1
            if _stdout_refcount <= 0:
                sys.stdout = _stdout_original
                if _stdout_devnull:
                    _stdout_devnull.close()
                _stdout_devnull = None
                _stdout_original = None




def _hf_cache_root():
    """返回 HuggingFace 默认缓存根目录"""
    hf_home = os.environ.get("HF_HOME")
    if hf_home:
        return os.path.join(hf_home, "hub")
    xdg_cache = os.environ.get("XDG_CACHE_HOME")
    if xdg_cache:
        return os.path.join(xdg_cache, "huggingface", "hub")
    return os.path.join(os.path.expanduser("~"), ".cache", "huggingface", "hub")


def _hf_repo_ready(repo_id):
    """检查 HuggingFace 模型是否已缓存"""
    cache_root = _hf_cache_root()
    # HF 缓存目录格式: models--<org>--<model>
    dir_name = "models--" + repo_id.replace("/", "--")
    repo_dir = os.path.join(cache_root, dir_name)
    if not os.path.isdir(repo_dir):
        return False
    # refs 下任意分支文件存在即可视为已缓存
    refs_dir = os.path.join(repo_dir, "refs")
    if os.path.isdir(refs_dir):
        for name in os.listdir(refs_dir):
            if os.path.isfile(os.path.join(refs_dir, name)):
                return True

    # snapshots 目录存在并含子目录也表示已下载
    snapshots_dir = os.path.join(repo_dir, "snapshots")
    if os.path.isdir(snapshots_dir):
        for name in os.listdir(snapshots_dir):
            if os.path.isdir(os.path.join(snapshots_dir, name)):
                return True

    return False


class FunASRServer:
    def __init__(self):
        self.asr_model = None
        self.initialized = False
        self.running = True
        self.transcription_count = 0
        self.total_audio_duration = 0.0
        self.engine = "paraformer"

        # 自动检测推理设备：优先 GPU，不可用时回退 CPU
        self.device = self._detect_device()

        signal.signal(signal.SIGTERM, self._signal_handler)
        signal.signal(signal.SIGINT, self._signal_handler)
        self._setup_runtime_environment()

    def _detect_device(self):
        """检测可用的推理设备，优先使用 GPU"""
        try:
            import torch
            if torch.cuda.is_available():
                gpu_name = torch.cuda.get_device_name(0)
                gpu_mem = torch.cuda.get_device_properties(0).total_memory / (1024**3)
                logger.info(f"检测到 NVIDIA GPU: {gpu_name} ({gpu_mem:.1f}GB)，使用 CUDA 加速")
                return "cuda"
            else:
                logger.info("CUDA 不可用，使用 CPU 推理")
                return "cpu"
        except ImportError:
            logger.info("PyTorch 未安装，使用 CPU 推理")
            return "cpu"
        except Exception as e:
            logger.warning(f"GPU 检测失败: {e}，回退到 CPU 推理")
            return "cpu"

    def _setup_runtime_environment(self):
        """设置运行时环境变量以优化性能"""
        try:
            import os

            # 强制 HuggingFace Hub 离线模式：模型已由 download_models.py 预下载，
            # 避免 AutoModel 内部再次调用 snapshot_download 导致 Windows 上卡住
            os.environ["HF_HUB_OFFLINE"] = "1"
            # 自适应线程数：充分利用多核CPU
            cpu_count = os.cpu_count() or 4
            thread_count = max(4, cpu_count - 2)
            os.environ["OMP_NUM_THREADS"] = str(thread_count)
            logger.info(f"运行时环境变量设置完成，HF_HUB_OFFLINE=1, OMP_NUM_THREADS={thread_count} (CPU核心数: {cpu_count})")
        except Exception as e:
            logger.warning(f"环境设置失败: {str(e)}")

    def _signal_handler(self, signum, frame):
        """处理退出信号"""
        logger.info(f"收到信号 {signum}，准备退出...")
        self.running = False

    def _load_asr_model(self):
        """加载ASR模型"""
        try:
            logger.info(f"开始加载ASR模型 (device={self.device})...")
            with suppress_stdout():
                from funasr import AutoModel

                try:
                    self.asr_model = AutoModel(
                        model="paraformer-zh",
                        model_revision="v2.0.4",
                        vad_model="fsmn-vad",
                        vad_model_revision="v2.0.4",
                        punc_model="ct-punc",
                        punc_model_revision="v2.0.4",
                        hub="hf",
                        vad_kwargs={"hub": "hf"},
                        punc_kwargs={"hub": "hf"},
                        disable_update=True,
                        device=self.device,
                    )
                except Exception as e:
                    logger.warning(f"ct-punc v2.0.4 加载失败：{e}，尝试不指定 revision")
                    self.asr_model = AutoModel(
                        model="paraformer-zh",
                        model_revision="v2.0.4",
                        vad_model="fsmn-vad",
                        vad_model_revision="v2.0.4",
                        punc_model="ct-punc",
                        hub="hf",
                        vad_kwargs={"hub": "hf"},
                        punc_kwargs={"hub": "hf"},
                        disable_update=True,
                        device=self.device,
                    )
            logger.info(f"ASR模型加载完成 (device={self.device})")
            return True
        except Exception as e:
            if self.device == "cuda":
                logger.warning(f"ASR模型GPU加载失败: {e}，回退到CPU")
                self.device = "cpu"
                return self._load_asr_model()
            logger.error(f"ASR模型加载失败: {str(e)}")
            return False

    def initialize(self):
        """初始化 FunASR 模型"""
        # 默认 paraformer 流程
        if self.initialized:
            return {"success": True, "message": "模型已初始化"}

        try:
            import time

            logger.info("正在初始化FunASR模型...")
            start_time = time.time()

            ok = self._load_asr_model()
            if not ok:
                error_msg = "ASR模型加载失败"
                logger.error(error_msg)
                return {"success": False, "error": error_msg, "type": "init_error"}

            total_time = time.time() - start_time
            self.initialized = True
            logger.info(
                f"FunASR模型初始化完成，总耗时: {total_time:.2f}秒"
            )
            return {
                "success": True,
                "message": f"FunASR模型初始化成功，耗时: {total_time:.2f}秒",
                "model_loaded": True,
                "engine": self.engine,
            }

        except ImportError as e:
            error_msg = "FunASR未安装，请先安装FunASR: pip install funasr"
            logger.error(error_msg)
            return {"success": False, "error": error_msg, "type": "import_error", "engine": self.engine}

        except Exception as e:
            error_msg = f"FunASR模型初始化失败: {str(e)}"
            logger.error(error_msg)
            logger.error(traceback.format_exc())
            return {"success": False, "error": error_msg, "type": "init_error", "engine": self.engine}

    def transcribe_audio(self, audio_path, options=None):
        """转录音频文件"""
        import time

        if not self.initialized:
            init_result = self.initialize()
            if not init_result["success"]:
                return init_result

        try:
            # 检查音频文件是否存在
            if not os.path.exists(audio_path):
                return {"success": False, "error": f"音频文件不存在: {audio_path}"}

            total_start = time.time()
            logger.info(f"开始转录音频文件: {audio_path}")

            # 预先获取音频时长，用于决定是否跳过 VAD
            duration = self._get_audio_duration(audio_path)
            logger.info(f"音频时长: {duration:.2f}秒")

            # 设置默认选项
            default_options = {
                "batch_size_s": 300,
                "hotword": "",
            }

            if options:
                default_options.update(options)

            # 执行ASR识别（官方推荐：AutoModel 内部集成 VAD + PUNC）
            asr_start = time.time()
            with suppress_stdout():
                asr_result = self.asr_model.generate(
                    input=audio_path,
                    batch_size_s=default_options["batch_size_s"],
                    hotword=default_options["hotword"],
                )
            asr_elapsed = time.time() - asr_start

            # 提取识别文本
            if isinstance(asr_result, list) and len(asr_result) > 0:
                if isinstance(asr_result[0], dict) and "text" in asr_result[0]:
                    final_text = asr_result[0]["text"]
                else:
                    final_text = str(asr_result[0])
            else:
                final_text = str(asr_result)

            logger.info(f"ASR识别完成，耗时: {asr_elapsed:.2f}秒，文本: {final_text[:100]}...")

            self.transcription_count += 1

            total_elapsed = time.time() - total_start
            logger.info(f"转录完成，总耗时: {total_elapsed:.2f}秒，最终文本: {final_text[:100]}...")

            result = {
                "success": True,
                "text": final_text,
                "raw_text": final_text,
                "confidence": (
                    getattr(asr_result[0], "confidence", 0.0)
                    if isinstance(asr_result, list)
                    else 0.0
                ),
                "duration": duration,
                "language": "zh-CN",
                "model_type": "pytorch",  # 标识使用的是pytorch版本
            }

            # 生产环境：每10次转录后进行内存清理
            if self.transcription_count % 10 == 0:
                self._cleanup_memory()
                logger.info(f"已完成 {self.transcription_count} 次转录，执行内存清理")

            return result

        except Exception as e:
            error_msg = f"音频转录失败: {str(e)}"
            logger.error(error_msg)
            logger.error(traceback.format_exc())
            return {"success": False, "error": error_msg, "type": "transcription_error"}

    def _get_audio_duration(self, audio_path):
        """获取音频时长"""
        try:
            import librosa

            duration = librosa.get_duration(filename=audio_path)
            self.total_audio_duration += duration  # 累计音频时长
            return duration
        except:
            return 0.0

    def _cleanup_memory(self):
        """生产环境内存清理"""
        try:
            import gc

            gc.collect()
            logger.info("内存清理完成")
        except Exception as e:
            logger.warning(f"内存清理失败: {str(e)}")

    def get_performance_stats(self):
        """获取性能统计信息"""
        models_loaded = {
            "asr": self.asr_model is not None,
            "vad": self.asr_model is not None,
            "punc": self.asr_model is not None,
        }
        return {
            "transcription_count": self.transcription_count,
            "total_audio_duration": round(self.total_audio_duration, 2),
            "average_duration": round(
                self.total_audio_duration / max(1, self.transcription_count), 2
            ),
            "initialized": self.initialized,
            "engine": self.engine,
            "models_loaded": models_loaded,
        }

    def check_status(self):
        """检查FunASR状态"""
        try:
            import funasr

            # 获取 GPU 详情
            device_info = {"device": self.device}
            if self.device == "cuda":
                try:
                    import torch
                    device_info["gpu_name"] = torch.cuda.get_device_name(0)
                    device_info["gpu_memory_total"] = round(torch.cuda.get_device_properties(0).total_memory / (1024**3), 1)
                except Exception:
                    pass

            models = {
                "asr": self.asr_model is not None,
                "vad": self.asr_model is not None,
                "punc": self.asr_model is not None,
            }
            model_loaded = self.asr_model is not None

            return {
                "success": True,
                "installed": True,
                "initialized": self.initialized,
                "version": getattr(funasr, "__version__", "unknown"),
                "engine": self.engine,
                "model_loaded": model_loaded,
                "models": models,
                **device_info,
            }
        except ImportError:
            return {
                "success": False,
                "installed": False,
                "initialized": False,
                "error": "FunASR未安装",
            }

    def run(self):
        """运行服务器主循环"""
        logger.info("FunASR服务器启动")

        hf_cache = _hf_cache_root()
        logger.info(f"HuggingFace 缓存根目录: {hf_cache}")

        # 检查模型是否已缓存
        missing = []
        repos = [
            "funasr/paraformer-zh",
            "funasr/fsmn-vad",
        ]

        for repo_id in repos:
            if not _hf_repo_ready(repo_id):
                missing.append(repo_id)

        if not _hf_repo_ready("funasr/ct-punc"):
            missing.append("funasr/ct-punc")

        if not missing:
            logger.info("模型文件存在，开始初始化")
            init_result = self.initialize()
        else:
            logger.info(f"模型文件不存在或不完整：{', '.join(missing)}，跳过初始化")
            init_result = {
                "success": False,
                "error": "模型文件未下载，请先下载模型",
                "type": "models_not_downloaded",
                "engine": self.engine
            }
        print(json.dumps(init_result, ensure_ascii=False))
        sys.stdout.flush()

        while self.running:
            try:
                # 读取命令
                line = sys.stdin.readline()
                if not line:
                    break

                line = line.strip()
                if not line:
                    continue

                try:
                    command = json.loads(line)
                except json.JSONDecodeError:
                    result = {"success": False, "error": "无效的JSON命令"}
                    print(json.dumps(result, ensure_ascii=False))
                    sys.stdout.flush()
                    continue

                # 处理命令
                if command.get("action") == "transcribe":
                    audio_path = command.get("audio_path")
                    options = command.get("options", {})
                    result = self.transcribe_audio(audio_path, options)
                elif command.get("action") == "status":
                    result = self.check_status()
                elif command.get("action") == "stats":
                    result = {"success": True, "stats": self.get_performance_stats()}
                elif command.get("action") == "cleanup":
                    self._cleanup_memory()
                    result = {"success": True, "message": "内存清理完成"}
                elif command.get("action") == "exit":
                    result = {"success": True, "message": "服务器退出"}
                    print(json.dumps(result, ensure_ascii=False))
                    sys.stdout.flush()
                    break
                else:
                    result = {
                        "success": False,
                        "error": f"未知命令: {command.get('action')}",
                    }

                # 输出结果
                print(json.dumps(result, ensure_ascii=False))
                sys.stdout.flush()

            except KeyboardInterrupt:
                break
            except Exception as e:
                error_result = {
                    "success": False,
                    "error": str(e),
                    "traceback": traceback.format_exc(),
                }
                print(json.dumps(error_result, ensure_ascii=False))
                sys.stdout.flush()

        logger.info("FunASR服务器退出")

if __name__ == "__main__":
    server = FunASRServer()
    server.run()
