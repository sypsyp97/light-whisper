<div align="center">

# Light-Whisper 轻语

**本地与云端语音转文字 · Windows 桌面应用**

简体中文 | [English](README.md)

[![Tauri 2](https://img.shields.io/badge/Tauri-2.0-24c8db?style=for-the-badge&logo=tauri)](https://tauri.app/)
[![React 19](https://img.shields.io/badge/React-19-61dafb?style=for-the-badge&logo=react)](https://react.dev/)
[![Rust](https://img.shields.io/badge/Rust-2021-f74c00?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
[![License: GPL-3.0-only](https://img.shields.io/badge/License-GPL--3.0--only-2f6f9f?style=for-the-badge)](LICENSE)

<br>

<img src="assets/readme-hero.png" alt="Light-Whisper 深色听写界面" width="100%" />

<br>

**按下热键，开口说话，松开后文字自动输入到当前应用。**

[下载安装包](https://github.com/sypsyp97/light-whisper/releases/latest)

</div>

## 功能

- **听写与翻译**：按住说话或点击切换录音，将文字输入当前应用。
- **本地与云端识别**：自由选择引擎；本地识别支持字幕悬浮窗。
- **AI 润色与 Jev**：按要求改写，可选自动判断是否需要润色，无需修改时保留原文。
- **语音助手与划词工具**：支持提问、改写、翻译、解释和搜索，可选加入应用、选区或截图上下文。
- **个性化**：热词、纠错学习，以及按应用设置指令、润色和翻译规则。
- **可选本地历史**：搜索、导出、删除和重新润色；保存音频后可重新识别。

## 快速开始

1. 从 [Releases](https://github.com/sypsyp97/light-whisper/releases/latest) 下载 Windows 10/11 x64 的 `*_x64-setup.exe`，无需另装 Python 或编译工具。
2. 打开设置，选择麦克风和识别引擎，下载本地模型，或配置云端 API Key 与区域。
3. 将焦点放到目标应用，按住 `F2` 说话，松开后输出文字。热键和录音模式均可修改。
4. 如需 AI 润色或语音助手，配置服务商、模型及 API Key，或使用受支持的账号登录。基础本地听写不需要 LLM 账号。

使用 **AI 润色 → Jev 智能跳过润色** 时，另选 TypeSafe（官方）、OpenRouter 或 Vercel，填写对应的独立 API Key。Jev 默认关闭，出错或超时会继续正常润色；翻译、助手/编辑和手动重新润色不经过该判断。

## 识别引擎

| 引擎 | 运行方式 | 要求 |
|:--|:--|:--|
| **Confucius4-R2T2 Q8** | Windows 原生 CUDA / CPU | 独立下载约 2.48 GB 模型；原生流式，支持语言与主题提示；无需 WSL |
| **Qwen3-ASR 0.6B Q8** | CUDA / Vulkan / CPU | 独立下载约 850 MB 模型；多语言听写 |
| **GLM-ASR** | 云端 | API Key 与区域；仅返回最终结果 |
| **阿里 DashScope** | 云端 | API Key、区域与模型；仅返回最终结果 |

本地模型下载后会缓存，FireRedVAD 已内置；GPU 加速不是必需条件。R2T2 字幕由不回退的已确认文本和可修订的预览组成；原生流式不代表在所有场景下延迟都更低。

**升级到 1.6：**原有 Qwen 1.7B 选项会迁移到 R2T2，需要另行下载模型。Qwen 0.6B 和已有模型缓存保留。R2T2 使用独立的[模型许可证](src-tauri/resources/R2T2-MODEL-LICENSE.txt)。

## 数据使用

本地识别在电脑上处理音频；云端识别会将音频发送给所选服务商。启用 AI 润色或 Jev 后，文本和处理要求会发送给对应服务；可选的截图、选区上下文及联网搜索也可能向配置的服务发送数据。Jev 密钥按服务商分别存入系统凭据库。历史记录与音频保存均为可选的本地设置。

## 开发与文档

- [构建、测试与排障](docs/development.md)
- [发版流程](docs/releasing.md)
- [R2T2 实现与实测限制](docs/r2t2-native.md)

## 许可证与致谢

Light-Whisper 是采用 [GPL-3.0-only](LICENSE) 的开源软件。第三方组件与模型继续适用各自条款，详见 [NOTICE](NOTICE) 和 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

基于 Tauri 和 React 构建，使用 Qwen3-ASR / transcribe.cpp、Confucius4-R2T2 / audio.cpp 和 FireRedVAD。
