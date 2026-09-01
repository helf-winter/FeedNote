mod ai;
mod commands;
mod db;
mod error;
mod feishu_sync;
mod mobile_push;
mod models;
mod secrets;
mod vault;
mod windows_selection;

use std::{
    collections::HashMap,
    sync::{atomic::AtomicBool, Arc, Mutex},
};

use db::Database;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};

pub struct AppState {
    database: Arc<Database>,
    secrets_path: std::path::PathBuf,
    delete_confirmations: Mutex<HashMap<String, (String, i64)>>,
    pending_capture: Mutex<Option<PendingCapture>>,
    selection_suppressed: Arc<AtomicBool>,
    dock_expanded: AtomicBool,
    feishu_syncing: Arc<AtomicBool>,
    vault: Arc<vault::Vault>,
}

pub struct PendingCapture {
    snapshot: windows_selection::SelectionSnapshot,
    feed: Option<models::CreateFeedResult>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name("FeedNote")
                .arg("--autostart")
                .build(),
        )
        .setup(|app| {
            // The prototype is portable-first so user data stays beside the executable.
            let data_dir = std::env::var_os("FEEDNOTE_DATA_DIR")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| {
                    std::env::current_exe()
                        .ok()
                        .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
                        .unwrap_or_else(|| std::path::PathBuf::from("."))
                        .join("data")
                });
            std::fs::create_dir_all(&data_dir)?;
            let database = Database::open(&data_dir.join("feednote.db"))?;
            let selection_suppressed = Arc::new(AtomicBool::new(false));
            app.manage(AppState {
                database: Arc::new(database),
                secrets_path: data_dir.join("secrets.env"),
                delete_confirmations: Mutex::new(HashMap::new()),
                pending_capture: Mutex::new(None),
                selection_suppressed: selection_suppressed.clone(),
                dock_expanded: AtomicBool::new(false),
                feishu_syncing: Arc::new(AtomicBool::new(false)),
                vault: Arc::new(vault::Vault::new()),
            });

            WebviewWindowBuilder::new(
                app,
                "capture-dot",
                WebviewUrl::App("index.html?surface=capture-dot".into()),
            )
            .title("FeedNote capture")
            .inner_size(32.0, 32.0)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .shadow(false)
            .focusable(false)
            .visible(false)
            .build()?;

            WebviewWindowBuilder::new(
                app,
                "capture-menu",
                WebviewUrl::App("index.html?surface=capture-menu".into()),
            )
            .title("FeedNote selection")
            .inner_size(300.0, 238.0)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .shadow(false)
            .visible(false)
            .build()?;

            WebviewWindowBuilder::new(
                app,
                "plan-dock",
                WebviewUrl::App("index.html?surface=plan-dock".into()),
            )
            .title("FeedNote plans")
            .inner_size(58.0, 58.0)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .shadow(false)
            .disable_drag_drop_handler()
            .build()?;

            if let Some(main) = app.get_webview_window("main") {
                let main_for_event = main.clone();
                main.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = main_for_event.hide();
                    }
                });
            }
            commands::initialize_plan_dock(app.handle());
            windows_selection::start_watcher(app.handle().clone(), selection_suppressed);
            let state = app.state::<AppState>();
            mobile_push::start_scheduler(state.database.clone(), state.secrets_path.clone());
            feishu_sync::start_scheduler(
                state.database.clone(),
                state.secrets_path.clone(),
                state.feishu_syncing.clone(),
                app.handle().clone(),
                state.vault.clone(),
            );
            let open_item = MenuItem::with_id(app, "open", "打开 FeedNote", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open_item, &quit_item])?;
            app.on_menu_event(|app, event| match event.id.as_ref() {
                "open" => show_main_window(app),
                "quit" => app.exit(0),
                _ => {}
            });
            let icon_rgba = make_tray_icon();
            TrayIconBuilder::new()
                .menu(&menu)
                .show_menu_on_left_click(false)
                .tooltip("FeedNote")
                .icon(tauri::image::Image::new_owned(icon_rgba, 32, 32))
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button,
                        button_state,
                        ..
                    } = event
                    {
                        if !tray_click_opens_main(button, button_state) {
                            return;
                        }
                        show_main_window(tray.app_handle());
                    }
                })
                .build(app)?;

            let shortcut = Shortcut::new(Some(Modifiers::ALT | Modifiers::SHIFT), Code::Space);
            app.global_shortcut()
                .on_shortcut(shortcut, |app, _shortcut, _event| {
                    show_main_window(app);
                })?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::create_feed,
            commands::list_feeds,
            commands::list_memories,
            commands::get_memory,
            commands::list_reviews,
            commands::resolve_review,
            commands::request_delete_feed,
            commands::delete_feed,
            commands::get_stats,
            commands::get_settings,
            commands::update_settings,
            commands::check_ai,
            commands::process_feed,
            commands::prepare_capture,
            commands::prepare_drag_capture,
            commands::discard_capture,
            commands::get_capture_preview,
            commands::list_memos,
            commands::update_memo,
            commands::record_memo_capture,
            commands::get_vault_status,
            commands::initialize_vault,
            commands::unlock_vault,
            commands::lock_vault,
            commands::list_secret_items,
            commands::update_secret_item,
            commands::delete_secret_item,
            commands::stash_capture,
            commands::undo_secret_stash,
            commands::commit_capture,
            commands::resolve_plan_time,
            commands::list_plans,
            commands::update_plan,
            commands::set_plan_done,
            commands::toggle_plan_dock,
            commands::show_plan_dock_menu,
            commands::open_main_window,
            commands::open_external_link,
            commands::test_mobile_push,
            commands::get_feishu_sync_status,
            commands::sync_feishu_now,
            commands::get_feishu_secret_status,
            commands::sync_feishu_secrets_now,
            commands::get_feishu_memo_status,
            commands::sync_feishu_memos_now,
            commands::get_feishu_source_status,
            commands::sync_feishu_source_now,
            commands::export_data,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run FeedNote");
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn tray_click_opens_main(button: MouseButton, state: MouseButtonState) -> bool {
    button == MouseButton::Left && state == MouseButtonState::Up
}

fn make_tray_icon() -> Vec<u8> {
    let mut pixels = vec![0_u8; 32 * 32 * 4];
    for y in 0..32 {
        for x in 0..32 {
            let index = ((y * 32 + x) * 4) as usize;
            let in_page = (5..=26).contains(&x) && (3..=28).contains(&y);
            let in_fold = x >= 21 && y <= 8 && x + y >= 29;
            let in_line = (9..=22).contains(&x) && [12, 17, 22].contains(&y);
            let color = if in_line {
                [255, 255, 255, 255]
            } else if in_page && !in_fold {
                [28, 126, 117, 255]
            } else {
                [0, 0, 0, 0]
            };
            pixels[index..index + 4].copy_from_slice(&color);
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_released_left_tray_click_opens_the_main_window() {
        assert!(tray_click_opens_main(
            MouseButton::Left,
            MouseButtonState::Up
        ));
        assert!(!tray_click_opens_main(
            MouseButton::Right,
            MouseButtonState::Up
        ));
        assert!(!tray_click_opens_main(
            MouseButton::Left,
            MouseButtonState::Down
        ));
    }
}
