// 智研（ZhiYan）Tauri 后端入口。
// Phase 4：注册 notification 插件 + credentials 命令（keyring 加密存 apiKey）。

mod credentials;
mod db;

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
        .invoke_handler(tauri::generate_handler![
            credentials::store_api_key,
            credentials::load_api_key,
            credentials::delete_api_key,
        ])
        .setup(|app| {
            // 确保应用数据目录存在，SQLite 数据库文件将落在此目录。
            db::init_db(app.handle())?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running zhiyan application");
}
