//! Session 级供应商路由器
//!
//! 在代理模式下，为不同 session 提供独立的供应商路由。
//! override 关系持久化到 JSON 文件（`~/.cc-switch/session_routes.json`），重启后自动恢复。
//!
//! ## 核心设计
//!
//! - `sessions`：自动发现的活跃 session（从请求中提取，纯内存）
//! - `overrides`：用户显式设置的 session → provider 覆盖（持久化到 JSON 文件）
//!
//! 请求到达时：
//! 1. 查 overrides 有无覆盖 → 有则路由到指定 provider
//! 2. 无覆盖 → fallback 到全局 current_provider

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tokio::sync::RwLock;

/// 持久化文件中的单条 override 记录
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedOverride {
    provider_id: String,
    display_name: String,
    project_dir: Option<String>,
    pinned_at: i64,
}

/// 持久化文件结构
#[derive(Debug, Default, Serialize, Deserialize)]
struct PersistedData {
    /// key: "app_type:session_id"
    overrides: HashMap<String, PersistedOverride>,
}

/// 活跃 session 信息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveSessionInfo {
    /// Session 唯一标识
    pub session_id: String,
    /// 应用类型（claude / codex / gemini 等）
    pub app_type: String,
    /// 当前使用的供应商 ID
    pub provider_id: String,
    /// 显示名称（project_dir 的 basename 或 session_id 前缀）
    pub display_name: String,
    /// 项目目录完整路径
    pub project_dir: Option<String>,
    /// 最后活跃时间（epoch 毫秒）
    pub last_active_at: i64,
    /// Session ID 来源
    pub source: SessionDiscoverySource,
}

/// Session 发现来源
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionDiscoverySource {
    /// 从请求头提取（客户端提供的稳定 session ID）
    Header,
    /// 从 metadata.user_id 提取（Claude）
    MetadataUserId,
    /// 从 metadata.session_id 提取
    MetadataSessionId,
    /// 代理自动生成（不稳定，不持久）
    Generated,
}

/// 供前端展示的 session 路由条目
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRouteEntry {
    pub session_id: String,
    pub app_type: String,
    pub provider_id: String,
    pub display_name: String,
    pub project_dir: Option<String>,
    pub last_active_at: i64,
    /// 是否有显式覆盖（true = 用户手动设置，false = 继承全局）
    pub is_routed: bool,
    /// 对应的客户端进程是否存活（通过 ~/.claude/sessions/ 中的 PID 检查）
    pub is_alive: bool,
}

/// 获取 JSON 持久化文件路径
fn persistence_path() -> PathBuf {
    crate::config::get_app_config_dir().join("session_routes.json")
}

/// 从文件加载持久化数据
fn load_from_file() -> PersistedData {
    let path = persistence_path();
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_else(|e| {
            log::warn!("[SessionRouter] 解析持久化文件失败: {e}，使用空数据");
            PersistedData::default()
        }),
        Err(_) => PersistedData::default(),
    }
}

/// 将数据持久化到文件
fn save_to_file(data: &PersistedData) {
    let path = persistence_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_string_pretty(data) {
        Ok(content) => {
            if let Err(e) = std::fs::write(&path, content) {
                log::warn!("[SessionRouter] 写入持久化文件失败: {e}");
            }
        }
        Err(e) => {
            log::warn!("[SessionRouter] 序列化持久化数据失败: {e}");
        }
    }
}

/// Session 级供应商路由器
///
/// 内存结构 + JSON 文件持久化，override 关系重启后自动恢复。
/// 线程安全：overrides 写操作通过 Mutex 序列化文件 IO，内存状态通过 RwLock 保护。
pub struct SessionRouter {
    /// 已发现的活跃 session：`(app_type, session_id) → ActiveSessionInfo`
    sessions: RwLock<HashMap<(String, String), ActiveSessionInfo>>,
    /// 用户显式覆盖：`(app_type, session_id) → provider_id`
    overrides: RwLock<HashMap<(String, String), String>>,
    /// 持久化文件的写锁（防止并发写入冲突）
    file_lock: Mutex<()>,
}

impl SessionRouter {
    /// 创建 SessionRouter 并从 JSON 文件恢复持久化状态
    pub fn new() -> Self {
        let persisted = load_from_file();
        let mut overrides = HashMap::new();

        for (key, record) in &persisted.overrides {
            // key 格式: "app_type:session_id"
            if let Some((app_type, session_id)) = key.split_once(':') {
                overrides.insert(
                    (app_type.to_string(), session_id.to_string()),
                    record.provider_id.clone(),
                );
            }
        }

        log::info!(
            "SessionRouter 初始化完成，从文件恢复了 {} 条 override 记录",
            overrides.len()
        );

        Self {
            sessions: RwLock::new(HashMap::new()),
            overrides: RwLock::new(overrides),
            file_lock: Mutex::new(()),
        }
    }

    /// 记录一个从请求中发现的 session（纯内存，不持久化）
    pub async fn record_session(
        &self,
        app_type: &str,
        session_id: &str,
        provider_id: &str,
        display_name: String,
        project_dir: Option<String>,
        source: SessionDiscoverySource,
    ) {
        let key = (app_type.to_string(), session_id.to_string());
        let now = now_millis();

        let info = ActiveSessionInfo {
            session_id: session_id.to_string(),
            app_type: app_type.to_string(),
            provider_id: provider_id.to_string(),
            display_name,
            project_dir,
            last_active_at: now,
            source,
        };

        let mut sessions = self.sessions.write().await;
        sessions.insert(key, info);
    }

    /// 查找 session 的覆盖 provider
    pub async fn get_override_provider(
        &self,
        app_type: &str,
        session_id: &str,
    ) -> Option<String> {
        let key = (app_type.to_string(), session_id.to_string());
        let overrides = self.overrides.read().await;
        overrides.get(&key).cloned()
    }

    /// 设置 session 的供应商覆盖（同时持久化到 JSON 文件）
    pub async fn set_override(
        &self,
        app_type: &str,
        session_id: &str,
        provider_id: &str,
    ) {
        let key = (app_type.to_string(), session_id.to_string());

        // 更新内存
        {
            let mut overrides = self.overrides.write().await;
            overrides.insert(key.clone(), provider_id.to_string());
        }

        // 同步更新 session 记录中的 provider_id
        let mut sessions = self.sessions.write().await;
        if let Some(info) = sessions.get_mut(&(app_type.to_string(), session_id.to_string())) {
            info.provider_id = provider_id.to_string();
        }
        drop(sessions);

        // 持久化到文件
        let file_key = format!("{app_type}:{session_id}");
        let display_name = {
            let sessions = self.sessions.read().await;
            sessions
                .get(&(app_type.to_string(), session_id.to_string()))
                .map(|s| s.display_name.clone())
                .unwrap_or_default()
        };
        let project_dir = {
            let sessions = self.sessions.read().await;
            sessions
                .get(&(app_type.to_string(), session_id.to_string()))
                .and_then(|s| s.project_dir.clone())
        };

        let _guard = self.file_lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut data = load_from_file();
        data.overrides.insert(
            file_key,
            PersistedOverride {
                provider_id: provider_id.to_string(),
                display_name,
                project_dir,
                pinned_at: now_millis(),
            },
        );
        save_to_file(&data);
    }

    /// 移除 session 的供应商覆盖（同时清除持久化记录）
    pub async fn remove_override(
        &self,
        app_type: &str,
        session_id: &str,
    ) -> Option<String> {
        let key = (app_type.to_string(), session_id.to_string());

        // 更新内存
        let removed = {
            let mut overrides = self.overrides.write().await;
            overrides.remove(&key)
        };

        // 同步清除持久化记录
        if removed.is_some() {
            let file_key = format!("{app_type}:{session_id}");
            let _guard = self.file_lock.lock().unwrap_or_else(|e| e.into_inner());
            let mut data = load_from_file();
            data.overrides.remove(&file_key);
            save_to_file(&data);
        }

        removed
    }

    /// 列出指定 app_type 的所有活跃 session
    pub async fn list_sessions(&self, app_type: &str) -> Vec<SessionRouteEntry> {
        let sessions = self.sessions.read().await;
        let overrides = self.overrides.read().await;

        let mut entries: Vec<SessionRouteEntry> = sessions
            .iter()
            .filter(|((at, _), _)| at == app_type)
            .map(|((_, sid), info)| {
                let key = (app_type.to_string(), sid.clone());
                let is_routed = overrides.contains_key(&key);
                let effective_provider = overrides
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| info.provider_id.clone());

                SessionRouteEntry {
                    session_id: sid.clone(),
                    app_type: app_type.to_string(),
                    provider_id: effective_provider,
                    display_name: info.display_name.clone(),
                    project_dir: info.project_dir.clone(),
                    last_active_at: info.last_active_at,
                    is_routed,
                    is_alive: is_claude_session_alive(sid),
                }
            })
            .collect();

        entries.sort_by(|a, b| b.last_active_at.cmp(&a.last_active_at));
        entries
    }

    /// 获取指定 provider 下的所有 session（供 ProviderCard 展示标签）
    pub async fn sessions_for_provider(
        &self,
        app_type: &str,
        provider_id: &str,
    ) -> Vec<ActiveSessionInfo> {
        let sessions = self.sessions.read().await;
        let overrides = self.overrides.read().await;

        sessions
            .iter()
            .filter(|((at, _), info)| {
                if at != app_type {
                    return false;
                }
                let key = (app_type.to_string(), info.session_id.clone());
                let effective = overrides
                    .get(&key)
                    .map(|s| s.as_str())
                    .unwrap_or(&info.provider_id);
                effective == provider_id
            })
            .map(|(_, info)| info.clone())
            .collect()
    }

    /// 列出所有活跃 session（供 get_status 聚合）
    ///
    /// 合并内存 session 和持久化文件中仅有 override 记录的 session，
    /// 确保重启后 override 的 session 仍能显示。
    pub async fn list_all_sessions(&self) -> Vec<ActiveSessionInfo> {
        let sessions = self.sessions.read().await;

        // 先收集内存中已有的 session
        let mut result: Vec<ActiveSessionInfo> =
            sessions.iter().map(|(_, info)| info.clone()).collect();

        // 收集内存中没有的 override session（重启后还没收到请求的 session）
        let memory_keys: std::collections::HashSet<(String, String)> =
            sessions.keys().cloned().collect();

        let persisted = load_from_file();
        for (key, record) in &persisted.overrides {
            if let Some((app_type, session_id)) = key.split_once(':') {
                let composite_key = (app_type.to_string(), session_id.to_string());
                if !memory_keys.contains(&composite_key) {
                    result.push(ActiveSessionInfo {
                        session_id: session_id.to_string(),
                        app_type: app_type.to_string(),
                        provider_id: record.provider_id.clone(),
                        display_name: if record.display_name.is_empty() {
                            session_id.chars().take(8).collect()
                        } else {
                            record.display_name.clone()
                        },
                        project_dir: record.project_dir.clone(),
                        last_active_at: record.pinned_at,
                        source: SessionDiscoverySource::Generated,
                    });
                }
            }
        }

        result
    }

    #[allow(dead_code)]
    /// 清理不活跃的 session（仅内存，持久化记录通过文件单独管理）
    pub async fn cleanup_stale(&self, max_age_ms: i64) {
        let now = now_millis();
        let mut sessions = self.sessions.write().await;

        let stale_keys: Vec<(String, String)> = sessions
            .iter()
            .filter(|(_, info)| {
                let age = now - info.last_active_at;
                let threshold = match info.source {
                    SessionDiscoverySource::Generated => max_age_ms,
                    _ => max_age_ms * 4,
                };
                age > threshold
            })
            .map(|(key, _)| key.clone())
            .collect();

        for key in &stale_keys {
            sessions.remove(key);
        }

        if !stale_keys.is_empty() {
            drop(sessions);
            let mut overrides = self.overrides.write().await;
            for key in &stale_keys {
                overrides.remove(key);
            }
        }
    }
}

/// 获取当前时间的 epoch 毫秒
fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// 从请求体中提取项目目录信息
///
/// 提取优先级：
/// 1. `metadata.cwd`（Claude Code 可能在 metadata 中发送）
/// 2. `metadata.project_dir`（其他 CLI 工具）
/// 3. None
pub fn extract_project_dir(body: &serde_json::Value) -> Option<String> {
    let metadata = body.get("metadata")?;

    metadata
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            metadata
                .get("project_dir")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
}

/// 从 Claude Code 本地 session 文件中查找项目目录
///
/// Claude Code 在 `~/.claude/sessions/<pid>.json` 中存储了每个 session 的 `cwd`。
/// 当请求体中不包含 `metadata.cwd` 时，作为 fallback 使用。
///
/// 查找方式：遍历 `~/.claude/sessions/` 目录下所有 JSON 文件，
/// 匹配 `sessionId` 字段，返回对应的 `cwd`。
pub fn find_project_dir_from_claude_sessions(session_id: &str) -> Option<String> {
    let sessions_dir = dirs::home_dir()?.join(".claude").join("sessions");
    let entries = std::fs::read_dir(&sessions_dir).ok()?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // 快速跳过不包含目标 session_id 的文件（避免完整 JSON 解析开销）
        if !content.contains(session_id) {
            continue;
        }

        let session: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if session.get("sessionId").and_then(|v| v.as_str()) == Some(session_id) {
            return session.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
        }
    }

    None
}

/// 检查 Claude Code session 对应的客户端进程是否存活
///
/// 从 `~/.claude/sessions/` 中找到匹配 sessionId 的文件，读取其 `pid` 字段，
/// 检查该 PID 进程是否仍在运行。
pub fn is_claude_session_alive(session_id: &str) -> bool {
    let sessions_dir = match dirs::home_dir() {
        Some(h) => h.join(".claude").join("sessions"),
        None => return true, // 无法判断时假设存活，避免误清理
    };
    let entries = match std::fs::read_dir(&sessions_dir) {
        Ok(e) => e,
        Err(_) => return true,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        if !content.contains(session_id) {
            continue;
        }

        let session: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if session.get("sessionId").and_then(|v| v.as_str()) == Some(session_id) {
            let pid = match session.get("pid").and_then(|v| v.as_u64()) {
                Some(p) => p,
                None => return true,
            };
            // kill -0 <pid> 不发信号，只检查进程是否存在
            return std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(true);
        }
    }

    // session 文件中找不到 → 非标准 Claude Code 客户端，假设存活
    true
}

/// 从 project_dir 生成显示名称
///
/// 取路径最后一段作为显示名；若 project_dir 为空，则用 fallback（session_id 前 8 位）。
pub fn display_name_from_project_dir(project_dir: Option<&str>, fallback: &str) -> String {
    project_dir
        .and_then(|dir| dir.rsplit('/').next())
        .filter(|name| !name.is_empty())
        .map(|name| name.to_string())
        .unwrap_or_else(|| {
            let len = 8.min(fallback.len());
            fallback[..len].to_string()
        })
}

/// 将 session.rs 的 SessionIdSource 转换为 SessionDiscoverySource
impl From<crate::proxy::session::SessionIdSource> for SessionDiscoverySource {
    fn from(src: crate::proxy::session::SessionIdSource) -> Self {
        match src {
            crate::proxy::session::SessionIdSource::Header => SessionDiscoverySource::Header,
            crate::proxy::session::SessionIdSource::MetadataUserId => {
                SessionDiscoverySource::MetadataUserId
            }
            crate::proxy::session::SessionIdSource::MetadataSessionId => {
                SessionDiscoverySource::MetadataSessionId
            }
            crate::proxy::session::SessionIdSource::Generated => {
                SessionDiscoverySource::Generated
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_name_from_project_dir() {
        assert_eq!(
            display_name_from_project_dir(Some("/home/user/my-project"), "abc123"),
            "my-project"
        );
        assert_eq!(display_name_from_project_dir(None, "abc123def"), "abc123de");
        assert_eq!(display_name_from_project_dir(Some("/"), "fallback"), "fallback");
    }

    #[test]
    fn test_extract_project_dir() {
        use serde_json::json;

        let body = json!({
            "metadata": { "cwd": "/home/user/project-a" }
        });
        assert_eq!(
            extract_project_dir(&body),
            Some("/home/user/project-a".to_string())
        );

        let body = json!({
            "metadata": { "project_dir": "/tmp/workspace" }
        });
        assert_eq!(
            extract_project_dir(&body),
            Some("/tmp/workspace".to_string())
        );

        let body = json!({ "metadata": {} });
        assert_eq!(extract_project_dir(&body), None);

        let body = json!({});
        assert_eq!(extract_project_dir(&body), None);
    }

    #[test]
    fn test_find_project_dir_from_claude_sessions() {
        // 使用实际存在的 session ID（如果 ~/.claude/sessions/ 中有对应的文件）
        // 如果环境中没有 session 文件，返回 None（不报错）
        let result = find_project_dir_from_claude_sessions("nonexistent-session-id");
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_session_router_override_flow() {
        let router = SessionRouter::new();

        router
            .record_session(
                "claude",
                "session-a",
                "provider-1",
                "project-a".to_string(),
                Some("/home/user/project-a".to_string()),
                SessionDiscoverySource::Header,
            )
            .await;

        router
            .record_session(
                "claude",
                "session-b",
                "provider-1",
                "project-b".to_string(),
                Some("/home/user/project-b".to_string()),
                SessionDiscoverySource::Header,
            )
            .await;

        assert!(router
            .get_override_provider("claude", "session-a")
            .await
            .is_none());

        router
            .set_override("claude", "session-a", "provider-2")
            .await;

        assert_eq!(
            router
                .get_override_provider("claude", "session-a")
                .await
                .as_deref(),
            Some("provider-2")
        );

        assert!(router
            .get_override_provider("claude", "session-b")
            .await
            .is_none());

        let sessions = router
            .sessions_for_provider("claude", "provider-2")
            .await;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "session-a");

        let sessions = router
            .sessions_for_provider("claude", "provider-1")
            .await;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "session-b");

        let removed = router.remove_override("claude", "session-a").await;
        assert_eq!(removed.as_deref(), Some("provider-2"));

        assert!(router
            .get_override_provider("claude", "session-a")
            .await
            .is_none());
    }

    #[tokio::test]
    async fn test_session_router_list_sessions() {
        let router = SessionRouter::new();

        router
            .record_session(
                "claude",
                "session-a",
                "provider-1",
                "project-a".to_string(),
                None,
                SessionDiscoverySource::Header,
            )
            .await;

        router
            .set_override("claude", "session-a", "provider-2")
            .await;

        let entries = router.list_sessions("claude").await;
        assert_eq!(entries.len(), 1);
        assert!(entries[0].is_routed);
        assert_eq!(entries[0].provider_id, "provider-2");
    }
}
