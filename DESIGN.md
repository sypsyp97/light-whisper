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
  success: "#2E8B57"
  error: "#BF4D43"
typography:
  title:
    fontFamily: "PingFang SC, Microsoft YaHei, Noto Sans SC, system-ui, sans-serif"
    fontSize: "14px"
    fontWeight: 600
    lineHeight: 1.3
  body:
    fontFamily: "PingFang SC, Microsoft YaHei, Noto Sans SC, system-ui, sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.6
  label:
    fontFamily: "PingFang SC, Microsoft YaHei, Noto Sans SC, system-ui, sans-serif"
    fontSize: "12px"
    fontWeight: 500
    lineHeight: 1.4
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
    textColor: "{colors.surface-raised}"
    rounded: "{rounded.sm}"
    padding: "8px 18px"
  button-ghost:
    backgroundColor: "{colors.surface-raised}"
    textColor: "{colors.ink-muted}"
    rounded: "{rounded.sm}"
    padding: "6px 12px"
  input:
    backgroundColor: "{colors.surface-raised}"
    textColor: "{colors.ink}"
    rounded: "{rounded.sm}"
    padding: "10px 12px"
---

# Design System: 轻语 Whisper

## Overview

**Creative North Star: "Quiet Instrument"**

轻语是一个长期停留在桌面的生产力工具。界面应像可靠的录音设备：安静、精确、状态明确，让用户把注意力放在说话、校正和结果上。视觉采用中性工作表面、暖色操作信号和克制的紫色助手语义，所有动效只解释状态变化。

**Key Characteristics:**

- 紧凑、清晰、适合长时间使用。
- 暖色表示录音、润色与主要操作；紫色只表示助手。
- 深浅主题使用同一层级和组件语法。
- 150–250ms 的非超调动效；减少动态效果时立即切换。

## Colors

中性表面承载内容，暖陶土色承担行动与润色状态，克制紫色承担助手状态。

**The Two-State Rule.** `signal` 表示录音、润色和主要操作；`assistant` 专门表示助手。两者保持足够色差，紫色不扩散到普通控件。

**The Neutral Surface Rule.** 大面积背景保持中性灰。暖色通过信号色体现，不使用奶油纸张式大背景。

## Typography

产品界面只使用一套中文友好的系统无衬线字体。标题依靠字重和间距建立层级；按钮、标签和数据保持同一种字形语言。正文行长控制在 65–75 个字符以内。

**The Utility Type Rule.** 衬线字体不进入按钮、标签、状态和设置项。

## Elevation

默认深度由表面色和边框建立。阴影只用于浮层、拖拽态和短暂反馈；模糊半径控制在 12px 以内。字幕窗口可使用一次轻量背景模糊，因为它浮在其他应用上方。

**The Flat-by-Default Rule.** 静止卡片保持平面；浮层获得结构性阴影；边框与宽泛阴影不同时作为装饰。

## Components

### Buttons

- 主按钮使用信号色、8px 圆角和 8×18px 内边距。
- 次按钮使用透明或抬升表面；hover、focus、active、disabled、loading 状态必须齐全。
- 图标与文字共享 4–6px 间距，并在视觉中心线上对齐。

### Cards / Containers

- 设置区块使用 12–16px 圆角、中性表面和结构性细边框。
- 不嵌套装饰卡片；子配置使用色层或分隔线表达层级。

### Inputs / Fields

- 输入框使用抬升表面、8px 圆角和清晰标签。
- focus 使用信号色边框与 2px 外环；错误状态使用语义红色并提供具体恢复方式。

### Navigation

- 设置导航保持横向、可滚动和键盘可达。
- 激活项使用信号色与位置指示；动效 200ms 内完成。

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
