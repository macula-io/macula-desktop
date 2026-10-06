//! Tray mode: closing the window hides it, so the Rust core keeps its
//! mesh presence (the pool, the `agent.hello` heartbeat, joined rooms)
//! while no window is open. The tray icon brings the window back or
//! quits the app for real.
//!
//! Linux tray hosts (appindicator) deliver no click events, only the
//! menu, so "Show" lives in the menu on every OS; a left click shows the
//! window too where the OS reports one.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime, Window, WindowEvent};

const MENU_SHOW: &str = "show";
const MENU_QUIT: &str = "quit";
const MAIN_WINDOW: &str = "main";

/// What a tray menu entry asks the app to do.
#[derive(Debug, PartialEq, Eq)]
pub enum TrayAction {
    ShowWindow,
    Quit,
    Ignore,
}

/// action_for_menu maps a tray menu entry's id to its action.
pub fn action_for_menu(id: &str) -> TrayAction {
    match id {
        MENU_SHOW => TrayAction::ShowWindow,
        MENU_QUIT => TrayAction::Quit,
        _ => TrayAction::Ignore,
    }
}

/// install puts the tray icon and its menu in place.
pub fn install<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, MENU_SHOW, "Show macula-desktop", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let mut tray = TrayIconBuilder::with_id("macula-desktop")
        .tooltip("macula-desktop: on the mesh")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| run_action(app, action_for_menu(event.id.as_ref())))
        .on_tray_icon_event(|tray, event| on_icon_event(tray.app_handle(), event));
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// hide_on_close keeps the app running when the window is closed (the
/// titlebar's close button, Alt+F4, the window manager): the window
/// hides and the mesh link stays up. Quit is the tray menu's.
pub fn hide_on_close<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        api.prevent_close();
        window.hide().ok();
    }
}

fn on_icon_event<R: Runtime>(app: &AppHandle<R>, event: TrayIconEvent) {
    let is_left_release = matches!(
        event,
        TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        }
    );
    if is_left_release {
        run_action(app, TrayAction::ShowWindow);
    }
}

fn run_action<R: Runtime>(app: &AppHandle<R>, action: TrayAction) {
    match action {
        TrayAction::ShowWindow => show_main_window(app),
        TrayAction::Quit => app.exit(0),
        TrayAction::Ignore => {}
    }
}

fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };
    window.show().ok();
    window.unminimize().ok();
    window.set_focus().ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_shows_the_window_or_quits() {
        assert_eq!(action_for_menu("show"), TrayAction::ShowWindow);
        assert_eq!(action_for_menu("quit"), TrayAction::Quit);
    }

    #[test]
    fn an_unknown_menu_entry_does_nothing() {
        assert_eq!(action_for_menu("close"), TrayAction::Ignore);
    }
}
