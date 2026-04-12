# Moonshine 会议助手 — 产品设计规格书

> 版本：v2.0（商业化重构）
> 目标：CEO 看了说 wow 的会议记录产品

---

## 1. Concept & Vision

**一句话定位：**
本地优先、AI 加持的会议伴侣——隐私可信、秒开秒用、输出专业。

**情感目标：**
用户打开 App → 感觉「这工具懂我」→ 会议结束后感觉「这段对话没白开」。

**设计哲学：**
> 工具的价值在于「让你专注会议，而不是工具本身」。

视觉语言参考：Linear（克制）+ Raycast（效率感）+ 播客 Transcript（可读性）

---

## 2. Design Language

### 2.1 色彩系统

```
背景层：
  --bg-base:      #09090e   ← 全局最深背景
  --bg-surface:   #111118   ← 卡片/面板
  --bg-elevated:  #18181f   ← 悬浮/下拉
  --bg-input:     #1c1c26   ← 输入框/选择器

边框：
  --border-subtle: #1c1c28   ← 极淡分割线
  --border:        #26263a   ← 标准边框
  --border-strong: #363650   ← 强调边框

主色调（紫蓝）：
  --accent:        #7c6af5   ← 主品牌色（按钮、焦点）
  --accent-hover:  #9585f8
  --accent-dim:    rgba(124, 106, 245, 0.12)  ← 背景提亮

功能色：
  --recording:     #f25757   ← 录制中（红色）
  --recording-dim: rgba(242, 87, 87, 0.12)
  --success:       #34d399   ← 成功/就绪
  --success-dim:   rgba(52, 211, 153, 0.12)
  --warning:       #f5a623   ← 警告/异常
  --warning-dim:   rgba(245, 166, 35, 0.12)
  --info:          #60a5fa   ← 信息提示

文字：
  --text-primary:  #ededf0   ← 主文字
  --text-secondary: #9092a8  ← 次要文字
  --text-muted:    #555770   ← 占位/禁用
  --text-inverse:  #09090e   ← 深色背景上的浅色文字
```

### 2.2 字体

```
UI 字体：
  font-family: 'Inter', 'Noto Sans SC', -apple-system, sans-serif
  权重：400（正文）/ 500（标签）/ 600（标题）/ 700（数字强调）

等宽字体（时间戳/代码）：
  font-family: 'JetBrains Mono', 'Fira Code', monospace

字号系统：
  --text-xs:   10px   ← 标签/角标
  --text-sm:   12px   ← 辅助文字/统计数字
  --text-base: 13px   ← 正文
  --text-lg:   15px   ← 字幕主文字
  --text-xl:   18px   ← 面板标题
  --text-2xl:  24px   ← Hero 标题
  --text-3xl:  36px   ← 启动页大标题
```

### 2.3 间距系统

```
--space-1: 4px
--space-2: 8px
--space-3: 12px
--space-4: 16px
--space-5: 20px
--space-6: 24px
--space-8: 32px
--space-10: 40px
--space-12: 48px
--space-16: 64px
```

### 2.4 圆角

```
--radius-sm: 4px   ← 小标签/角标
--radius:    8px   ← 卡片/按钮/输入框
--radius-lg: 12px  ← 大面板/弹窗
--radius-xl: 20px  ← 启动页元素
--radius-full: 9999px ← 圆形按钮/徽章
```

### 2.5 动效哲学

**原则：克制 + 有意义**
- 每处动画都有原因，不为炫技
- 快速响应：hover 60ms，转场 200-350ms
- 录制状态要有「生命感」：呼吸灯、波形

**标准缓动：**
```css
--ease-out: cubic-bezier(0.16, 1, 0.3, 1);     /* 自然减速 */
--ease-spring: cubic-bezier(0.34, 1.56, 0.64, 1); /* 弹性 */
--ease-in-out: cubic-bezier(0.4, 0, 0.2, 1);   /* 对称 */
```

**动效规范：**
- 字幕出现：slide-up + fade-in，200ms，--ease-out，stagger 30ms
- 面板切换：opacity + scale(0.98→1)，250ms
- 录制按钮：脉冲光环 + 颜色切换，2s infinite
- VAD 波形：canvas 实时绘制，16fps
- 空状态图标：轻微 float 动画，3s ease-in-out infinite

---

## 3. Layout & Structure

### 3.1 整体架构（三栏）

```
┌─────────────────────────────────────────────────────────────────┐
│  标题栏 40px                                          [—][□][×]  │
├────────────┬────────────────────────────┬─────────────────────┤
│            │                            │                     │
│  控制面板   │       实时字幕区域          │    AI 摘要面板      │
│  240px     │       flex-1               │    280px（可折叠）   │
│            │                            │                     │
│  ·设备选择  │  ·实时字幕流               │  ·实时摘要          │
│  ·波形可视化│  ·时间戳                   │  ·关键词提取        │
│  ·统计数字  │  ·说话人标签               │  ·会议统计          │
│  ·控制按钮  │                            │                     │
│            │                            │                     │
└────────────┴────────────────────────────┴─────────────────────┘
```

**响应式策略：**
- < 900px：右侧面板默认折叠
- < 700px：左侧面板收缩为图标模式
- Electron 固定最小 760×520（已定义）

### 3.2 启动页（全屏覆盖）

首次启动或无录音历史时，显示品牌启动页：
- 中央：动态月球图形（CSS animation）
- 下方：4 步引导（设备检测 → 权限 → 试用文字 → 开始使用）
- 底部：已有会议记录用户可直接点击「跳过，进入应用」

---

## 4. Features & Interactions

### 4.1 启动引导（Onboarding）

**Step 1：品牌展示（1.5s 自动跳过或点击跳过）**
- 月球动画（纯 CSS）
- 品牌名称 + 标语淡入

**Step 2：设备检测**
- 自动扫描麦克风
- 扫描结果逐个出现动画
- 发现 Loopback 设备时高亮提示

**Step 3：权限引导**
- 麦克风权限说明（图标示意）
- 「允许」按钮点击后检测权限状态

**Step 4：简介**
- 3 个核心功能图文展示
- 「开始使用」主按钮

### 4.2 主界面 — 控制面板

**设备选择区：**
- 麦克风选择：下拉，带设备名预览
- 内录设备选择：（同）
- Loopback 未检测到时：橙色提示条 + 「查看帮助」可展开
- 刷新设备按钮

**VAD 波形可视化：**
- 录制中：实时显示输入音量波形（canvas）
- 静音时：波形变暗，显示 VAD 跳过提示
- 非录制时：静态居中波形图标

**统计数字：**
- 录制时长（mm:ss，大字体）
- 麦克风转写条数
- 内录转写条数
- 总字数

**控制区：**
- 录制按钮：圆形，红色脉冲光环（录制中）
- 停止按钮：（录制中显示）
- 导出下拉 + 导出按钮
- 快捷键提示（⌘R 开始，⌘S 停止，⌘E 导出）

### 4.3 主界面 — 字幕区域

**字幕卡片设计：**
```
┌──────────────────────────────────────────────────────────────┐
│ 🎤  麦克风                              10:32:15 AM          │
│                                                              │
│  "所以我们这个季度的主要目标是提升用户留存率……"              │
└──────────────────────────────────────────────────────────────┘
```
- 卡片圆角 10px，带 --border 边框
- hover：边框颜色变为 --accent
- 新条目：从底部滑入，opacity 0→1
- 来自内录的条目：左侧 --info 色竖条标识
- 长文本自动换行，最大宽度 100%

**空状态：**
- 居中图标：🎙️（带 float 动画）
- 文字：「准备就绪，等待开始」
- 副文字：「点击上方 ⏺ 或按 ⌘R 开始录制」

### 4.4 主界面 — AI 摘要面板

**实时摘要生成（Mock 数据流）：**
- 会议进行中，右侧每 30 秒更新一次摘要
- 摘要内容：关键词 + 核心观点（结构化）
- 关键词标签：pill 样式，点击可高亮对应字幕

**会议统计：**
- 总时长
- 总字数
- 麦克风 / 内录 比例（迷你饼图）
- 语速（字/分钟）

**导出快捷入口：**
- MD / SRT / TXT 三个图标按钮
- Hover 显示格式说明

### 4.5 快捷键

| 快捷键 | 功能 |
|--------|------|
| ⌘R / Ctrl+R | 开始录制 |
| ⌘S / Ctrl+S | 停止录制 |
| ⌘E / Ctrl+E | 导出 |
| ⌘, / Ctrl+, | 设置 |
| Esc | 停止录制（如果正在录制） |

### 4.6 状态设计

**全局状态指示（标题栏右侧）：**
- 🔴 录制中：红点 + 「录制中 mm:ss」
- 🟡 加载中：黄色脉冲
- 🟢 就绪：绿色静态点
- ⚠️ 异常：红色静态点 + 提示

**设备状态条：**
- 就绪：绿色勾 + 设备名
- 警告：橙色三角 + 原因
- 错误：红色叉 + 解决方案

---

## 5. Component Inventory

### 5.1 录制按钮

- 默认：圆形 56px，背景透明，边框 --border-strong
- Hover：背景 --accent-dim，边框 --accent
- 录制中：背景 --recording，外环呼吸脉冲动画
- 禁用：opacity 0.3，cursor not-allowed

### 5.2 字幕卡片

- 默认：背景 --bg-surface，边框 --border-subtle
- Hover：边框 --border
- 聚焦（最后一条）：左边框 2px solid --accent-dim

### 5.3 设备选择器

- 默认：背景 --bg-input，边框 --border，圆角 --radius
- Focus：边框 --accent，box-shadow: 0 0 0 3px --accent-dim
- 错误：边框 --recording

### 5.4 开关 Toggle

- 默认滑块：背景 --border-strong
- 开启：背景 --accent
- 动画：200ms --ease-out

### 5.5 标签 / Badge

- 麦克风来源：绿色背景 + 绿色文字
- 内录来源：蓝色背景 + 蓝色文字
- 关键词：半透明紫色背景 + 紫色文字

---

## 6. Technical Approach

### 6.1 渲染层

- 纯 HTML + CSS + Vanilla JS（无框架依赖，保持 Electron 轻量）
- CSS 变量实现主题系统
- Canvas API 绘制 VAD 波形
- CSS animation + Intersection Observer 实现滚动入场动画

### 6.2 Electron 主进程

- main.js 负责窗口管理、IPC、Python 进程生命周期
- preload.js 暴露安全 bridge
- electron-log 记录完整日志

### 6.3 Python 后端（不变）

- transcribe.py：音频采集 + VAD + moonshine 转写
- stdout JSON 协议：IPC 通道
- list_devices.py：音频设备枚举

### 6.4 构建

- electron-builder：Windows NSIS + Portable + macOS DMG + Linux AppImage
- 打包资源：main.js + preload.js + renderer/ + python/ + assets/

---

## 7. Implementation Checklist

- [ ] 启动引导页（Onboarding）完整实现
- [ ] 品牌视觉系统（CSS 变量）
- [ ] 三栏布局重构
- [ ] VAD 波形可视化（Canvas）
- [ ] 字幕卡片动画系统
- [ ] AI 摘要面板（Mock 数据 + 预留接口）
- [ ] 快捷键系统
- [ ] 状态系统（全局状态指示器）
- [ ] 设备引导优化
- [ ] 空状态 + 加载状态 + 错误状态
- [ ] 导出面板优化
- [ ] 响应式适配（< 900px）
