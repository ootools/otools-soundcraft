//! SoundCraft 的 OTools 插件外壳。
//!
//! 本文件改编自上游 `apps/soundcraft-web/src/main.rs` + `src/web.rs`：
//! UI 仍然是上游的 `soundcraft_ui_egui::SoundApp`（原封不动），
//! 唯一的区别是「文件从哪来、存到哪去」：
//!
//! 1. `io::set_deliver` —— 保存/导出走宿主写盘（`crate::otools::deliver`），
//!    不在 OTools 里退回浏览器下载（`crate::download`）；
//! 2. `Dialog::PathPrompt`（Open Session / Import Audio / Import MIDI）——
//!    在 OTools 里弹宿主打开对话框，字节进 `Inbox`；否则退回上游的 `rfd`；
//! 3. 首次启动用 OTools 的界面主题给 egui 一个初始亮度；
//! 4. 内嵌 CJK + 阿拉伯 fallback 字体：上游 `fonts.rs` 在 wasm 下只找系统字体
//!    （恒空），中文/日文/阿拉伯文的会话名、轨道名会画成方框（见 `soundcraft-otools.patch`）。
//!
//! 上游 `crates/*` 一律不改：它们按 upstream.pin 的 commit 拉取，保持原样才能干净地跟上上游。
//! 上游那些纯浏览器设施没有移植：GPU 丢失重启、开机字体抓取、多标签页 Web Locks、
//! 粘贴事件。它们在插件里没有意义。

#[cfg(target_arch = "wasm32")]
mod download;
#[cfg(target_arch = "wasm32")]
mod otools;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(target_arch = "wasm32")]
fn main() {
    web::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    // 外壳是 wasm-only 的：宿主（x86_64）下 cargo build 会走到这里，别把它当编译报错。
    eprintln!("otools-soundcraft only runs in a webview: build it with `trunk build --release` in shell/");
}