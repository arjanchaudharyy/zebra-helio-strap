// Prevents additional console window on Windows in release, DO NOT REMOVE!!
// On macOS, .app bundles have no console window by default.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // The uninstaller calls this once when "Delete app data" is checked. It deletes the tokens and exits without a window (uninstall.rs).
    if std::env::args().any(|arg| arg == zeppbridge_lib::PURGE_CREDENTIALS_ARG) {
        zeppbridge_lib::purge_credentials_for_uninstall();
        return;
    }
    // The button on the post-run rating toast: it only saves the rating and exits, without a window (feedback_toast.rs).
    if let Some(link) = std::env::args()
        .find(|arg| arg.starts_with(zeppbridge_lib::feedback_toast::FEEDBACK_LINK_PREFIX))
    {
        zeppbridge_lib::feedback_toast::handle_feedback_link(&link);
        return;
    }
    zeppbridge_lib::run()
}
