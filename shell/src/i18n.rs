//! 界面语言：跟随 OTools 宿主语言，接上游 `soundcraft_ui_egui::localization` 模块。
//!
//! 上游 2026-11 起自带 localization（`crates/ui-egui/src/localization.rs`：English +
//! PortugueseBrazil 静态表，显示字符串原样过 `crate::localization::text` / `localized_format!`），
//! 旧的自研 tr() 包裹补丁已弃用。插件这边只做两件事：
//!
//! 1. `shell/patches/soundcraft-zh-cn.patch`（`node scripts/gen-i18n.mjs` 从
//!    `shell/i18n/zh-CN.tsv` 生成）给上游 `Language` 加一个 `Chinese` 变体和 zh-CN 表；
//! 2. 这里把宿主语言（`window.soundcraftOtools.language()`，BCP 47）映射成上游
//!    `Language`，启动时写进 `app.ui.language`（上游每帧 `enter(self.ui.language)`），
//!    运行中变了也跟着改。
//!
//! 宿主不可用（把 dist 拖进普通浏览器）时按浏览器 navigator.language 走同样的映射。

use soundcraft_ui_egui::localization::{self, Language, LanguageScope};
use std::cell::RefCell;

thread_local! {
    /// 上一次写进 app 的宿主语言，避免每帧重复调 `soundcraftOtools.language()`。
    static LAST: RefCell<Option<Language>> = const { RefCell::new(None) };
}

/// BCP 47 locale → 是否该用简体中文表。
///
/// `zh` / `zh-CN` / `zh-SG` / `zh-Hans` 是简体；`zh-TW` / `zh-HK` / `zh-Hant` 只有繁体，
/// 但简体表也远比英文可读，所以一并走简中（没有第二张表时可接受）。
pub fn is_chinese(locale: &str) -> bool {
    let l = locale.trim().to_ascii_lowercase().replace('_', "-");
    l == "zh" || l.starts_with("zh-")
}

/// 宿主 locale → 上游语言。上游只认 `en` / `pt-BR` / `zh-CN`（`Language::parse`
/// 按整串等值匹配），zh-* 一律简中，其余精确匹配不中就回英文。
pub fn host_language() -> Language {
    let locale = crate::otools::language();
    if is_chinese(&locale) {
        return Language::Chinese;
    }
    let locale = locale.trim();
    if locale.is_empty() {
        return Language::English;
    }
    Language::parse(locale).unwrap_or(Language::English)
}

/// 启动期（SoundApp 还没建好）用宿主语言包住加载/报错文本。
pub fn enter_host() -> LanguageScope {
    localization::enter(host_language())
}

/// 启动时把宿主语言写进 app 的语言偏好；此后上游每帧 `enter(app.ui.language)`。
pub fn apply_at_start(app: &mut soundcraft_ui_egui::SoundApp) {
    let language = host_language();
    app.ui.language = language;
    LAST.with(|slot| *slot.borrow_mut() = Some(language));
    log::info!("界面语言：{:?}（宿主 locale = {:?}）", language.id(), crate::otools::language());
}

/// 每帧节流调用（调用方按帧号控制，见 `web.rs` 的 `WebShell::logic`）：
/// 宿主语言变了就写进 app 的语言偏好，运行中改宿主语言也能跟上。
///
/// 语言是上游的用户偏好项（preferences 里也能手选）：宿主语言变化优先，
/// 插件场景「跟着宿主走」是产品语义。
pub fn poll(app: &mut soundcraft_ui_egui::SoundApp) {
    let language = host_language();
    let changed = LAST.with(|slot| slot.borrow_mut().replace(language) != Some(language));
    if changed {
        app.ui.language = language;
        log::info!("界面语言切到 {:?}（宿主 locale = {:?}）", language.id(), crate::otools::language());
    }
}
