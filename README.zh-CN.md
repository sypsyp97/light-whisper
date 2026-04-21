<div align="center">

# Light-Whisper 轻语

**macOS 分支 · Apple Silicon 在线语音转文字**

简体中文 | [English](README.md)

[![Tauri 2](https://img.shields.io/badge/Tauri-2.0-24c8db?style=for-the-badge&logo=tauri)](https://tauri.app/)
[![React 19](https://img.shields.io/badge/React-19-61dafb?style=for-the-badge&logo=react)](https://react.dev/)
[![Rust](https://img.shields.io/badge/Rust-2021-f74c00?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
[![License: CC BY-NC 4.0](https://img.shields.io/badge/License-CC%20BY--NC%204.0-lightgrey?style=for-the-badge)](LICENSE)

<br>

<img src="assets/icon.png" alt="Light-Whisper" width="128" />

<br>

**按下热键，开口说话，松开即得文字。**

当前分支：`codex/apple-silicon-mlx-asr`

</div>

<br>

## 安装

> [!IMPORTANT]
> 这个分支基于最新 `main` 重建，但保留了 macOS 专属的输入、热键和权限流。
> 当前分支的 ASR 仅保留在线引擎：
> - `Alibaba DashScope`
> - `GLM-ASR`
>
> 历史本地引擎配置（`local`、`sensevoice`、`whisper`）会自动迁移到 `alibaba-asr`。

### 方式一：安装包（推荐）

这个分支建议直接从源码构建 macOS app / dmg。打包产物不会再内置任何本地 ASR 运行时或模型负载。

### 方式二：从源码构建

参见下方[快速开始](#快速开始)。

## 功能亮点

<table>
<tr>
<td width="50%">

**一键听写**<br>
按住 <kbd>F2</kbd>（可自定义）录音，松开自动转写并输入到当前活动窗口。

**两个在线 ASR 引擎**<br>
Alibaba DashScope 或 GLM-ASR。没有本地模型下载，也没有模型目录管理。

**AI 润色**<br>
可选 LLM 后处理：修正同音字、标点、口头禅；自动检测前台应用适配语气。内置 OpenAI / DeepSeek / Cerebras / SiliconFlow 预设，支持 OpenAI 兼容和 Anthropic API 格式。

**自适应学习**<br>
只学习结构化纠错和术语；手动删除的脏热词会进入黑名单，不再自动长回来。

**字幕悬浮窗**<br>
透明浮窗实时显示听写状态和助手结果。

</td>
<td width="50%">

**语音助手**<br>
独立热键触发悬浮答案卡。可自动读取选中文本、前台应用和全屏截图作为上下文，自动检测模型图片支持。内置 Web 搜索（Exa / Tavily）获取实时信息。

**编辑选中文本**<br>
选中文字后按热键，说出指令（"翻译成英文"、"改成正式语气"），原地改写。

**实时翻译**<br>
设置目标语言（8 种预设 + 自定义），转写结果自动翻译后输出。

**更多**<br>
按住说话 / 切换模式 · 输入队列 · 中英文界面 · 自动更新检查

</td>
</tr>
</table>

## Light-Whisper vs Typeless

| 功能 | Light-Whisper | Typeless |
|:-----|:---:|:---:|
| **价格** | 免费开源 | 免费版 (4k 词/周)；$12–30/月 |
| **隐私** | 当前分支使用在线 ASR | 云端处理，零数据留存 |
| **开源** | ✅ | ❌ |
| **平台** | macOS 分支 | Windows、Mac、iOS、Android、Web |
| **ASR 引擎** | 2 种在线引擎 | 云端专有 |
| **语言数** | 5–99+（取决于引擎） | 100+ |
| **AI 润色** | 多 LLM 后端，自带 key | 内置 |
| **屏幕感知助手 + Web 搜索** | ✅ | ❌ |
| **字幕悬浮窗** | ✅ | ❌ |

## 从源码构建 — 环境要求

> [!IMPORTANT]
> **macOS 14+ / Apple Silicon。** 以下要求仅针对这个分支。

| 工具 | 版本 | 用途 |
|:-----|:-----|:-----|
| [Rust](https://www.rust-lang.org/tools/install) | >= 1.75 | 后端编译 |
| [Node.js](https://nodejs.org/) | >= 18 | 前端构建 |
| [pnpm](https://pnpm.io/) | >= 8 | 前端包管理 |
| Xcode Command Line Tools | 最新版 | macOS 工具链 |

> [!TIP]
> 这个分支是在线 ASR 版。启动后在设置页填入 Alibaba DashScope 或 GLM-ASR API Key 即可使用。

<details>
<summary><b>逐步安装指引</b></summary>

```powershell
# 1. Visual Studio Build Tools — 下载安装器，勾选「使用 C++ 的桌面开发」
# 2. Rust
winget install Rustlang.Rustup
# 3. Node.js + pnpm
winget install OpenJS.NodeJS.LTS
npm install -g pnpm
# 4. uv
winget install astral-sh.uv
```

验证：

```powershell
rustc --version     # >= 1.75
node --version      # >= 18
pnpm --version      # >= 8
uv --version        # >= 0.4
```

> [!TIP]
> Python 由 `uv` 自动管理，无需手动安装。

</details>

## 快速开始

```bash
git clone https://github.com/sypsyp97/light-whisper.git
cd light-whisper

pnpm install          # 前端依赖
cargo check --manifest-path src-tauri/Cargo.toml
```

### 构建运行

```bash
pnpm tauri build
```

`.app` 位于 `src-tauri/target/release/bundle/macos/`，`.dmg` 位于 `src-tauri/target/release/bundle/dmg/`。

## macOS 权限

- 麦克风：录音
- 辅助功能 + 自动化（`System Events`）：把转写结果粘贴到别的应用
- 屏幕录制：屏幕感知助手

如果已经授权但仍然无法粘贴或截屏，先彻底退出 app 再重新打开。

## 架构

```
┌──────────────┐                ┌──────────────┐
│   React 前端  │  Tauri IPC     │   Rust 核心   │
│  TypeScript  │◄──invoke/emit─►│  (Tauri 2)   │
└──────────────┘                └──────┬───────┘
                                       ├─── HTTP ──► GLM-ASR API
                                       ├─── HTTP ──► Alibaba DashScope ASR
                                       ├─── HTTP ──► LLM API（AI 润色 / 助手 / 翻译）
                                       ├─── HTTP ──► Web 搜索（Exa / Tavily）→ 助手上下文
                                       ├─── 屏幕捕获 ──► 全屏截图 → 助手上下文
                                       └─── 用户画像 ──► 热词 + 黑名单 → ASR + LLM prompt
```

<details>
<summary><b>关键路径</b></summary>

| 层 | 路径 |
|:---|:-----|
| **前端** | `src/pages/`, `src/components/`, `src/hooks/`, `src/contexts/`, `src/lib/`, `src/i18n/`, `src/styles/` |
| **Rust 命令** | `src-tauri/src/commands/` — audio, assistant, clipboard, funasr, hotkey, ai_polish, profile, updater, window |
| **Rust 服务** | `src-tauri/src/services/` — funasr_service, glm_asr_service, audio_service, assistant_service, ai_polish_service, llm_client, llm_provider, profile_service, screen_capture_service, web_search_service, download_service |
| **状态** | `src-tauri/src/state/` — app_state, user_profile |

</details>

## 开发命令

```bash
pnpm tauri dev          # 开发模式（热更新）
pnpm tauri build        # 生产构建 + mac app/dmg
pnpm build              # 仅构建前端
cd src-tauri && cargo check   # Rust 类型检查
```

## 常见问题

<details>
<summary><b>在线 ASR 很慢或请求失败</b></summary>

- 先确认设置里已经填好当前供应商的 API Key。
- 如果使用阿里 DashScope，确认区域选择和 API Key 所属控制台一致。
- 如果请求超时，换一个网络环境或 VPN 后再试。

</details>

<details>
<summary><b>依赖权限的功能不工作</b></summary>

- 如果热键、选中文本抓取、自动粘贴无效，请授予“辅助功能”权限。
- 如果启用了助手/润色的屏幕感知，请授予“屏幕录制”权限。
- 如果应用需要控制其他 App 执行粘贴，请授予“自动化”权限。

</details>

<details>
<summary><b>输入到应用时部分字符变成句号</b></summary>

中文输入法拦截了 `SendInput` Unicode 事件。解决：在设置中切换为**剪贴板粘贴**模式，或将输入法切到英文模式。

</details>

<details>
<summary><b>中文显示乱码怎么办？</b></summary>

开启 Windows 的 UTF-8 支持来解决编码问题：控制面板 → 区域（或者叫“时钟和区域”）→ 管理 → 更改系统区域设置 → 勾选底部的 `Beta 版: 使用 Unicode UTF-8 提供全球语言支持` → 确定 → 重启电脑。

</details>

<details>
<summary><b>热键没反应</b></summary>

默认听写热键是 F2。如果被其他软件占用，可在设置中更换（如 `Ctrl+Win+R`）。

</details>

<details>
<summary><b>日志位置</b></summary>

这个分支不再启动本地 Python ASR 服务，因此 `funasr_server.log` / `whisper_server.log` 不再适用。
需要排查时，请查看 Tauri 生成的应用日志，以及 `~/Library/Application Support/com.light-whisper.desktop/` 下的应用数据目录。

</details>

## 致谢

- [Alibaba DashScope](https://www.alibabacloud.com/help/en/model-studio/dashscope-api-reference/) — Qwen ASR / Omni 在线语音识别
- [GLM-ASR](https://bigmodel.cn/) — 智谱 AI
- [Tauri](https://tauri.app/) / [React](https://react.dev/)

## 许可证

[知识共享署名-非商业性使用 4.0 国际许可协议 (CC BY-NC 4.0)](LICENSE)
