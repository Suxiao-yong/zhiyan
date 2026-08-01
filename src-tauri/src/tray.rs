// Tray lifecycle (M4 Task 1): closing the main window hides it, a tray menu
// controls the resident app, and 彻底退出 stops new work and exits. The
// EXITING flag is read by the window close handler in lib.rs so the real quit
// path closes instead of hiding.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::menu::{Menu, MenuItem, MenuItemKind};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

use crate::scheduler::Scheduler;

/// Set by the tray 彻底退出 item; the window close handler stops hiding once
/// this is true so the process can terminate.
pub static EXITING: AtomicBool = AtomicBool::new(false);

const MENU_OPEN: &str = "open";
const MENU_PAUSE: &str = "pause";
const MENU_TODAY: &str = "today";
const MENU_QUIT: &str = "quit";

pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, MENU_OPEN, "打开智研", true, None::<&str>)?;
    let pause = MenuItem::with_id(app, MENU_PAUSE, "暂停提醒", true, None::<&str>)?;
    let today = MenuItem::with_id(app, MENU_TODAY, "今日任务", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "彻底退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &pause, &today, &quit])?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::AssetNotFound("icon".into()))?;

    TrayIconBuilder::with_id("main-tray")
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            MENU_OPEN | MENU_TODAY => show_main_window(app),
            MENU_PAUSE => toggle_pause(app),
            MENU_QUIT => {
                EXITING.store(true, Ordering::Relaxed);
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;

    sync_pause_label(app);
    Ok(())
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Flip the settings-backed reminder pause and update the menu label.
fn toggle_pause(app: &AppHandle) {
    let scheduler = app.state::<Scheduler>();
    let scheduler = scheduler.inner().clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let currently_paused = scheduler.reminders_paused().await.unwrap_or(false);
        let paused = scheduler.set_reminders_paused(!currently_paused).await;
        let _ = paused;
        set_pause_label(&app, !currently_paused);
    });
}

/// Reflect the persisted pause state in the tray menu label.
pub fn sync_pause_label(app: &AppHandle) {
    let scheduler = app.state::<Scheduler>();
    let scheduler = scheduler.inner().clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let paused = scheduler.reminders_paused().await.unwrap_or(false);
        set_pause_label(&app, paused);
    });
}

fn set_pause_label(app: &AppHandle, paused: bool) {
    if let Some(menu) = app.menu() {
        if let Some(MenuItemKind::MenuItem(menu_item)) = menu.get(MENU_PAUSE) {
            let _ = menu_item.set_text(if paused {
                "恢复提醒"
            } else {
                "暂停提醒"
            });
        }
    }
}
