# AGENTS.md —— otools-soundcraft 移植约定

给后续接手这个仓库的 agent（和人）的规则。核心一句话：**`upstream/` 一行都不要改**，
所有移植代码都放在 `shell/`，这样上游同步永远干净。

## 铁律

1. **`upstream/` 保持上游原样**。它是 `storytold/soundcraft` 在 `upstream.pin`
   里那个 commit 的浅拉取结果，**不在 git 里**（见 `scripts/fetch-upstream.mjs`）。
   任何「顺手改一下上游」的念头都会在下次同步时丢失或变成冲突。
   **确需裁剪上游时**：改完 `upstream/` 后用 `git diff > shell/patches/<名字>.patch`
   收成补丁、`git checkout -f` 还原，由 `fetch-upstream.mjs` 在每次拉取后幂等重放。
2. `shell/src/web.rs` 是上游 `apps/soundcraft-web/src/web.rs` 的整层拷贝，**不要就地改上游逻辑**；
   要改行为就改 `shell/src/otools.rs` 或 `shell/src/main.rs`，或在补丁里改上游。
3. **不引入 native cdylib**。OTools 宿主已经注入了
   `tools_webview_read_file` / `tools_webview_write_file` 与文件对话框，够用；
   再加 FFI 只是多一份动态库要跟着分发。
4. **改 `shell/Cargo.toml` 的上游依赖版本时，对齐 `upstream/Cargo.toml` 的
   `[workspace.dependencies]`**（egui/eframe 目前是 `0.36` 这一档）。

## 上手前必须知道的三件事

- **rustc 版本**：上游 `rust-version = "1.90"`，ui-egui 声明 `1.95`（egui 0.36 用了
  1.95 才稳定的 `Vec::push_mut`）。本机 1.94 会在 `epaint` 上直接 `E0658`。
  本机环境变量 `RUSTC_WRAPPER=sccache` 指向 PATH 里的旧 rustc，会让 cargo 用错工具链
  ——构建前 `unset RUSTC_WRAPPER`（`scripts/build-wasm.mjs` 也会忽略它）。
- **git 拉取优先走 SSH**：HTTPS 直连 github.com 在本机会偶发超时，
  `fetch-upstream.mjs` 的策略是「SSH 优先，回落 HTTPS」。
  上游的 `craft-ui` 是 git 依赖（`storytold/craft-libs`），cargo 侧要
  `CARGO_NET_GIT_FETCH_WITH_CLI=true`（构建脚本已设）。
- **trunk 首次运行会下载 wasm-bindgen-cli 与 wasm-opt**（GitHub release），
  国内网络可能要直连。
- **`NO_COLOR` 环境变量**：trunk 把它当布尔参数读，沙箱/CI 里常见的 `NO_COLOR=1`
  会让 trunk 以 `invalid value '1' for '--no-color'` 直接退出。
  `scripts/build-wasm.mjs` 已归一成合法布尔值。

## 上游管理：pin + 按需拉取（不是 subtree / submodule）

上游源码**不在**本仓库里，只有 `upstream.pin` 记录了仓库与精确的 40 位 commit：

```json
{ "upstream": "storytold/soundcraft", "ssh": "…", "https": "…", "ref": "main", "commit": "…" }
```

- **本地/CI 取源码**：`node scripts/fetch-upstream.mjs`（缺失才拉，已对齐就跳过；
  `--force` 丢弃本地改动重拉）。GitHub 允许按任意可达 commit 做 `--depth 1` 拉取。
- **同步上游更新**：改 `upstream.pin` 的 `commit` 为新的完整 SHA，重新拉取，
  然后 diff `upstream/apps/soundcraft-web/src/*.rs` 与 `shell/src/*.rs`。
- **为什么不用 subtree**：单个快照十几 MB、五个 craft 合计约 150 MB，且每次同步只增不减，
  还会把上游几千个文件混进自己的树（干扰 grep / IDE 搜索）。pin 到 SHA 反而更可复现。

## 移植层的地图

| 文件 | 作用 |
| --- | --- |
| `shell/index.html` | 唯一的 JS：`window.soundcraftOtools`（available/theme/language/openFiles/saveFile）+ WebAudio 首次手势 resume 垫片 |
| `shell/src/web.rs` | 上游 `apps/soundcraft-web/src/web.rs` 整层拷贝 + 三处改动（见下） |
| `shell/src/otools.rs` | 桥：`pick_open` / `pick_open_for_command` / `deliver` / 主题 / 社区链接判定 |
| `shell/src/download.rs` | 上游浏览器下载回退（原样副本） |
| `shell/src/i18n.rs` | 界面语言：读宿主 locale，把 `shell/i18n/zh-CN.tsv` 生成的表装进上游 `i18n` 模块 |
| `shell/i18n/zh-CN.tsv` | 英 → 简体中 对照表（**唯一要编辑的翻译文件**，1179 条）；`node scripts/gen-i18n.mjs` 生成 `src/i18n/table.rs` |
| `shell/patches/soundcraft-i18n.patch` | 上游 `i18n` 模块本身 + 所有显示点的 `t(...)` / `t_ctx(...)` 包装（413 处，21 个文件） |
| `shell/src/main.rs` | wasm-only 外壳入口 |
| `shell/patches/*.patch` | 对 `upstream/` 的本地裁剪（fallback 字体注入点 + 移除 Discord 链接） |
| `upstream/` | （不在 git 里）上游源码，都是 path 依赖直接复用 |

### `shell/src/web.rs` 相对上游改了三处

1. `take_path_prompt`：在 OTools 里弹宿主打开对话框（`crate::otools::pick_open_for_command`），
   不在 OTools 里才退回上游的 `rfd`；
2. `start()`：`io::set_deliver` 在 OTools 里接宿主写盘，否则接浏览器下载；
3. `logic()`：吞掉社区外链；启动时按宿主主题设 `ui.theme`。

## 常见任务

- **构建 dist/** → `node scripts/build-wasm.mjs`（会先自动 take upstream）。
- **本地试跑** → `scripts/deploy-to-otools.ps1` 同步到 otools 仓库的 `plugins/otools-soundcraft`。
- **验证宿主桥** → `node scripts/probe-host-bridge.mjs`（无头 Chromium + 假宿主）。
- **改 OTools 侧的元信息**（名称/图标/关键词）→ `plugin.json`。

## 已知限制

- 上游网页版的设施都没移植：TCP 控制通道、开机字体抓取、多标签页 Web Locks、粘贴事件。
- 会话在浏览器/wasm 下只能是单个 `.scraftz` 包（上游 `io::save_session` 的 wasm 分支），
  所以「保存」始终是一个文件，不会在旁边写 `Audio Files/` 目录。
- 界面语言：简体中文覆盖菜单栏、命令面板、所有浮动窗口（标题/字段/按钮）、工具栏提示、
  面板分区标题、轨道/片段右键菜单、About 与恢复对话框，以及插件分类名、插件名、插件参数名与选项值
  （`soundcraft_dsp` 的数据，出现在插件面板、插入菜单、轨道视图菜单、Clip Effects、AudioSuite 等处）。
  还没有的：引擎状态文案（含插件名的报错一籾整句英文，不半若中不中）、撤销历史步骤名、
  egui 自带的文本编辑右键菜单、插件搜索框的匹配（按英文名搜）。
  只跟随 OTools 宿主语言（`__OTOOLS_LOCALE__.locale` 以 `zh` 开头就用简中表），运行中切换语言下一秒生效。
  会话名/轨道名里的中文靠内嵌 fallback 字体显示（见 `soundcraft-otools.patch`）。

## 不要做的事

- 不要把 `upstream/` 提交进 git（它是构建中间件，不是源码）。
- 不要手改 `upstream/` 里的文件：本地裁剪一律走 `shell/patches/*.patch`，
  由 `scripts/fetch-upstream.mjs` 在拉取后重放（手改会在下次同步时静默丢失——
  补丁没打上时构建报的是「上游没有 xx 符号」这种隔层错误）。
- 不要在本仓库跑 `cargo build` 而不管 target：外壳是 wasm-only 的，
  宿主（x86_64）下 `main()` 只会打印一行提示，别把它当编译报错。

## 界面语言（i18n）

上游没有 i18n（`upstream/docs/localization-parity.md`：English only，估时 70–110h）。插件这一层自己搭了一套：

- `upstream/crates/ui-egui/src/i18n.rs` 是**补丁加进去的**（`thread_local` 存一张 `&'static [(&str,&str)]` 表，
  `t(s)` 二分查找，未命中原样返回）。补丁同时在 `lib.rs` 加 `pub mod i18n;`。
- 补丁把上游所有**显示用**字符串包上 `t(...)`。改上游显示文案就是改补丁：
  改 `upstream/` → 剥掉 fonts.rs（`git checkout -- crates/ui-egui/src/fonts.rs`）→
  `git diff > shell/patches/soundcraft-i18n.patch`（新文件先 `git add -N` 才会进入 diff）→
  **`git reset -- <新文件>`** → 把 otools 补丁打回去（`git apply shell/patches/soundcraft-otools.patch`）→
  `git checkout -f -- . && git clean -fd` → `git apply --check` 验证。
  不 `git reset` 的后果：索引里留一条空 blob，下次 `git checkout -f -- .` 会把新文件
  （i18n.rs）截成空文件，而 `git clean -fd` 不会删它（它已经“被跟踪”了），结果 `git apply` 报
  `already exists in working directory`。
- **只译 label，不译 path**。`MenuNode { label, path }` 的 `path` 承载逻辑
  （`command_for_path` / `ruler_id` / `mix_section_id` / `params_for`）；`live_label` 的撤销步骤名来自引擎，只译动词。
- 同理不能包的：自动化模式名（`m.label()` 同时是 `run` 的参数）、`timebase` 的 `ticks`/`samples`、
  `bit_depth` 的 `16`/`24`/`32f`、`FadeShape::label()`（同时是 `radio_value` 的值）、
  `TrackHeight::label()`（同时是 `run` 的参数）、`<factory default>`（预设名）、
  `panels.rs` 里 `for f in ["volume","pan",...]` 这种用来组命令 id 的字符串。
- 加翻译：改 `shell/i18n/zh-CN.tsv` → `node scripts/gen-i18n.mjs` → 重构建。补丁不动。
- 第三列（上下文）：同一个英文句在不同界面含义不同时用，目前只有一条。
  `Mix` 在插件参数里是干湿混合量（代码写 `t_ctx("param", p.name)`，译“混合”）；
  菜单 / 窗口标题 / 命令面板里的 `Mix` 是混音台窗口（默认键，译“混音台”）。
  写法：`Mix<TAB>混合<TAB>param`，生成的键是 `param\u{1f}Mix`。别的继续用两列。
- 验证：`node scripts/probe-language.mjs zh-CN` / `en-US`，截图对照。
