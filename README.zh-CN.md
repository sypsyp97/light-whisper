<p align="center">
  <img src="assets/readme-banner.svg" alt="Light-Whisper 轻语 — 在你正在使用的应用中，把声音变成文字" width="100%" />
</p>

<p align="center">
  <strong>本地与云端语音转文字 · Windows 桌面应用</strong><br />
  按住热键说话，松开后，文字自动输入当前应用。
</p>

<p align="center">
  <a href="https://github.com/sypsyp97/light-whisper/releases/latest"><strong>下载 Windows 安装包</strong></a>
  &nbsp; · &nbsp; <a href="#快速开始">快速开始</a>
  &nbsp; · &nbsp; <a href="README.md">English</a>
</p>

---

## 快速开始

1. **安装**。从 [Releases](https://github.com/sypsyp97/light-whisper/releases/latest) 下载 Windows 10/11 x64 的 `*_x64-setup.exe`，然后运行安装包。识别运行时已内置，无需另装 Python 或编译工具。
2. **选择引擎**。打开**设置**，选择麦克风和识别引擎。如果缺少所选本地模型，在主界面点击**开始下载**。云端识别需要 API Key 和区域配置。
3. **开始说话**。将文字光标放在目标应用中。按住 <kbd>F2</kbd> 说话，松开后插入文字。

你可以在**设置**中修改热键，也可以切换为点击录音模式。

本地听写不需要 AI 账号。如需 AI 润色、翻译或助手，在**设置**中配置服务商和模型，并使用 API Key 或受支持的账号登录。

## 能做什么

| 功能 | 日常用法 |
|:--|:--|
| **听写与翻译** | 用热键把文字输入当前应用，也可翻译为指定语言。 |
| **实时字幕** | 在悬浮窗查看本地识别结果，包括已确认文本和可变的预览。 |
| **AI 润色** | 按你的要求修改文本；自动档可在无需修改时保留原文。 |
| **语音与划词助手** | 提问、编辑、翻译、解释和搜索，可选加入应用、选中文本或截图上下文。 |
| **个性化** | 热词、学习到的纠错，以及各应用独立的指令和处理规则。 |
| **本地历史** | 搜索、导出、删除和重新润色；开启音频保存后也可重新识别。 |
| **命令行接口** | 控制桌面引擎、诊断本地运行时、转写文件，以及供编码 agent 使用的 JSON 会话。 |

## 识别引擎

| 引擎 | 处理方式 | 配置 |
|:--|:--|:--|
| **Qwen3-ASR 0.6B Q8** | 本地 · CUDA / CPU | 下载约 1.15 GB 模型；多语言听写。 |
| **Confucius4-R2T2 Q8** | 本地 · CUDA / CPU | 下载约 2.48 GB 模型；原生流式，支持语言与主题提示。 |
| **GLM-ASR** | 云端 · 最终结果 | API Key 与区域。 |
| **阿里 DashScope** | 云端 · 最终结果 | API Key、区域与模型。 |

两个本地引擎均在 Windows 原生运行，无需 WSL。模型单独下载并缓存在本地。应用已内置 FireRedVAD，GPU 加速为可选功能。Confucius4-R2T2 使用独立的[模型许可证](src-tauri/resources/R2T2-MODEL-LICENSE.txt)。

R2T2 的已确认字幕保持不变，预览文本仍可变化。流式识别不一定降低延迟。详见[实测表现与限制](docs/r2t2-native.md)。

本地引擎的设置中可开启**音频处理（RE-USE）**，使用 NVIDIA Real-time RE-USE 在识别前处理音频。此功能默认关闭，首次使用时下载约 54 MB 模型。其许可证仅允许非商业研究和教育用途。详见 [RE-USE 条款与实测](docs/r2t2-native.md#optional-re-use-audio-processing)。

## AI 服务

润色与助手可以共用模型，也可以分别设置模型。预设包含 OpenAI、xAI、DeepSeek、Cerebras 和 SiliconFlow，也可添加 OpenAI 兼容或 Anthropic 格式的自定义服务。应用支持 ChatGPT/Codex 和 Grok Build 账号登录。账号权限和用量限制仍然适用。

AI 润色、屏幕感知和助手联网搜索均提供**关闭／开启／自动**模式。自动模式使用决策模型判断何时启用对应功能。请在**设置 → 决策模型**中配置。

搜索选项包括受支持服务的内置搜索、Bing、Exa、Tavily 和 Google grounding。部分选项需要 API Key。应用会提示搜索失败，并可在没有网页来源的情况下继续回答。

连接配置和服务默认值见 [API 文档](docs/api-defaults.md)。

## 数据去向

| 设置 | 数据如何处理 |
|:--|:--|
| **本地识别** | 电脑在本地处理音频；下载模型需要联网。 |
| **云端识别** | 应用将音频发送给所选识别服务商。 |
| **润色、助手与决策模型** | 使用时将文本和指令发送给对应服务；上下文和搜索也可能发送图片、选中文本或查询。 |
| **历史与音频保存** | 两者默认关闭，开启后保存在本地；保存音频需先开启历史。 |

完整的网络请求、本地存储和第三方服务说明见[隐私政策](docs/privacy.md)。

## 命令行接口

安装后，新开一个终端窗口，直接运行：

```powershell
light-whisper status
light-whisper transcribe "sample.wav"
```

安装程序会把 `light-whisper` 加入当前用户的 PATH。它支持读取和控制桌面引擎，也支持单独运行本地引擎进行排障。
命令通过 JSON 和退出码返回结果，适合编码 agent 使用。完整命令和持续会话示例见 [CLI 指南](docs/cli.md)。

## 开发文档

[构建与排障](docs/development.md) · [发版流程](docs/releasing.md) · [R2T2 后端](docs/r2t2-native.md) · [形式验证](formal/README.md) · [更新记录](CHANGELOG.md)

应用使用 Tauri、React 和 Rust。Rust 引擎通过 audio.cpp 运行两个本地识别模型，
使用 ONNX Runtime 检测语音活动。独立 CLI 也是原生 Rust 可执行文件。

Light-Whisper 是采用 [GPL-3.0-only](LICENSE) 许可证的开源软件。第三方代码与模型继续适用各自条款，详见 [NOTICE](NOTICE) 和 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
