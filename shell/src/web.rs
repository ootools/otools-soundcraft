//! The plugin shell: the eframe web runner around [`SoundApp`], with the OTools host
//! bridge standing in for the browser's file picker and download.
//!
//! 改编自上游 `apps/soundcraft-web/src/web.rs`（整层拷贝），改动只有三处：
//! 1. `take_path_prompt` 在 OTools 里弹宿主的打开对话框（`crate::otools::pick_open`），
//!    字节进 `Inbox`；不在 OTools 里才退回上游的 `rfd` 浏览器选文件框；
//! 2. `io::set_deliver` 在 OTools 里把保存/导出的字节交给宿主写盘
//!    （`crate::otools::deliver`），否则仍是浏览器下载（`crate::download`）；
//! 3. `logic` 里吞掉社区外链，启动时跟随 OTools 的界面主题。

use serde_json::json;

use crate::otools::Purpose;

use soundcraft_engine::{Engine, io};
use soundcraft_ui_egui::dialogs::Dialog;
use soundcraft_ui_egui::sampler_window::SampleRead;
use soundcraft_ui_egui::{Services, SoundApp};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use wasm_bindgen::JsCast as _;

const CANVAS_ID: &str = "soundcraft_canvas";
const LOADING_ID: &str = "soundcraft_loading";

pub fn start() {
    eframe::WebLogger::init(log::LevelFilter::Info).ok();
    // 界面语言：第一帧之前就装好表（宿主语言决定菜单中文还是英文）。
    crate::i18n::poll_host_language();
    // 保存/导出：在 OTools 里交给宿主的另存对话框与磁盘；否则退回浏览器下载。
    if crate::otools::available() {
        io::set_deliver(crate::otools::deliver);
    } else {
        io::set_deliver(crate::download::download);
    }
    wasm_bindgen_futures::spawn_local(async {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else {
            log::error!("no document");
            return;
        };
        let Some(canvas) = document.get_element_by_id(CANVAS_ID).and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok()) else {
            log::error!("missing <canvas id=\"{CANVAS_ID}\">");
            return;
        };
        let mut options = eframe::WebOptions::default();
        if query().contains("webgl")
            && let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup
        {
            create.instance_descriptor.backends = eframe::wgpu::Backends::GL;
        }
        let result = eframe::WebRunner::new()
            .start(
                canvas,
                options,
                Box::new(move |cc| {
                    if let Some(rs) = &cc.wgpu_render_state {
                        log::info!("soundcraft-web: wgpu backend {:?}", rs.adapter.get_info().backend);
                    }
                    let inbox: Inbox = Rc::default();
                    let samples: SampleInbox = Rc::default();
                    let engine = if query().contains("empty") { Engine::default() } else { soundcraft_engine::demo::demo_engine() };
                    let player = soundcraft_playback::Player::new(Arc::new(engine.session().clone()));
                    // wasm 读不到系统字体（上游 fonts.rs 在 wasm 下恒空）：内嵌 CJK + 阿拉伯 fallback，
                    // 否则中文/日文/阿拉伯文的会话名、轨道名与界面文本画成方框。
                    soundcraft_ui_egui::fonts::set_extra_fallback_fonts(vec![
                        ("otools-cjk".into(), std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../assets/fonts/otools-cjk-subset.ttf")))),
                        ("otools-arabic".into(), std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../assets/fonts/otools-arabic-subset.ttf")))),
                    ]);
                    let mut app = SoundApp::new(engine, Some(player), Services::default());
                    // 第一次打开跟随 OTools 的界面主题（插件 webview 的 documentElement.classList）。
                    if crate::otools::theme() == "light" {
                        app.ui.theme = soundcraft_ui_egui::theme::ThemeMode::Light;
                    }
                    // 打开对话框读到的字节进这个 inbox（宿主不可用时整体静默）。
                    crate::otools::install(inbox.clone(), cc.egui_ctx.clone());
                    Ok(Box::new(WebShell { app, inbox, samples, frames: 0 }))
                }),
            )
            .await;
        if let Some(el) = document.get_element_by_id(LOADING_ID) {
            match result {
                Ok(()) => el.remove(),
                Err(e) => el.set_inner_html(&format!("<p>SoundCraft failed to start: {e:?}</p><p>A browser with WebGPU or WebGL2 is required.</p>")),
            }
        }
    });
}

fn query() -> String {
    web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default()
}

/// Files picked or dropped, read asynchronously (browsers can only read files that way) and
/// handled on the next frame. The browser has no paths: files travel as bytes.
type Inbox = Rc<RefCell<Vec<(Purpose, String, Vec<u8>)>>>;

/// Audio dropped on an instrument track or a Sampler window, its bytes read, waiting for the
/// next frame to load into that track's sampler.
type SampleInbox = Rc<RefCell<Vec<(SampleRead, Result<Vec<u8>, String>)>>>;

struct WebShell {
    app: SoundApp,
    inbox: Inbox,
    samples: SampleInbox,
    /// 帧计数，用来把「轮询宿主语言」节流到每秒一次。
    frames: u32,
}

impl WebShell {
    /// File > Open Session… / Import > Audio… / MIDI… would show a text box for a path, which the
    /// browser can't read (#63, #64). Show the browser's file picker instead.
    fn take_path_prompt(&mut self, ctx: &egui::Context) {
        let purpose = match &self.app.dialogs.open {
            Some(Dialog::PathPrompt { cmd, .. }) => Purpose::for_command(cmd),
            _ => None,
        };
        let Some(purpose) = purpose else { return };
        // 「另存为 / 保存副本」必须弹宿主的另存对话框；「保存」（已有路径）沿用上次落盘点覆盖。
        // 这里只置标记、不清对话框：命令照常确认执行，最后经 io::set_deliver 交到宿主。
        if let Some(Dialog::PathPrompt { cmd, .. }) = &self.app.dialogs.open {
            crate::otools::set_force_pick(matches!(cmd.as_str(), "session.save_as" | "session.save_copy"));
        }
        let cmd = match &self.app.dialogs.open {
            Some(Dialog::PathPrompt { cmd, .. }) => cmd.clone(),
            _ => return,
        };
        self.app.dialogs.open = None;
        // 在 OTools 里：弹宿主的打开对话框，字节由 otools 模块读进 Inbox（异步，下一帧处理）。
        if crate::otools::available() {
            crate::otools::pick_open_for_command(&cmd);
            return;
        }
        let (inbox, ctx) = (self.inbox.clone(), ctx.clone());
        wasm_bindgen_futures::spawn_local(async move {
            let (label, exts) = purpose.filter();
            let Some(file) = rfd::AsyncFileDialog::new().add_filter(label, exts).pick_file().await else { return };
            let bytes = file.read().await;
            inbox.borrow_mut().push((purpose, file.file_name(), bytes));
            ctx.request_repaint();
        });
    }

    fn take_drops(&mut self, ctx: &egui::Context) {
        // In the browser a dropped file's `path()` is only its name: read its bytes instead.
        // Audio dropped on an instrument track or a Sampler window goes to its sampler instead.
        let dropped = self.app.take_dropped_files(ctx);
        ctx.input_mut(|i| i.raw.dropped_files.clear());
        for read in self.app.take_sample_reads() {
            let (samples, ctx) = (self.samples.clone(), ctx.clone());
            wasm_bindgen_futures::spawn_local(async move {
                let bytes = read.file.bytes_async().await;
                samples.borrow_mut().push((read, bytes));
                ctx.request_repaint();
            });
        }
        for f in dropped {
            let name = f.path().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Audio.wav".into());
            let (inbox, ctx) = (self.inbox.clone(), ctx.clone());
            wasm_bindgen_futures::spawn_local(async move {
                match f.bytes_async().await {
                    Ok(bytes) => {
                        inbox.borrow_mut().push((Purpose::of(&name), name, bytes));
                        ctx.request_repaint();
                    }
                    Err(e) => log::error!("couldn't read dropped file {name}: {e}"),
                }
            });
        }
    }

    fn handle(&mut self, purpose: Purpose, name: &str, bytes: &[u8]) {
        if matches!(purpose, Purpose::Session) && soundcraft_ui_egui::bounce::refuses_session_switch(&mut self.app) {
            return;
        }
        let e = &mut self.app.engine;
        let at = e.session().edit.selection.start.max(0);
        let result = match purpose {
            Purpose::Session => {
                let _ = self.app.run("transport.stop", json!({}));
                io::open_session_bytes(&mut self.app.engine, name, bytes).map(|missing| {
                    if missing.is_empty() { format!("Opened {name}") } else { format!("Opened {name} ({} missing media)", missing.len()) }
                })
            }
            // Imports are edits: one undo step each, like the import commands.
            Purpose::Audio => {
                let before = e.session_arc();
                io::import_audio_bytes(e, name, bytes, None, None, at).map(|_| {
                    e.push_undo("Import Audio", before);
                    format!("Imported {name}")
                })
            }
            Purpose::Midi => {
                let before = e.session_arc();
                io::import_midi_bytes(e, bytes, at, true).map(|_| {
                    e.push_undo("Import MIDI", before);
                    format!("Imported {name}")
                })
            }
        };
        self.app.ui.status = result.unwrap_or_else(|err| err.to_string());
    }
}

impl eframe::App for WebShell {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 界面语言跟着 OTools 宿主走。egui 空闲不跑帧、trunk 产出的是 ES module，
        // JS → wasm 的回调不可行，所以这里按帧号轮询（每 60 帧约一秒一次）：
        // 启动时一定生效，运行中改宿主语言也能跟上。
        self.frames = self.frames.wrapping_add(1);
        if self.frames % 60 == 1 {
            crate::i18n::poll_host_language();
        }
        self.take_drops(ctx);
        self.take_path_prompt(ctx);
        let pending = std::mem::take(&mut *self.inbox.borrow_mut());
        for (purpose, name, bytes) in pending {
            self.handle(purpose, &name, &bytes);
        }
        let samples = std::mem::take(&mut *self.samples.borrow_mut());
        for (read, bytes) in samples {
            self.app.load_sample_bytes(&read, bytes);
        }
        self.app.logic(ctx);
        // 让插件更纯粹：吞掉所有社区/外部链接（Discord、ArtCraft 官网、上游 GitHub），
        // 点击不再跳出插件。文档内普通链接不受影响。
        ctx.output_mut(|o| {
            o.commands.retain(|c| match c {
                egui::OutputCommand::OpenUrl(u) => !crate::otools::is_community_url(&u.url),
                _ => true,
            });
        });
        // A prompt opened during this frame becomes a picker on the next one.
        self.take_path_prompt(ctx);
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.app.raw_input_hook(raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
    }
}
