//! ZhiYan Tauri backend entry point.

pub mod agent;
mod credentials;
pub mod db;
pub mod scheduler;
pub mod tray;

use agent::executor::AgentExecutor;
use agent::planner::Planner;
use agent::repository::AgentRepository;
use agent::runtime::AgentRuntime;
use tauri::Manager;

/// Re-exported so the agent planner can read the LLM API key from keyring.
pub(crate) use credentials::api_key_for;

fn agent_database_path(config_dir: &std::path::Path) -> std::path::PathBuf {
    config_dir.join("zhiyan.db")
}

fn setup_error(stage: &str, error: &dyn std::fmt::Display) -> std::io::Error {
    std::io::Error::other(format!("agent {stage}: {error}"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_sql::Builder::default()
                .add_migrations("sqlite:zhiyan.db", db::migrations())
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        .on_window_event(|window, event| {
            // Close-to-hide: the tray 彻底退出 item sets EXITING so the real
            // quit path closes instead of hiding (M4 Task 1).
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if !tray::EXITING.load(std::sync::atomic::Ordering::Relaxed) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            credentials::store_api_key,
            credentials::load_api_key,
            credentials::delete_api_key,
            agent::commands::agent_health,
            agent::commands::agent_prepare_database_restore,
            agent::commands::agent_create_session,
            agent::commands::agent_create_run,
            agent::commands::agent_start_run,
            agent::commands::agent_cancel_run,
            agent::commands::agent_list_tools,
            agent::commands::agent_execute_tool,
            agent::commands::agent_decide_approval,
            agent::commands::agent_undo_tool,
            agent::commands::agent_run_planner,
            agent::commands::agent_context_audit_list,
            agent::commands::agent_memory_list,
            agent::commands::agent_memory_create,
            agent::commands::agent_memory_confirm,
            agent::commands::agent_memory_update,
            agent::commands::agent_memory_deactivate,
            agent::commands::agent_memory_delete,
        ])
        .setup(|app| {
            db::init_db(app.handle())?;
            let config_dir = app.path().app_config_dir()?;
            std::fs::create_dir_all(&config_dir)?;
            let database_path = agent_database_path(&config_dir);
            let pool = tauri::async_runtime::block_on(db::runtime::connect(&database_path))
                .map_err(|error| setup_error("database", &error))?;
            let runtime = AgentRuntime::new(
                AgentRepository::new(pool.clone()),
                AgentExecutor::new(pool.clone()),
            );
            let memory = agent::memory::MemoryRepository::new(pool.clone());
            let planner = Planner::new(pool.clone(), runtime.clone(), memory.clone());
            let context_audit = agent::context::ContextAudit::new(pool.clone());
            let scheduler = scheduler::Scheduler::new(pool);
            tauri::async_runtime::block_on(runtime.recover_interrupted())
                .map_err(|error| setup_error("recovery", &error))?;
            app.manage(runtime);
            app.manage(planner);
            app.manage(context_audit);
            app.manage(memory);
            app.manage(scheduler);
            tray::build_tray(app.handle())?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running zhiyan application");
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{agent_database_path, setup_error};

    #[test]
    fn database_setup_errors_include_stage_context() {
        let error = setup_error("database", &"sqlite unavailable");

        assert_eq!(error.to_string(), "agent database: sqlite unavailable");
    }

    #[test]
    fn recovery_setup_errors_include_stage_context() {
        let error = setup_error("recovery", &"interrupted recovery failed");

        assert_eq!(
            error.to_string(),
            "agent recovery: interrupted recovery failed"
        );
    }

    #[test]
    fn agent_database_path_is_canonicalized_under_config_dir() {
        let config_dir = Path::new(r"C:\app\config");

        assert_eq!(
            agent_database_path(config_dir),
            config_dir.join("zhiyan.db")
        );
    }

    #[test]
    fn sql_plugin_preloads_agent_database_before_setup() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let preload = config["plugins"]["sql"]["preload"]
            .as_array()
            .expect("sql preload must be configured");

        assert!(preload.iter().any(|value| value == "sqlite:zhiyan.db"));
    }
}
