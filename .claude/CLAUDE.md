Whenever you fix an issue, be sure to update the CLAUDE.md file with the changes you made so you can remember them later.

Always remove legacy code that is no longer needed after making changes.

## Changes Log

### HuggingFace 模型下载/加载优化 (2026-02-04)
- `download_models.py`: 添加 `_is_repo_cached()` 检查，已缓存模型直接跳过 `snapshot_download`，避免 Windows 上缓慢的完整性校验（从 60+ 秒降到 0.3 秒）
- `funasr_server.py`: 在 `_setup_runtime_environment()` 中设置 `HF_HUB_OFFLINE=1`，防止 FunASR 的 `AutoModel` 内部调用 `snapshot_download` 导致 "Fetching N files" 卡住
- `funasr_server.py`: 加载 Fun-ASR Nano 前显式导入本地 remote code，确保 `FunASRNano` 注册，避免 "is not registered" 初始化失败
- `download_models.py`/`funasr_service.rs`: 缓存检测兼容 refs 任意分支与 snapshots 目录，降低误判导致的重复下载或初始化跳过
- `funasr_nano_model.py`: CTC 初始化失败时自动降级禁用（避免 `SenseVoiceTokenizer` 缺失导致模型加载失败）
- `funasr_server.py`: Fun-ASR Nano 的 VAD 子模型强制走 HF hub，避免隐式走 ModelScope

### FunASR IPC 与 AI 请求稳定性改进 (2026-02-04)
- `funasr_service.rs`: 增加 JSON 响应读取的容错逻辑（跳过非 JSON 行，统一超时控制），提升 IPC 稳定性
- `funasr_service.rs`: 统一设置 Python 进程 UTF-8 环境变量并传递 `QUQU_DATA_DIR`，避免 Windows 下编码问题并规范日志目录
- `funasr_service.rs`: 初始化失败时不再错误广播 "ready"，改为按真实状态发送事件
- `app_state.rs`: 移除未使用的 `models_initialized` 状态字段与相关方法，简化状态管理
- `ai_service.rs`/`ai.rs`: 复用 HTTP 客户端并集中处理 AIMode 解析，降低重复与潜在超时问题
- `funasr_service.rs`: 明确初始化响应类型，修复 Rust 类型推断失败导致的编译错误
- `funasr_service.rs`: 避免重复移动 `response.error`，复用错误消息并恢复模型加载状态推断
- `funasr_server.py`: Fun-ASR Nano 显式指定 `batch_size=1`，修复 “batch decoding is not implemented”
### FunASR 官方用法对齐与加载优化 (2026-02-04)
- `funasr_server.py`: Paraformer 改为官方推荐的 AutoModel 集成 VAD+PUNC 流程，移除手动 VAD/PUNC
- `funasr_server.py`: Fun-ASR Nano 采用官方参数（input 列表 + cache + hotwords + language/itn），并通过 `ctc_decoder=None` 显式禁用 CTC 解码
- `funasr_server.py`: 精简 paraformer 初始化流程，避免重复加载多套模型
- `funasr_nano_model.py`: 与 FunAudioLLM/Fun-ASR 官方 `model.py` 同步

### FunASR 引擎收敛与 HuggingFace 固定 (2026-02-04)
- 移除 Fun-ASR Nano 全链路支持（后端引擎参数、前端设置项、资源文件），仅保留 Paraformer
- `funasr_server.py`: 仅加载 Paraformer，并为 VAD/PUNC 传入 `hub="hf"`，阻止 ModelScope 触发重复下载
- `download_models.py`: 简化为仅下载 Paraformer 相关模型，不再接受引擎参数
- 前端设置页移除引擎切换 UI，模型状态与下载逻辑改为单引擎

### Paraformer 标点模型对齐 (2026-02-04)
- `funasr_server.py`: ASR/VAD 固定 `v2.0.4`，PUNC 优先用 ct-punc-c（HF iic 仓库，不强制 revision），失败回退 `funasr/ct-punc`
- `download_models.py`: 优先下载 `iic/punc_ct-transformer_zh-cn-common-vocab272727-pytorch`（ct-punc-c），失败自动回退 `funasr/ct-punc`
- `funasr_service.rs`: 标点模型缓存检查支持 ct-punc-c（HF 对应 iic 仓库）或 ct-punc

### 标点异常回退与原文保护 (2026-02-04)
- `funasr_server.py`: `generate` 返回 `raw_text` 并在出现 `<unk>`、短文本或标点密度过高时回退原文，避免“每字一标点”

### FunASR 官方标点策略对齐 (2026-02-04)
- `funasr_server.py`: 按官方用法改回 `punc_model="ct-punc"`，移除 ct-punc-c 及自定义标点稀释/回退逻辑，短文本不再强制跳过标点
- `funasr_server.py`: `generate` 去除 `return_raw_text`，仅使用模型输出文本
- `download_models.py`: 标点模型仅下载 `funasr/ct-punc`（按 v2.0.4，失败自动回退到默认分支）
- `funasr_service.rs`: 标点缓存检测与日志同步为 `funasr/ct-punc`

### 清理未使用资源 (2026-02-04)
- 移除 `src-tauri/resources/tools` 中未被引用的 Python 工具模块，并从 `tauri.conf.json` 的 bundle resources 中删除对应路径
- `.gitignore` 增加 Rust/Tauri 构建产物忽略（`target/`, `src-tauri/target/`, `src-tauri/gen/`）

### 深度瘦身与仓库重置 (2026-02-04)
- 删除 Tauri 生成物与缓存：`src-tauri/gen/`、`src-tauri/target/`、`dist/`、`node_modules/`、`src-tauri/resources/__pycache__/` 与空的 `resources/tools`
- 断开旧仓库：移除 `.git` 并重新执行 `git init` 初始化本地新仓库
- `.gitignore` 补充忽略 `.venv/`

### 移除 AI 与数据库 (2026-02-04)
- 前端：删除 AI/设置/数据库 API 与 Hook，录音流程仅保留 FunASR 转写与自动粘贴
- UI：设置页移除 AI 配置与保存按钮，主界面移除 AI 润色展示
- 后端：删除 AI/数据库服务与命令，移除 SQL 插件与权限声明
- 配置：`Cargo.toml` 去掉 `tauri-plugin-sql`、`reqwest` 等无用依赖；`.env.example` 移除
