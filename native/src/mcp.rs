//! 上游 `soundcraft-automation` 与宿主 MCP 契约之间的薄适配（见宿主 spec §5.4）。
//!
//! 上游已经把工具层与 MCP 语义都做好了，这里只做三件事：
//!   1. `tool_definitions()` 原样转成宿主 `{ tools: [...] }`
//!   2. `Server<Headless>::call_tool(name, args)` 的返回值原样透传（已是 content + isError）
//!   3. 把 headless 会话常驻在 dylib 内，保证 `list_commands → execute → inspect_session`
//!      这类有状态序列落在同一个 Session 上
//!
//! **有意不做按工具名的过滤**：上游把 `APP_ONLY` 工具（截图 / 点按 / 菜单）也列进表里，
//! 但 `Headless` 调用它们时返回「需要桌面 App」的可读错误。原样透传既不会漂移，
//! 也让 AI 拿到能据以改参数或换工具的提示（宿主 spec §10.2 的同一原则）。

use serde_json::{json, Value};
use soundcraft_automation::mcp::Server;
use soundcraft_automation::Headless;
use soundcraft_engine::Engine;
use std::sync::{Mutex, OnceLock};

/// 宿主 `__otools_mcp_tools__` 的响应体。
pub fn tools_payload() -> Value {
    json!({ "tools": soundcraft_automation::mcp::tool_definitions() })
}

/// 进程内**常驻**的 headless MCP 服务。
///
/// 上游 `Server` 持有 `Headless`（内含 `Engine`），工具序列有状态：
/// `new_session` → `import_audio` → `bounce_mix` 依赖同一个 Engine。
/// 每次调用新建 Server 会让第二次调用面对空会话，整条工作流断裂。
static SERVER: OnceLock<Mutex<Server<Headless>>> = OnceLock::new();

fn server() -> &'static Mutex<Server<Headless>> {
    SERVER.get_or_init(|| Mutex::new(Server::new(Headless::new(Engine::default()))))
}

/// 关掉会话（供宿主热更新前的停机握手调用）。
pub fn shutdown_server() {
    if let Some(lock) = SERVER.get() {
        if let Ok(mut guard) = lock.lock() {
            *guard = Server::new(Headless::new(Engine::default()));
        }
    }
}

/// 宿主 `__otools_mcp_call__` 的响应体。
pub fn call_payload(name: &str, arguments: &Value) -> Value {
    let mut guard = match server().lock() {
        Ok(guard) => guard,
        // 锁中毒（上一次调用 panic）时给出明确错误，而不是继续用半坏状态。
        Err(_) => {
            return json!({
                "content": [{ "type": "text", "text": "headless 会话状态已损坏，请重启 OTools" }],
                "isError": true
            })
        }
    };
    guard.call_tool(name, arguments)
}

/// 上游 MCP 的 resources 能力（会话快照 / 命令目录）。
///
/// **不自己复制资源表**：上游 `Server::handle` 已经实现了 `resources/list`、
/// `resources/templates/list`、`resources/read`（读的是同一个 headless 会话）。
/// 这里把内层 JSON-RPC 请求原样转发过去，取 `result` 返回，因此上游改了资源
/// 描述（uri / name / mimeType / description）插件自动跟随，不会漂移。
pub fn resources_payload(method: &str) -> Result<Value, String> {
    forward(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": {}
    }))
}

/// 读一个资源。上游 `resources/read` 对未知 uri 会返回错误，这里原样归一。
pub fn read_resource_payload(uri: &str) -> Result<Value, String> {
    forward(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "resources/read",
        "params": { "uri": uri }
    }))
}

/// 把一条 JSON-RPC 请求转给上游 `Server::handle`，取出 `result`。
///
/// 失败一律返回 `Err`（宿主对 resources 只有「成功返回数据 / 报错」两种结果，
/// 不像 tools 那样有 `isError` 信封），这样网关层能区分资源不存在与后端损坏。
fn forward(request: &Value) -> Result<Value, String> {
    let mut guard = server()
        .lock()
        .map_err(|_| "headless 会话状态已损坏，请重启 OTools".to_string())?;
    let reply = guard
        .handle(request)
        .ok_or_else(|| "上游未响应 resources 请求".to_string())?;
    match reply.get("result") {
        Some(result) => Ok(result.clone()),
        None => Err(reply
            .get("error")
            .map(|e| e.to_string())
            .unwrap_or_else(|| "上游没有返回 result".to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 这些用例共享进程内的常驻会话（这本身就是被测行为），必须串行执行。
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn tools_payload_wraps_upstream_definitions() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let value = tools_payload();
        let tools = value["tools"].as_array().expect("tools array");
        assert!(!tools.is_empty(), "tool table should not be empty");
        for tool in tools {
            assert!(
                tool.get("name").and_then(Value::as_str).is_some(),
                "every tool needs a name: {tool}"
            );
        }
    }

    #[test]
    fn tools_payload_passes_through_app_only_tools() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let value = tools_payload();
        let names: Vec<&str> = value["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t.get("name").and_then(Value::as_str))
            .collect();
        assert!(
            names.contains(&"screenshot"),
            "APP_ONLY 工具也应透传，由上游给出可读错误；got {names:?}"
        );
    }

    #[test]
    fn call_unknown_tool_returns_is_error() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let result = call_payload("definitely_not_a_tool", &json!({}));
        assert_eq!(result["isError"], true, "result: {result}");
    }

    #[test]
    fn call_read_only_tool_succeeds_headless() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let result = call_payload("list_commands", &json!({}));
        assert_eq!(result["isError"], false, "result: {result}");
        assert!(
            result.get("content").and_then(Value::as_array).is_some(),
            "must return MCP content array: {result}"
        );
    }

    #[test]
    fn session_state_persists_across_calls() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // 防止「每次调用新建 Server」回归：建出来的轨道必须被下一次 inspect 看见。
        let created = call_payload(
            "execute",
            &json!({ "command": "track.new", "params": { "count": 2, "format": "stereo" } }),
        );
        assert_eq!(
            created["isError"], false,
            "track.new should succeed: {created}"
        );

        let inspected = call_payload("inspect_session", &json!({}));
        assert_eq!(
            inspected["isError"], false,
            "inspect should succeed: {inspected}"
        );
        let text = inspected["content"][0]["text"].as_str().unwrap_or_default();
        let parsed: Value = serde_json::from_str(text).expect("inspect_session returns JSON text");
        let tracks = parsed["tracks"].as_array().map(Vec::len).unwrap_or(0);
        assert!(
            tracks >= 2,
            "the session created in the previous call must still be there; got {tracks} tracks"
        );
    }

    fn tool_names() -> (std::sync::MutexGuard<'static, ()>, Vec<String>) {
        let guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let value = tools_payload();
        let names = value["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t.get("name").and_then(Value::as_str).map(str::to_string))
            .collect();
        (guard, names)
    }

    /// 上游 `resources/list` 声明了会话与命令目录两个资源，必须原样透出。
    #[test]
    fn resources_list_comes_from_upstream() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let value = resources_payload("resources/list").expect("upstream resources/list");
        let resources = value["resources"].as_array().expect("resources array");
        let uris: Vec<&str> = resources
            .iter()
            .filter_map(|r| r.get("uri").and_then(Value::as_str))
            .collect();
        assert!(uris.contains(&"soundcraft://session"), "got {uris:?}");
        assert!(uris.contains(&"soundcraft://commands"), "got {uris:?}");
    }

    /// 资源读取走的是同一个 headless 会话：读出来的必须是真 Session。
    #[test]
    fn read_resource_returns_session_json() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let value = read_resource_payload("soundcraft://session").expect("read session resource");
        let contents = value["contents"].as_array().expect("contents array");
        assert!(!contents.is_empty(), "contents should not be empty");
        let text = contents[0]["text"].as_str().unwrap_or_default();
        let parsed: Value = serde_json::from_str(text).expect("session resource is JSON");
        assert!(
            parsed.get("tracks").is_some(),
            "session resource should describe tracks: {text:.200}"
        );
    }

    /// 未知 uri 必须报错，而不是静默返回空内容。
    #[test]
    fn read_unknown_resource_is_an_error() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        assert!(read_resource_payload("soundcraft://nope").is_err());
    }

    /// 与工具面同一条守门逻辑：`plugin.json` 的 `mcp.resources` 必须与上游实际资源双向一致。
    #[test]
    fn plugin_manifest_resources_match_upstream() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let value = resources_payload("resources/list").expect("upstream resources/list");
        let actual: Vec<String> = value["resources"]
            .as_array()
            .expect("resources array")
            .iter()
            .filter_map(|r| r.get("uri").and_then(Value::as_str).map(str::to_string))
            .collect();

        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../plugin.json");
        let raw = std::fs::read_to_string(&manifest)
            .unwrap_or_else(|e| panic!("读取 {} 失败: {e}", manifest.display()));
        let parsed: serde_json::Value =
            serde_json::from_str(&raw).expect("plugin.json 必须是合法 JSON");
        let declared: Vec<String> = parsed["mcp"]["resources"]
            .as_array()
            .expect("plugin.json 必须声明 mcp.resources")
            .iter()
            .map(|item| item.as_str().unwrap_or_default().to_string())
            .collect();

        let missing: Vec<&String> = actual.iter().filter(|u| !declared.contains(u)).collect();
        assert!(
            missing.is_empty(),
            "上游有资源但 plugin.json 的 mcp.resources 未列出: {missing:?}；实际 {actual:?}"
        );
        let extra: Vec<&String> = declared.iter().filter(|u| !actual.contains(u)).collect();
        assert!(
            extra.is_empty(),
            "plugin.json 的 mcp.resources 含上游不存在的 uri: {extra:?}；实际 {actual:?}"
        );
    }

    /// `plugin.json` 的 `mcp.expose` 是对外白名单初值。若它列了上游不存在的工具名，
    /// 宿主设置页会出现永远勾不上的条目；工具改名时这里必须同步。这条测试就是防漂移。
    #[test]
    fn plugin_manifest_expose_matches_upstream_tools() {
        let (_guard, names) = tool_names();
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../plugin.json");
        let raw = std::fs::read_to_string(&manifest)
            .unwrap_or_else(|e| panic!("读取 {} 失败: {e}", manifest.display()));
        let parsed: serde_json::Value =
            serde_json::from_str(&raw).expect("plugin.json 必须是合法 JSON");
        let expose = parsed["mcp"]["expose"]
            .as_array()
            .expect("plugin.json 必须声明 mcp.expose");
        let exposed: Vec<String> = expose
            .iter()
            .map(|item| item.as_str().unwrap_or_default().to_string())
            .collect();

        // 方向一：expose 不能列出上游没有的工具（否则设置页出现永远勾不上的死条目）。
        let unknown: Vec<&String> = exposed
            .iter()
            .filter(|name| !names.iter().any(|n| n == *name))
            .collect();
        assert!(
            unknown.is_empty(),
            "plugin.json 的 mcp.expose 含上游不存在的工具: {unknown:?}；上游实际有 {names:?}"
        );

        // 方向二（「不能少功能」的守门员）：上游每个工具都必须在 expose 里。
        // 上游新增工具时这条会红，提醒同步 expose，避免新能力静默不可见。
        let missing: Vec<&String> = names.iter().filter(|n| !exposed.contains(n)).collect();
        assert!(
            missing.is_empty(),
            "上游有工具但 plugin.json 的 mcp.expose 未列出（新能力会不可见）: {missing:?}"
        );
    }
}
