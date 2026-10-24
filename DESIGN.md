---
name: "轻语 Whisper"
description: "A quiet desktop instrument for voice capture, correction, and assisted research."
colors:
  signal: "#AE5630"
  signal-dark: "#D97757"
  assistant: "#7557D9"
  assistant-dark: "#A78BFA"
  surface: "#F5F6F7"
  surface-raised: "#FFFFFF"
  surface-dark: "#1A1917"
  ink: "#1A1A18"
  ink-muted: "#5A5854"
  ink-inverse: "#F5F5F0"
  success: "#2E8B57"
  error: "#BF4D43"
typography:
  title:
    fontFamily: '-apple-system, BlinkMacSystemFont, "SF Pro Text", "PingFang SC", "Helvetica Neue", "Segoe UI", "Microsoft YaHei UI", system-ui, sans-serif'
    fontSize: "13px"
    fontWeight: 700
    letterSpacing: "-0.02em"
  body:
    fontFamily: '-apple-system, BlinkMacSystemFont, "SF Pro Text", "PingFang SC", "Helvetica Neue", "Segoe UI", "Microsoft YaHei UI", system-ui, sans-serif'
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.6
    letterSpacing: "-0.008em"
  label:
    fontFamily: '-apple-system, BlinkMacSystemFont, "SF Pro Text", "PingFang SC", "Helvetica Neue", "Segoe UI", "Microsoft YaHei UI", system-ui, sans-serif'
    fontSize: "12px"
    fontWeight: 500
    lineHeight: 1.4
    letterSpacing: "-0.004em"
  mono:
    fontFamily: '"Cascadia Mono", Consolas, monospace'
rounded:
  xs: "4px"
  sm: "8px"
  md: "12px"
  lg: "16px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "16px"
  lg: "24px"
components:
  button-primary:
    backgroundColor: "{colors.signal}"
    textColor: "{colors.ink-inverse}"
    rounded: "{rounded.lg}"
    padding: "8px 18px"
  button-ghost:
    backgroundColor: "transparent"
    textColor: "{colors.ink-muted}"
    rounded: "{rounded.md}"
    padding: "6px 12px"
  input:
    backgroundColor: "{colors.surface-raised}"
    textColor: "{colors.ink-muted}"
    rounded: "{rounded.sm}"
    padding: "10px 12px"
---

# Design System: 轻语 Whisper

## Overview

**Creative North Star: "Quiet Instrument"**

本文记录现有界面，不要求改变实现。颜色、字体、圆角、阴影和动效以 `src/styles/theme.css` 为准，设置控件与内容样式分别见 `src/styles/pages.css` 和 `src/styles/markdown.css`。`.impeccable/design.json` 是旧版预览缓存，其中的 MiSans 字体、组件片段和阴影示例不作为当前实现的依据。

轻语是一个长期停留在桌面的生产力工具。界面应像可靠的录音设备：安静、精确、状态明确，让用户把注意力放在说话、校正和结果上。视觉采用中性工作表面、暖色操作信号和克制的紫色助手语义，所有动效只解释状态变化。

**Key Characteristics:**

- 紧凑、清晰、适合长时间使用。
- 暖色表示录音、润色与主要操作；紫色只表示助手。
- 深浅主题使用同一层级和组件语法。
- 状态动效使用现有时长与缓动 token：常用 100、140、200、280ms，退出 160ms，较慢过渡为 380、520ms；减少动态效果时关闭动画并将过渡缩短至近乎即时。

## Colors

中性表面承载内容，暖陶土色承担行动与润色状态，克制紫色承担助手状态。

**The Two-State Rule.** `signal` 表示录音、润色和主要操作；`assistant` 专门表示助手。两者保持足够色差，紫色不扩散到普通控件。

**The Neutral Surface Rule.** 大面积背景保持中性灰。暖色通过信号色体现，不使用奶油纸张式大背景。

## Typography

产品界面使用 `--font-ui` 的系统字体栈；Windows 回退包括 Segoe UI 和 Microsoft YaHei UI，不内置 MiSans，也不在运行时下载 UI 字体。正文为 14px、字重 400、行高 1.6，窗口标题为 13px、字重 700，设置区块标题为 14px、字重 600。正文使用 `-0.008em`，标签使用 `-0.004em`，区块标题使用 `-0.014em`，窗口标题使用 `-0.02em` 的负字距。

**The Native-Type Rule.** UI、正文、标签和状态共享系统 UI 字体栈；代码、路径及等宽数据使用 `--font-mono`（Cascadia Mono、Consolas、monospace）。数学公式保留 KaTeX 的专用数学字形。

## Elevation

深度由表面色、边框和现有阴影 token 共同建立。设置卡片使用 `--shadow-card`，主按钮使用 `--shadow-sm` 并保留内侧高光，hover 使用 `--shadow-md`；浮层按组件使用 `--shadow-md`、`--shadow-lg` 或 `--shadow-xl`。深浅主题分别定义阴影，其中深色主题的 `--shadow-xl` 模糊半径为 20px。主窗口配置原生 acrylic 效果，字幕和选区窗口保留现有透明与背景模糊处理。

**The Existing-Depth Rule.** 保留卡片、按钮和浮层已有的层次与状态反馈，使用对应阴影 token，不为满足旧版缓存的平面示例而改动界面。

## Components

### Buttons

- 标准主按钮使用信号色、16px 圆角和 8×18px 内边距；紧凑主按钮使用 8px 圆角和 6×14px 内边距。
- 标准 ghost 按钮使用透明背景、12px 圆角和 6×12px 内边距；保留现有 hover、focus、active、disabled、loading 状态。
- 图标与文字共享 4–6px 间距，并在视觉中心线上对齐。

### Cards / Containers

- 设置卡片使用抬升表面、16px 圆角、14px 内边距、结构性细边框和 `--shadow-card`。
- 不嵌套装饰卡片；子配置使用色层或分隔线表达层级。

### Inputs / Fields

- 输入框使用抬升表面、8px 圆角和清晰标签。
- 设置输入框使用 10×12px 内边距和次级文字色；focus 使用 `--color-border-accent` 边框与 2.5px 低透明度外环。保留现有键盘焦点与错误反馈。

### Navigation

- Settings use six task-based tabs. Show only the active panel; wrap tabs into two rows in narrow windows and support arrow-key navigation.
- Keep behavior controls separate from shared model connections. Collapse credentials and advanced options until needed; links open and focus the relevant service.

### Assistant Status

- 助手使用克制紫色；润色和录音使用暖色信号，并继续通过图标形状和文案区分阶段。
- 波形、旋转指示和文字必须共享同一 24–28px 对齐槽。

## Do's and Don'ts

### Do:

- **Do** 使用 CSS token 表达颜色、圆角、间距、阴影和动效。
- **Do** 保证所有自定义选择器支持方向键、Home/End、首字母检索和 Escape 回焦。
- **Do** 在长任务中显示阶段、耗时和必要的重试操作。
- **Do** 同时验证深色、浅色、高对比和减少动态效果模式。

### Don't:

- **Don't** 模仿 Claude 或其他 AI 产品的完整视觉风格。
- **Don't** 把助手紫色用于录音、润色或普通主按钮。
- **Don't** 使用奶油纸张背景、渐变文字、装饰性玻璃卡片或宽泛光晕。
- **Don't** 在滚动容器内放置会被裁切的绝对定位下拉框。
- **Don't** 使用超调、弹跳或与状态无关的动画。
