# SoundCraft · OTools 插件

[SoundCraft](https://github.com/storytold/soundcraft) 是 storytold 的 ArtCraft「Crafting Apps」家族之一：
纯 Rust 重写的 Pro Tools 式数字音频工作站，编译为 WebAssembly + egui，在浏览器里运行。
本仓库把它移植成 OTools 插件：UI 仍是上游 `soundcraft-ui-egui`，只替换了「文件从哪来、存到哪去」。

> 同家族移植模式见 `../otools-vectorcraft`（矢量绘图）。本插件沿用同一套结构。

## 目录结构

```
otools-soundcraft/
├── plugin.json            # 插件清单（宿主只认这个 + logo + dist）
├── logo.svg               # 插件图标
├── upstream.pin           # 上游 commit SHA（按需浅拉，不入库）
├── upstream/              # 上游源码（git 浅拉，.gitignore 忽略，**不入库**）
├── shell/                 # 本插件自己写的 Rust 外壳 + HTML 桥
│   ├── Cargo.toml         # 独立 workspace，path 依赖上游 crate
│   ├── Trunk.toml         # trunk 构建配置（产物 dist/）
│   ├── index.html         # window.soundcraftOtools 宿主桥 + WebAudio 首次手势 resume
│   ├── src/
│   │   ├── main.rs        # 编译自上游 soundcraft-web：wasm-only 外壳
│   │   ├── web.rs         # 编译自上游 apps/soundcraft-web/src/web.rs（整层拷贝 + 改动）
│   │   ├── otools.rs      # wasm-bindgen 桥：openFiles/saveFile/theme/language/deliver
│   │   ├── i18n.rs        # 界面语言：读宿主 locale → 装翻译表
│   │   ├── i18n/table.rs  # 由 scripts/gen-i18n.mjs 生成（不要手改）
│   │   └── download.rs    # 上游浏览器下载回退（原样副本）
│   ├── i18n/zh-CN.tsv      # 英 → 简体中 对照表（唯一要编辑的文件）
│   ├── assets/fonts/      # 内嵌 CJK + 阿拉伯 fallback 字体子集
│   └── patches/           # 对上游的本地裁剪补丁（随 upstream 重拉而重放）
│       ├── soundcraft-otools.patch
│       └── soundcraft-i18n.patch
├── scripts/               # 构建 / 打包 / 发布 / 验证
└── .github/workflows/     # 独立发布 CI
```

## 移植要点

### 1. 文件 I/O：宿主对话框替代浏览器选文件框

上游网页版没有文件系统：File ▸ Open Session…、Import ▸ Audio…/MIDI… 靠 `rfd` 的
`<input type=file>`，保存/导出靠浏览器下载。插件版把这两处换成 OTools 宿主：

| 上游钩子 | 网页版行为 | 插件版行为 |
| --- | --- | --- |
| `Dialog::PathPrompt`（session.open / file.import_audio / file.import_midi） | `rfd` 选文件，字节进 `Inbox` | `window.soundcraftOtools.openFiles`（宿主对话框 + `readHostFile`），字节进同一个 `Inbox` |
| `soundcraft_engine::io::set_deliver` | 浏览器下载 | 宿主另存对话框 + `tools_webview_write_file` 写盘 |

「保存」沿用上次落盘路径（同名单文件直接覆盖），「另存为 / 保存副本」强制弹对话框。

### 2. 中文字体：内嵌 fallback 子集

上游 `crates/ui-egui/src/fonts.rs` 只从系统字体目录读 UI 字体，在 wasm 下恒为空
（`load()` 直接返回 `None`），中文/日文/阿拉伯文的会话名、轨道名会画成方框。
`shell/patches/soundcraft-otools.patch` 给上游加了一个 `set_extra_fallback_fonts` 注入点，
外壳在启动时把内嵌的 CJK + 阿拉伯子集（`shell/assets/fonts/`）装进每个 font family。

### 3. WebAudio 首次手势 resume

cpal（wasm）在启动时就建输出流，早于任何用户手势，WebAudio 的 `AudioContext` 会停在
`suspended`：听不到声音，而且由音频回调驱动的走带时钟也不走。`shell/index.html` 里保留了
上游那段 AudioContext 包装 + 首次手势 resume 垫片。

### 4. 社区外链

About 面板里的 Discord 链接由补丁移除；`web.rs` 的 `logic()` 另外吞掉所有
`OpenUrl`（Discord / getartcraft.com / 上游 GitHub），点击不再跳出插件。

### 5. 界面语言：跟随 OTools 宿主（简体中文）

上游 SoundCraft **没有界面语言偏好项**（见 `upstream/docs/localization-parity.md`：English only）。
插件这一层把它补上：

1. `shell/index.html` 的 `window.soundcraftOtools.language()` 读出宿主注入的
   `window.__OTOOLS_LOCALE__.locale`（BCP 47，如 `zh-CN`）；
2. `shell/src/i18n.rs` 按 `zh` 前缀判定，把 `shell/i18n/zh-CN.tsv` 生成的表装进
   上游 `soundcraft_ui_egui::i18n`（由 `soundcraft-i18n.patch` 加进上游）；
3. 上游所有**显示用**字符串过 `crate::i18n::t(...)`，查不到就原样返回英文。

要加翻译：编辑 `shell/i18n/zh-CN.tsv`（一行一条，`英文` + TAB + `中文`）→
`node scripts/gen-i18n.mjs` → 重新构建。补丁里的包装点不用手改。

**只译 label，不译 path**。菜单树 `MenuNode { label, path }` 里 `path` 承载逻辑
（`command_for_path` / `ruler_id` / `mix_section_id` / `params_for` 全按 path 匹配），`label` 只用于显示。
同理，任何被 `match` / 查表当键用的字符串（自动化模式名、播放列表名、预设名）都不能包。

覆盖范围（简体中文）：菜单栏与命令面板全部条目、全部浮动窗口的标题与字段/按钮、
工具栏提示、面板分区标题、轨道/片段右键菜单、About 与恢复等对话框。
**还没有**：插件参数名与选项值（来自 `soundcraft_dsp` 的数据）、引擎状态文案、
撤销历史的步骤名、egui 自带的文本编辑右键菜单。

## 上游版本

打包所用的上游提交。`upstream.pin` 是唯一真源，这张表用来回溯「哪个插件版本打的是哪份上游代码」，
每次发布都要在这里加一行。

| 插件版本 | 上游标签 | 上游提交 | 打包日期 |
| --- | --- | --- | --- |
| 0.2.2 | v0.10.0 | `e8940766d77601942b31e62570a06c44db1ec68e` | 2026-10-21 |
| 0.2.1 | — | `f3a19480d8cc2d7a98cda492f5464d8c54558c9f` | — |

## 构建

```bash
unset RUSTC_WRAPPER                 # 它会把 cargo 指向旧 rustc
node scripts/build-wasm.mjs         # trunk build --release → dist/
```

需要 rustc ≥ 1.95 与 wasm32-unknown-unknown 目标（egui 0.36 的要求）。
trunk 首次运行会下载 wasm-bindgen-cli 与 wasm-opt（GitHub release），国内网络可能要直连。

## 本地验证

```bash
node scripts/probe-host-bridge.mjs  # 无头 Chromium + 假宿主：启动 / 打开 / 保存 / 覆盖
```

判定「真的启动」看 loading 节点是否被移除、canvas 是否有尺寸；宿主桥单独走一遍
打开 → 读字节 → 保存 → 同名单文件覆盖，并截图存到 `dist-pack/probe-screenshot.png`。

界面语言（伪造 locale，截图供肉眼核对）：

```bash
node scripts/probe-language.mjs zh-CN    # 默认：菜单栏 / 面板标题 / 传输栏应为中文
node scripts/probe-language.mjs en-US    # 应为英文（查不到就回退）
```

产出 `dist-pack/lang-menubar.png` 与 `dist-pack/lang-file-menu.png`。

## 跑起来

```powershell
pwsh -File scripts/deploy-to-otools.ps1
```

然后重载/重启 OTools，应用列表里会出现「音频工作站」插件。

URL 参数沿用上游：`?webgl` 强制 WebGL2、`?empty` 以空会话启动（默认载入演示会话）。

详细约定见 [AGENTS.md](./AGENTS.md)。