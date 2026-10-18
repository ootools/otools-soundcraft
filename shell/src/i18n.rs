//! 界面语言：把 OTools 宿主的界面语言变成一张查表，装进上游 `soundcraft_ui_egui::i18n`。
//!
//! 上游 SoundCraft **没有界面语言偏好项**（见 `upstream/docs/localization-parity.md`：
//! English only），所以「跟随宿主语言」这件事只能由插件这一层做：
//!
//! 1. `shell/index.html` 里的 `window.soundcraftOtools.language()` 读出宿主语言
//!    （`window.__OTOOLS_LOCALE__.locale`，BCP 47，如 `zh-CN`）；
//! 2. 这里判断该用哪张表，`set_table` 装进上游的 i18n 模块；
//! 3. 上游所有**显示用**字符串过 `crate::i18n::t(...)`，查不到就原样返回英文。
//!
//! 表在 `shell/i18n/zh-CN.tsv`，由 `node scripts/gen-i18n.mjs` 生成到
//! `src/i18n/table.rs`（已提交，构建不需要 node）。
//!
//! 已知边界：只覆盖**菜单 / 命令面板 / 窗口标题**这一层；面板内部、对话框字段与
//! 提示语仍是上游英文。`AGENTS.md` 的「已知限制」一节同步维护这句话。

pub mod table;

use std::cell::RefCell;

/// 简体中文对照表（`scripts/gen-i18n.mjs` 生成，按英文键有序）。
pub static ZH_CN: &[(&str, &str)] = table::ZH_CN;

thread_local! {
    /// 上一次见到的宿主语言，避免每帧重复装表。
    static LAST: RefCell<String> = const { RefCell::new(String::new()) };
}

/// BCP 47 locale → 是否用简体中文表。
///
/// `zh` / `zh-CN` / `zh-SG` / `zh-Hans` 是简体；`zh-TW` / `zh-HK` / `zh-Hant` 只有繁体，
/// 但简体表也远比英文可读，所以一并走简中（没有第二张表时可接受）。
pub fn is_chinese(locale: &str) -> bool {
    let l = locale.trim().to_ascii_lowercase().replace('_', "-");
    l == "zh" || l.starts_with("zh-")
}

/// 装表（`None` = 英文原文）。
pub fn apply(locale: &str) {
    if is_chinese(locale) {
        soundcraft_ui_egui::i18n::set_table(Some(ZH_CN));
        log::info!("界面语言：简体中文（宿主 locale = {locale:?}）");
    } else {
        soundcraft_ui_egui::i18n::set_table(None);
        if !locale.trim().is_empty() {
            log::info!("界面语言：English（宿主 locale = {locale:?}）");
        }
    }
}

/// 读一次宿主语言，变了才换表。
///
/// 一次 `soundcraftOtools.language()` 是一回 wasm-bindgen 调用加一次 `String` 分配，
/// 所以由调用方按帧号节流（见 `web.rs` 的 `WebShell::logic`）。
pub fn poll_host_language() {
    let locale = crate::otools::language();
    let changed = LAST.with(|slot| {
        let mut last = slot.borrow_mut();
        if *last == locale {
            false
        } else {
            *last = locale.clone();
            true
        }
    });
    if changed {
        apply(&locale);
    }
}