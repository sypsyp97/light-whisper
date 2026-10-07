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

1. **安装**。从 [Releases](https://github.com/sypsyp97/light-whisper/releases/latest) 下载 Windows 10/11 x64 的 `*_x64-setup.exe`。识别运行时已内置，无需另装 Python 或编译工具。
2. **选择引擎**。打开设置，选择麦克风，然后下载本地模型，或配置云端 API Key 与区域。
3. **开始说话**。将焦点放到目标应用，按住 <kbd>F2</kbd> 说话，松开后输出文字。热键可以修改，也可以改用点击切换录音。

基础本地听写不需要 LLM 账号。如需润色、翻译或语音助手，再配置服务商、模型与 API Key，或使用受支持的账号登录。

## 能做什么

| 功能 | 日常用法 |
|:--|:--|
| **听写与翻译** | 用热键把文字输入当前应用，也可翻译为指定语言。 |
| **实时字幕** | 在悬浮窗查看本地识别结果，区分已确认文本和可修订的预览。 |
| **AI 润色** | 按自定义要求改写；自动档可在无需润色时保留原文。 |
| **语音与划词助手** | 提问、改写、翻译、解释和搜索，可选加入应用、选区或截图上下文。 |
| **个性化** | 添加热词、学习纠错，按应用设置指令、翻译与处理规则。 |
| **本地历史** | 主动开启后可搜索、导出、删除和重新润色；保存音频后可重新识别。 |

## 识别引擎

| 引擎 | 处理方式 | 配置 |
|:--|:--|:--|
| **Qwen3-ASR 0.6B Q8** | 本地 · CUDA / Vulkan / CPU | 下载约 850 MB 模型；多语言听写。 |
| **Confucius4-R2T2 Q8** | 本地 · CUDA / CPU | 下载约 2.48 GB 模型；原生流式，支持语言与主题提示。 |
| **GLM-ASR** | 云端 · 最终结果 | API Key 与区域。 |
| **阿里 DashScope** | 云端 · 最终结果 | API Key、区域与模型。 |

两个本地引擎均在 Windows 原生运行，无需 WSL。模型单独下载并缓存，FireRedVAD 已内置；GPU 加速不是必需条件。R2T2 的已确认字幕不会回退，预览仍可修订。流式并不保证在所有场景下延迟都更低，详见[实测表现与限制](docs/r2t2-native.md)。

<details>
<summary><strong>AI 服务商、自动判断与联网搜索</strong></summary>

润色与助手可以共用模型，也可以分别设置服务商和模型。预设包含 OpenAI、xAI、DeepSeek、Cerebras 和 SiliconFlow；自定义服务支持 OpenAI 兼容与 Anthropic 格式。应用支持 ChatGPT/Codex 和 Grok Build 账号登录，可用能力取决于账号权限。Codex 模型选择器读取经过认证的上游目录，并按当前推理路径筛选条目。

AI 润色、屏幕感知和助手联网搜索均提供 **关闭／开启／自动**。只有自动档会使用所选决策模型判断是否需要处理。在 **设置 → 决策模型** 中选择由 TypeSafe（官方）、OpenRouter 或 Vercel 提供的 **JEV**，或 Liquid AI 的 **d1:free**。缺少密钥、超时或判断不确定时沿用原处理流程；决策模型不会生成替换文本，翻译和明确的编辑操作仍会执行。

自动润色以 80% 的无需修改概率为跳过门槛；同时开启自动屏幕感知时，也需要至少 80% 的无需屏幕概率。服务失败会回退到正常润色，并记录在 `app.log` 中。

还可单独开启决策模型的纠错规则审核和润色原意检查。原意检查在后台运行，不延迟或替换输出；你亲自确认的纠错规则会保留。

搜索选项包括受支持服务的模型内置搜索、免密钥 Bing 摘要、可选填 API Key 的 Exa MCP、Tavily 和 Google grounding。Tavily 与 Google 需要密钥；ChatGPT/Codex 登录使用 Exa 联网搜索。服务可能限流或要求浏览器验证，应用会提示搜索失败，并可在没有网页来源的情况下继续回答。

</details>

<details>
<summary><strong>从 1.6 之前的版本升级</strong></summary>

原有 Qwen 1.7B 选项会迁移到 R2T2，需要另行下载模型。Qwen 0.6B 和已有模型缓存保留。R2T2 使用独立的[模型许可证](src-tauri/resources/R2T2-MODEL-LICENSE.txt)。

</details>

## 1.7 更新

- **生命周期修复**：处理录音、热键替换、麦克风监测、OAuth 登录和模型下载中的过期回调与取消问题。
- **最终字幕保护**：迟到的预览事件不会覆盖已经完成的字幕结果。
- **形式验证成为发布门槛**：每个发布候选都需通过有界 TLA+ 状态模型、Lean 契约及实现回归测试。

这些检查在[明确的假设与边界](formal/COVERAGE.md)下验证应用状态和协议契约，不代表每条编译指令、外部依赖或识别结果都已得到证明。[验证指南](formal/README.md)提供复现命令与证据。

## 数据去向

| 设置 | 数据如何处理 |
|:--|:--|
| **本地识别** | 音频在电脑上处理；首次下载模型需要联网。 |
| **云端识别** | 音频发送给所选识别服务商。 |
| **润色、助手与决策模型** | 使用时将文本和处理要求发送给对应服务；启用截图、选区上下文或搜索后，也可能发送图片、选中文本或查询。 |
| **历史与音频保存** | 两者默认关闭，开启后保存在本地；保存音频需先开启历史。 |

决策模型 API 密钥按服务商分别存入系统凭据库。

完整的网络请求、本地存储和第三方服务说明见[隐私政策](docs/privacy.md)。

## Code signing policy

v1.7.4 Windows 安装包目前未签名。已于 2026 年 10 月 7 日提交 SignPath Foundation
免费签名申请，正在等待审核；通过审核并完成可验证的托管构建后，才能发布签名安装包。
详见[代码签名政策](docs/code-signing.md)。

[下载已发布版本](https://github.com/sypsyp97/light-whisper/releases/latest)。

## 开发文档

[构建与排障](docs/development.md) · [发版流程](docs/releasing.md) · [R2T2 后端](docs/r2t2-native.md) · [形式验证](formal/README.md)

基于 Tauri、React 和 Rust 构建，使用 Qwen3-ASR / transcribe.cpp、Confucius4-R2T2 / audio.cpp 和 FireRedVAD。

Light-Whisper 是采用 [GPL-3.0-only](LICENSE) 许可证的开源软件。第三方代码与模型继续适用各自条款，详见 [NOTICE](NOTICE) 和 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
