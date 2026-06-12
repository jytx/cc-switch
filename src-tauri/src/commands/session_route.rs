//! Session 级供应商路由命令
//!
//! 提供前端管理 session 路由覆盖的 Tauri 命令。

use crate::proxy::session_router::{ActiveSessionInfo, SessionRouteEntry};
use crate::store::AppState;
use tauri_plugin_opener::OpenerExt;

/// 列出指定应用的所有活跃 session 路由
#[tauri::command]
pub async fn list_session_routes(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<Vec<SessionRouteEntry>, String> {
    state
        .proxy_service
        .list_session_routes(&app_type)
        .await
        .map_err(|e| e.to_string())
}

/// 设置 session 的供应商覆盖
#[tauri::command]
pub async fn set_session_route(
    state: tauri::State<'_, AppState>,
    app_type: String,
    session_id: String,
    provider_id: String,
) -> Result<(), String> {
    state
        .proxy_service
        .set_session_route(&app_type, &session_id, &provider_id)
        .await
        .map_err(|e| e.to_string())
}

/// 移除 session 的供应商覆盖（恢复全局默认）
#[tauri::command]
pub async fn remove_session_route(
    state: tauri::State<'_, AppState>,
    app_type: String,
    session_id: String,
) -> Result<(), String> {
    state
        .proxy_service
        .remove_session_route(&app_type, &session_id)
        .await
        .map_err(|e| e.to_string())
}

/// 获取指定 provider 下的活跃 session 列表（供 ProviderCard 展示）
#[tauri::command]
pub async fn get_provider_sessions(
    state: tauri::State<'_, AppState>,
    app_type: String,
    provider_id: String,
) -> Result<Vec<ActiveSessionInfo>, String> {
    state
        .proxy_service
        .get_provider_sessions(&app_type, &provider_id)
        .await
        .map_err(|e| e.to_string())
}

/// 在 Finder 中打开指定路径
#[tauri::command]
pub async fn open_path_in_finder(
    app: tauri::AppHandle,
    path: String,
) -> Result<(), String> {
    app.opener()
        .open_path(&path, None::<String>)
        .map_err(|e| format!("打开路径失败: {e}"))
}
