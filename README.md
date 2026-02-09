<div align="center">

# Light-Whisper 轻语

**本地离线中文语音转文字桌面应用**

[![Tauri](https://img.shields.io/badge/Tauri-2.0-blue?style=flat-square&logo=tauri)](https://tauri.app/)
[![React](https://img.shields.io/badge/React-19-61dafb?style=flat-square&logo=react)](https://react.dev/)
[![Rust](https://img.shields.io/badge/Rust-2021-orange?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![FunASR](https://img.shields.io/badge/FunASR-SenseVoice-green?style=flat-square)](https://github.com/modelscope/FunASR)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue?style=flat-square)](LICENSE)

<img src="assets/icon.png" alt="Light-Whisper Logo" width="120" />

*按下 F2，开口说话，松开即得文字*

</div>

---

<!-- TODO: 在此添加应用截图或 GIF 动图，例如：-->
<!-- ![应用截图](assets/screenshot.png) -->

## 功能特点

- **F2 一键转写** — 按住录音，松开自动转写，结果直接输入到当前活动窗口
- **完全离线** — 基于阿里 FunASR [SenseVoiceSmall](https://huggingface.co/FunAudioLLM/SenseVoiceSmall) 模型，数据不出本机
- **GPU 加速** — 自动检测 NVIDIA GPU 并启用 CUDA 加速，无 GPU 则回退 CPU
- **双输入模式** — 支持 SendInput 直接输入（不占用剪贴板）和剪贴板粘贴（兼容中文输入法）两种模式
- **悬浮窗设计** — 无边框透明窗口，始终置顶，最小化到系统托盘
- **开机自启动** — 可在设置中开启，开机后自动运行

---

## 环境要求

> **操作系统**：目前仅支持 Windows 10/11（x64）

| 工具 | 版本要求 | 用途 |
|------|---------|------|
| [Node.js](https://nodejs.org/) | >= 18 | 前端构建 |
| [pnpm](https://pnpm.io/) | >= 8 | 前端包管理 |
| [Rust](https://www.rust-lang.org/tools/install) | >= 1.75 | 后端编译 |
| [Python](https://www.python.org/downloads/) | 3.11.x | AI 推理服务 |
| [uv](https://docs.astral.sh/uv/) | >= 0.4 | Python 包管理 |
| [Visual Studio Build Tools](https://visualstudio.microsoft.com/zh-hans/visual-cpp-build-tools/) | 2019+ | Rust/C++ 编译依赖 |

**磁盘空间**：至少预留 **8 GB**（Python 依赖约 5 GB + ASR 模型约 1 GB）。

**GPU 加速（可选）**：如果你有 NVIDIA 显卡，不需要单独安装 CUDA Toolkit — PyTorch 已自带 CUDA 12.4 运行时。只需确保安装了最新的 [NVIDIA 显卡驱动](https://www.nvidia.cn/drivers/lookup/)。

---

## 快速开始

### 第 0 步：安装前置工具

如果你已经装好了上述所有工具，可以跳到第 1 步。否则按顺序安装：

<details>
<summary><b>0.1 安装 Visual Studio Build Tools</b></summary>

Rust 在 Windows 上编译需要 MSVC C++ 构建工具。

1. 下载 [Visual Studio Build Tools](https://visualstudio.microsoft.com/zh-hans/visual-cpp-build-tools/)
2. 运行安装程序，勾选 **"使用 C++ 的桌面开发"** 工作负载
3. 安装完成后重启电脑

</details>

<details>
<summary><b>0.2 安装 Rust</b></summary>

```powershell
# 在 PowerShell 中运行
winget install Rustlang.Rustup
# 或访问 https://rustup.rs/ 下载安装器
```

安装完成后验证：
```powershell
rustc --version   # 应显示 1.75+
```

</details>

<details>
<summary><b>0.3 安装 Node.js 和 pnpm</b></summary>

```powershell
# 安装 Node.js（推荐 LTS 版本）
winget install OpenJS.NodeJS.LTS

# 安装 pnpm
npm install -g pnpm
```

验证：
```powershell
node --version    # 应显示 v18+
pnpm --version    # 应显示 8+
```

</details>

<details>
<summary><b>0.4 安装 Python 3.11</b></summary>

> **重要**：请安装 **3.11.x** 版本（FunASR 对 Python 版本有兼容性要求）。

1. 前往 [Python 3.11 下载页](https://www.python.org/downloads/release/python-3119/) 下载安装器
2. 安装时 **勾选** "Add Python to PATH"

验证：
```powershell
python --version   # 应显示 Python 3.11.x
```

</details>

<details>
<summary><b>0.5 安装 uv</b></summary>

[uv](https://docs.astral.sh/uv/) 是一个极速的 Python 包管理器：

```powershell
# PowerShell
irm https://astral.sh/uv/install.ps1 | iex

# 或使用 pip
pip install uv
```

验证：
```powershell
uv --version
```

</details>

<details>
<summary><b>验证所有工具是否就绪</b></summary>

在 PowerShell 中运行以下命令，确认所有工具已正确安装：

```powershell
node --version      # >= 18
pnpm --version      # >= 8
rustc --version     # >= 1.75
python --version    # 3.11.x
uv --version        # >= 0.4
```

如果某个命令提示"不是内部或外部命令"，说明对应工具未安装或未添加到 PATH，请回到上方对应步骤重新安装。

</details>

---

### 第 1 步：克隆项目

```bash
git clone https://github.com/sypsyp97/light-whisper.git
cd light-whisper
```

### 第 2 步：安装前端依赖

```bash
pnpm install
```

### 第 3 步：安装 Python 依赖

```bash
uv sync
```

这一步会：
- 在项目根目录自动创建 `.venv` 虚拟环境
- 安装 PyTorch（含 CUDA 12.4）、FunASR、transformers 等依赖
- **耗时较长**（约 5-15 分钟，取决于网速），因为 PyTorch 包体较大

> **网络问题？** 如果 PyTorch 下载缓慢，详见下方 [常见问题](#常见问题)。

### 第 4 步：下载 ASR 模型

首次运行应用时会**自动下载**模型（约 1 GB），但推荐提前手动下载，避免启动时等待：

```bash
uv run python -c "from huggingface_hub import snapshot_download; snapshot_download('FunAudioLLM/SenseVoiceSmall'); snapshot_download('funasr/fsmn-vad')"
```

模型会缓存到 `~/.cache/huggingface/hub/`，下载一次后续启动不再重复下载。

> **模型说明**：
> - [**SenseVoiceSmall**](https://huggingface.co/FunAudioLLM/SenseVoiceSmall)（~936 MB）— ASR 语音识别主模型，支持中/英/日/韩/粤语，内置标点恢复（ITN）
> - [**fsmn-vad**](https://huggingface.co/funasr/fsmn-vad)（~1.7 MB）— 语音活动检测（VAD），负责切分有效语音片段，跳过静音

> **国内下载慢？** 可以设置 HuggingFace 镜像：
> ```bash
> set HF_ENDPOINT=https://hf-mirror.com
> ```
> 然后再执行上面的下载命令。

### 第 5 步：构建并运行

```bash
pnpm tauri build
```

首次构建需要编译所有 Rust 依赖，耗时约 **5-15 分钟**，请耐心等待。构建完成后：

1. 直接运行 `src-tauri/target/release/light-whisper.exe`
2. 或在 `src-tauri/target/release/bundle/nsis/` 目录下找到安装包，安装后使用
3. 应用窗口出现在屏幕中央（无边框悬浮窗）
4. 等待状态显示"就绪"（模型加载中时会显示进度）
5. **按住 F2 说话，松开后自动转写并输入到当前光标位置**

---

## 使用说明

| 操作 | 说明 |
|------|------|
| **按住 F2** | 开始录音，松开后自动转写 |
| **点击圆形按钮** | 手动开始/停止录音 |
| **系统托盘图标** | 右键菜单（显示/隐藏/退出），双击切换显示 |
| **齿轮图标** | 打开设置页面 |

### 设置选项

| 选项 | 说明 |
|------|------|
| **主题** | 浅色 / 深色 / 跟随系统 |
| **输入方式** | 直接输入（SendInput，不占用剪贴板）或 剪贴板粘贴（兼容中文输入法） |
| **开机自启动** | 开启后系统启动时自动运行 |

### 状态指示

| 状态 | 含义 |
|------|------|
| `GPU: NVIDIA RTX...` | GPU 加速已启用 |
| `CPU` | 使用 CPU 推理 |
| `模型加载中...` | 正在初始化模型（首次约 10-30 秒） |
| `下载中 45%` | 正在下载 ASR 模型 |

---

## 项目结构

```
light-whisper/
├── src/                        # 前端 (React + TypeScript)
│   ├── api/                    # Tauri API 封装层
│   │   ├── funasr.ts           #   FunASR 服务调用
│   │   ├── clipboard.ts        #   剪贴板 / 文本输入
│   │   ├── hotkey.ts           #   快捷键注册
│   │   ├── window.ts           #   窗口控制
│   │   └── autostart.ts        #   开机自启动
│   ├── pages/                  # 页面组件
│   │   ├── MainPage.tsx        #   主界面（录音+转写）
│   │   └── SettingsPage.tsx    #   设置页面
│   ├── components/             # 通用组件
│   │   └── TitleBar.tsx        #   标题栏（窗口拖动、操作按钮）
│   ├── hooks/                  # React Hooks
│   │   ├── useRecording.ts     #   WebAudio 录音逻辑
│   │   ├── useModelStatus.ts   #   模型状态事件监听
│   │   ├── useHotkey.ts        #   F2 快捷键处理
│   │   ├── useTheme.ts         #   主题切换
│   │   └── useWindowDrag.ts    #   无边框窗口拖动
│   ├── contexts/
│   │   └── RecordingContext.tsx #   全局录音状态管理
│   ├── types/
│   │   └── index.ts            #   TypeScript 类型定义
│   └── main.tsx                # React 入口
│
├── src-tauri/                  # 后端 (Rust + Tauri 2)
│   ├── src/
│   │   ├── lib.rs              #   应用入口、插件注册、托盘设置
│   │   ├── commands/           #   Tauri 命令（前端可调用）
│   │   │   ├── funasr.rs       #     启动/停止/转写/状态查询
│   │   │   ├── clipboard.rs    #     复制/输入（SendInput / 剪贴板粘贴）
│   │   │   ├── hotkey.rs       #     快捷键注册
│   │   │   └── window.rs       #     窗口控制
│   │   ├── services/
│   │   │   └── funasr_service.rs  # Python 子进程管理、JSON IPC
│   │   ├── state/
│   │   │   └── app_state.rs    #   全局应用状态
│   │   └── utils/
│   │       ├── error.rs        #   错误类型定义
│   │       └── paths.rs        #   路径工具
│   ├── resources/              # 嵌入到应用中的 Python 脚本
│   │   ├── funasr_server.py    #   FunASR 推理服务（stdin/stdout IPC）
│   │   ├── download_models.py  #   模型下载脚本
│   │   └── hf_cache_utils.py   #   HuggingFace 缓存检测工具
│   ├── Cargo.toml
│   └── tauri.conf.json
│
├── package.json                # 前端依赖
├── pyproject.toml              # Python 依赖（含 CUDA 12.4 PyTorch）
├── vite.config.ts              # Vite 构建配置
└── .python-version             # Python 版本约束 (3.11)
```

### 架构通信流程

```
┌──────────────┐     Tauri IPC      ┌──────────────┐   stdin/stdout   ┌──────────────┐
│  React 前端  │ ◄──── invoke() ───►│  Rust 后端   │ ◄──── JSON ────► │  Python 服务  │
│  (TypeScript) │ ◄──── emit() ─────│  (Tauri 2)   │                  │  (FunASR)    │
└──────────────┘                    └──────────────┘                  └──────────────┘
```

1. **前端 → Rust**：通过 `invoke()` 调用 Tauri 命令
2. **Rust → Python**：通过子进程的 stdin 发送 JSON 命令，从 stdout 读取 JSON 响应
3. **Rust → 前端**：通过 `emit()` 广播状态事件

---

## 常见问题

<details>
<summary><b>网络问题：PyTorch 或模型下载很慢</b></summary>

**PyTorch 下载慢**：`uv sync` 会从 `download.pytorch.org` 下载 PyTorch CUDA 版（约 2.5 GB）。由于项目指定了 PyTorch 官方 CUDA 源，国内镜像无法加速此步骤。建议：

- 使用稳定的网络环境（科学上网或校园网）
- 如果多次断线，`uv sync` 支持断点续传，重新执行即可
- 其他 Python 依赖会从默认 PyPI 源下载，可通过清华镜像加速：
```powershell
$env:UV_INDEX_URL = "https://mirrors.tuna.tsinghua.edu.cn/pypi/web/simple"
uv sync
```

**模型下载慢**：ASR 模型从 HuggingFace Hub 下载，国内可设置镜像：

```powershell
$env:HF_ENDPOINT = "https://hf-mirror.com"
# 然后重新启动应用或手动下载模型
```

</details>

<details>
<summary><b>Python 找不到或版本不对</b></summary>

应用启动时，Rust 后端按以下顺序查找 Python：
1. **项目根目录的 `.venv/Scripts/python.exe`**（优先）
2. 系统 PATH 中的 `python.exe` / `python3.exe`

**确保 `uv sync` 在项目根目录执行过**，它会自动创建 `.venv` 目录。可以验证：

```powershell
.venv\Scripts\python.exe --version   # 应显示 Python 3.11.x
```

如果使用系统 Python，请确保版本 >= 3.11 且 FunASR 相关依赖已安装。

</details>

<details>
<summary><b>GPU 未被检测到</b></summary>

1. 确认安装了最新的 [NVIDIA 显卡驱动](https://www.nvidia.cn/drivers/lookup/)
2. 确认 PyTorch 是 CUDA 版本：
   ```powershell
   .venv\Scripts\python.exe -c "import torch; print(torch.cuda.is_available())"
   ```
   应输出 `True`。如果输出 `False`：
   - 检查驱动版本是否支持 CUDA 12.4（驱动版本 >= 525.60）
   - 确认 `uv sync` 安装的是 CUDA 版 PyTorch（`pyproject.toml` 中已配置）

如果不需要 GPU 加速，应用会自动回退到 CPU 模式，无需额外操作。

</details>

<details>
<summary><b>F2 快捷键没反应或被占用</b></summary>

F2 是全局快捷键，如果被其他程序占用（如某些游戏或工具），可能无法注册。检查是否有其他程序也在使用 F2。

当前 F2 按键是硬编码的，如需修改，编辑以下文件：
- `src/hooks/useHotkey.ts` — 前端监听
- `src-tauri/src/commands/hotkey.rs` — 后端注册

</details>

<details>
<summary><b>转写结果输入到光标位置时部分字符变成句号或乱码</b></summary>

这是因为默认的"直接输入"模式使用 Win32 `SendInput` API 以 `KEYEVENTF_UNICODE` 模式逐字符模拟键盘输入，**当系统开启中文输入法（IME）时，IME 可能拦截并错误处理这些合成的 Unicode 键盘事件**，导致某些中文字符（如"我"、"你"）被转换为其他字符（如"。"）。

**解决方法**（任选其一）：
1. **推荐**：打开设置页面，将输入方式切换为**「剪贴板粘贴」**模式，该模式通过剪贴板 + Ctrl+V 粘贴，完全兼容中文输入法
2. 在使用语音转写时，将输入法切换到**英文模式**（按 `Shift` 或 `Ctrl+Space` 切换）

</details>

<details>
<summary><b>首次构建很慢</b></summary>

首次 `pnpm tauri build` 需要编译所有 Rust 依赖（约 5-15 分钟），这是正常的。后续构建只会增量编译改动部分，速度很快。

</details>

<details>
<summary><b>应用日志在哪？</b></summary>

- **Python 服务日志**：`%APPDATA%\light-whisper\logs\funasr_server.log`
- **Rust/Tauri 日志**：开发模式下输出到控制台

</details>

---

## 开发命令速查

```bash
pnpm tauri build        # 构建 Windows 安装包
pnpm build              # 仅构建前端
uv sync                 # 同步 Python 依赖
uv add <package>        # 添加 Python 依赖
cd src-tauri && cargo check   # Rust 类型检查
cd src-tauri && cargo fmt     # Rust 代码格式化
```

---

## 致谢

本项目基于 [**ququ**](https://github.com/yan5xu/ququ) 修改开发，感谢原作者的贡献。

- [FunASR](https://github.com/modelscope/FunASR) — 阿里达摩院开源语音识别
- [SenseVoiceSmall](https://huggingface.co/FunAudioLLM/SenseVoiceSmall) — 多语言语音识别模型（中/英/日/韩/粤）
- [Tauri](https://tauri.app/) — 现代化桌面应用框架
- [React](https://react.dev/) — 用户界面库

## 许可证

[Apache License 2.0](LICENSE)
