//! OTools 宿主桥：把「文件从哪来、存到哪去」换成 OTools 宿主的对话框与磁盘读写。
//!
//! 上游 `apps/soundcraft-web` 在浏览器里没有文件系统：File ▸ Open Session… /
//! Import ▸ Audio…/MIDI… 靠 `rfd` 的 `<input type=file>`，保存与导出靠浏览器下载。
//! OTools 宿主注入的 `window.otools` 提供了真正的系统文件对话框与 `readHostFile` /
//! `writeHostFile`（底层是宿主的 `tools_webview_read_file` / `tools_webview_write_file`，
//! 无路径白名单限制），所以本模块是插件**唯一**需要新写的 Rust 代码：其余全部复用上游 crate。
//!
//! 与上游的对接点：
//! - `Dialog::PathPrompt`（`session.open` / `file.import_audio` / `file.import_midi`）
//!   —— 上游网页版在这里弹 `rfd` 选文件框；插件版改成宿主对话框，读到的字节进
//!   [`crate::web::Inbox`]，下一帧由 `WebShell::handle` 交给 `soundcraft_engine::io`
//! - `soundcraft_engine::io::set_deliver` —— 保存/导出（`.scraftz` 会话包、WAV、MusicXML…）
//!   把字节交给宿主写盘；上游的钩子是同步 `fn`，而宿主对话框是 Promise，所以这里
//!   发起一次后台任务并立刻返回 `Ok`（与上游网页版「下载已开始即算已保存」一致）
//!
//! JS 侧的实现见 `shell/index.html` 里的 `window.soundcraftOtools`。

use std::cell::{Cell, RefCell};

use base64::Engine as _;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::js_sys;

/// 打开对话框的扩展名过滤，与上游 `web.rs` 的 `SESSION_EXTS` / `AUDIO_EXTS` / `MIDI_EXTS` 一致。
const SESSION_EXTS: &[&str] = &["scraftz", "scraft"];
const AUDIO_EXTS: &[&str] = &[
    "wav", "wave", "bwf", "aif", "aiff", "aifc", "flac", "mp3", "ogg", "oga", "m4a", "m4b", "aac", "mp4", "caf", "mkv", "webm",
];
const MIDI_EXTS: &[&str] = &["mid", "midi", "smf"];

/// 打开文件的用途，决定对话框的过滤器（上游 `web.rs` 的 `Purpose`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Session,
    Audio,
    Midi,
}

impl Purpose {
    /// 命令 id → 用途（上游 `Purpose::for_command`）。
    pub fn for_command(cmd: &str) -> Option<Self> {
        match cmd {
            "session.open" => Some(Self::Session),
            "file.import_audio" => Some(Self::Audio),
            "file.import_midi" => Some(Self::Midi),
            _ => None,
        }
    }

    /// 文件名 → 用途（拖放进来的文件按扩展名分流，上游 `Purpose::of`）。
    pub fn of(name: &str) -> Self {
        let ext = name.rsplit_once('.').map(|(_, x)| x.to_ascii_lowercase()).unwrap_or_default();
        if SESSION_EXTS.contains(&ext.as_str()) {
            Self::Session
        } else if MIDI_EXTS.contains(&ext.as_str()) {
            Self::Midi
        } else {
            Self::Audio
        }
    }

    /// 对话框标题与过滤器（浏览器兜底分支复用）。
    pub fn filter(self) -> (&'static str, &'static [&'static str]) {
        match self {
            Self::Session => ("SoundCraft 会话", SESSION_EXTS),
            Self::Audio => ("音频", AUDIO_EXTS),
            Self::Midi => ("MIDI", MIDI_EXTS),
        }
    }
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = soundcraftOtools, js_name = "available")]
    fn host_available() -> bool;

    #[wasm_bindgen(js_namespace = soundcraftOtools, js_name = "theme")]
    fn host_theme() -> String;

    /// `{title, filters, multiple}` → `[{name, path, dataBase64}]`（用户取消时空数组）。
    #[wasm_bindgen(js_namespace = soundcraftOtools, js_name = "openFiles")]
    fn host_open_files(request: JsValue) -> js_sys::Promise;

    /// `{name, dataBase64, forcePick}` → `{path}` 或 `null`（用户取消）。
    #[wasm_bindgen(js_namespace = soundcraftOtools, js_name = "saveFile")]
    fn host_save_file(request: JsValue) -> js_sys::Promise;
}

thread_local! {
    /// 读到/存下的文件交给它，下一帧由 `WebShell::logic` 打开（上游的 Inbox 机制）。
    static INBOX: RefCell<Option<Inbox>> = const { RefCell::new(None) };
    /// 缓存 egui Context，供异步任务完成后触发重绘。
    static REPAINT: RefCell<Option<egui::Context>> = const { RefCell::new(None) };
    /// 下一次 `deliver` 是否**必须**弹另存对话框（「Save Session As」置真，「保存」用上次路径）。
    static FORCE_PICK: Cell<bool> = const { Cell::new(false) };
}

/// 上游 `web.rs` 的 Inbox：文件按用途分流，等下一帧处理。
pub type Inbox = std::rc::Rc<std::cell::RefCell<Vec<(Purpose, String, Vec<u8>)>>>;
/// 宿主是否可用（不在 OTools 里时退回上游的浏览器行为：下载文件）。
pub fn available() -> bool {
    host_available()
}

/// OTools 的界面主题，用于给 egui 一个初始亮度（`""` = 跟随系统）。
pub fn theme() -> String {
    host_theme()
}

/// 装上插件侧的两个 inbox 与重绘句柄（启动时调用一次）。
pub fn install(inbox: Inbox, ctx: egui::Context) {
    INBOX.with(|slot| *slot.borrow_mut() = Some(inbox));
    REPAINT.with(|slot| *slot.borrow_mut() = Some(ctx));
}

pub(crate) fn repaint() {
    REPAINT.with(|slot| {
        if let Some(ctx) = slot.borrow().as_ref() {
            ctx.request_repaint();
        }
    });
}

/// 「下次保存必须弹另存对话框」（上游 Save As / Save Copy 的语义）。
pub fn set_force_pick(force: bool) {
    FORCE_PICK.with(|slot| slot.set(force));
}

/// 社区/外部链接（Discord、ArtCraft 官网、上游 GitHub）：插件里点击不再跳出。
pub fn is_community_url(url: &str) -> bool {
    url.contains("discord.gg") || url.contains("getartcraft.com") || url.contains("github.com/storytold")
}

/// 按上游命令 id 打开（`session.open` / `file.import_audio` / `file.import_midi`）：
/// `web.rs` 里的 `Purpose` 与本模块的是两个类型，按命令 id 对齐最稳。
pub fn pick_open_for_command(cmd: &str) {
    if let Some(purpose) = Purpose::for_command(cmd) {
        pick_open(purpose);
    }
}

/// 打开对话框：按用途选文件，读到的字节进 inbox，下一帧被打开/导入。
pub fn pick_open(purpose: Purpose) {
    let (title, exts) = purpose.filter();
    let request = open_request(title, exts, false);
    wasm_bindgen_futures::spawn_local(async move {
        let files = match open_files(&request).await {
            Ok(files) => files,
            Err(e) => {
                log::error!("打开失败：{e}");
                return;
            }
        };
        for (name, bytes) in files {
            INBOX.with(|slot| {
                if let Some(inbox) = slot.borrow().as_ref() {
                    inbox.borrow_mut().push((purpose, name, bytes));
                }
            });
        }
        repaint();
    });
}

/// `soundcraft_engine::io::set_deliver` 的钩子：把保存/导出的字节交给宿主写盘。
///
/// 上游的钩子是同步的 `fn`，而宿主的对话框是 Promise，所以这里发起一次后台任务并
/// 立刻返回 `Ok`（与上游网页版「下载已开始即算已保存」的语义一致）。
pub fn deliver(name: &str, bytes: &[u8]) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("需要一个文件名".into());
    }
    let force_pick = FORCE_PICK.with(|slot| slot.replace(false));
    let request = save_request(name, &base64::engine::general_purpose::STANDARD.encode(bytes), force_pick);
    let name = name.to_string();
    wasm_bindgen_futures::spawn_local(async move {
        match save_file(&request).await {
            Ok(Some(path)) => log::info!("saved {name} -> {path}"),
            Ok(None) => log::error!("保存已取消：{name}"),
            Err(e) => log::error!("保存失败：{name}：{e}"),
        }
    });
    Ok(())
}

fn open_request(title: &str, exts: &[&str], multiple: bool) -> JsValue {
    let request = js_sys::Object::new();
    let arr = js_sys::Array::new();
    for e in exts {
        arr.push(&JsValue::from_str(e));
    }
    let filters = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&filters, &JsValue::from_str("name"), &JsValue::from_str("文件"));
    let _ = js_sys::Reflect::set(&filters, &JsValue::from_str("extensions"), &arr);
    let filter_arr = js_sys::Array::of1(&filters);
    let _ = js_sys::Reflect::set(&request, &JsValue::from_str("title"), &JsValue::from_str(title));
    let _ = js_sys::Reflect::set(&request, &JsValue::from_str("filters"), &filter_arr);
    let _ = js_sys::Reflect::set(&request, &JsValue::from_str("multiple"), &JsValue::from_bool(multiple));
    request.into()
}

async fn open_files(request: &JsValue) -> Result<Vec<(String, Vec<u8>)>, String> {
    let value = JsFuture::from(host_open_files(request.clone())).await.map_err(js_error)?;
    let mut out = Vec::new();
    for entry in js_sys::Array::from(&value).iter() {
        let name = field(&entry, "name").unwrap_or_else(|| "untitled".to_string());
        let data = field(&entry, "dataBase64").unwrap_or_default();
        let bytes = base64_decode(&data)?;
        out.push((name, bytes));
    }
    Ok(out)
}

fn save_request(name: &str, data: &str, force_pick: bool) -> JsValue {
    let request = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&request, &JsValue::from_str("name"), &JsValue::from_str(name));
    let _ = js_sys::Reflect::set(&request, &JsValue::from_str("dataBase64"), &JsValue::from_str(data));
    let _ = js_sys::Reflect::set(&request, &JsValue::from_str("forcePick"), &JsValue::from_bool(force_pick));
    request.into()
}

async fn save_file(request: &JsValue) -> Result<Option<String>, String> {
    let value = JsFuture::from(host_save_file(request.clone())).await.map_err(js_error)?;
    if value.is_null() || value.is_undefined() {
        return Ok(None);
    }
    Ok(field(&value, "path"))
}

fn base64_decode(data: &str) -> Result<Vec<u8>, String> {
    if data.is_empty() {
        return Err("宿主返回的文件内容为空".into());
    }
    base64::engine::general_purpose::STANDARD.decode(data).map_err(|e| format!("宿主返回的内容不是合法的 base64：{e}"))
}

fn field(value: &JsValue, key: &str) -> Option<String> {
    js_sys::Reflect::get(value, &JsValue::from_str(key)).ok()?.as_string()
}

fn js_error(e: JsValue) -> String {
    e.as_string().unwrap_or_else(|| format!("{e:?}"))
}