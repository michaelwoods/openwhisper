use std::rc::Rc;
use std::sync::Arc;
use anyhow::Result;
use chrono::{DateTime, Local};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::gui::{HistoryItem, HistoryWindow};
use crate::history::{HistoryEntry, HistoryManager};
use crate::output::clipboard::set_clipboard;

fn format_timestamp(iso_str: &str) -> String {
    if let Ok(dt) = DateTime::parse_from_rfc3339(iso_str) {
        let local_dt: DateTime<Local> = DateTime::from(dt);
        local_dt.format("%Y-%m-%d %H:%M:%S").to_string()
    } else {
        iso_str.to_string()
    }
}

fn entry_to_item(entry: &HistoryEntry) -> HistoryItem {
    let ts = format_timestamp(&entry.timestamp);
    let has_audio = entry
        .audio_path
        .as_ref()
        .map_or(false, |p| std::path::Path::new(p).exists());

    let meta = format!(
        "{:.1}s • {} chars • {}{}",
        entry.duration_secs,
        entry.char_count,
        if entry.model.is_empty() {
            "whisper"
        } else {
            &entry.model
        },
        if has_audio { " • 🔊 Audio" } else { "" }
    );

    HistoryItem {
        id: entry.id.unwrap_or(0) as i32,
        timestamp: SharedString::from(ts),
        text: SharedString::from(entry.text.clone()),
        meta_info: SharedString::from(meta),
        has_audio,
    }
}

/// Runs the standalone Slint History GUI window.
pub fn run_history_gui(history_mgr: Arc<HistoryManager>) -> Result<()> {
    let window = HistoryWindow::new()?;
    let items_model = Rc::new(VecModel::<HistoryItem>::default());
    window.set_history_items(ModelRc::new(items_model.clone()));

    let reload = {
        let window_weak = window.as_weak();
        let mgr = Arc::clone(&history_mgr);
        let items_model = items_model.clone();
        move |query: &str| {
            let Some(win) = window_weak.upgrade() else {
                return;
            };
            let entries = if query.trim().is_empty() {
                mgr.list(100, 0).unwrap_or_default()
            } else {
                mgr.search(query.trim(), 100).unwrap_or_default()
            };

            let total_count = mgr.count().unwrap_or(0);
            let showing_count = entries.len();

            let items: Vec<HistoryItem> = entries.iter().map(entry_to_item).collect();
            items_model.set_vec(items);

            win.set_is_empty(showing_count == 0);
            if query.trim().is_empty() {
                win.set_summary_text(SharedString::from(format!(
                    "Showing {} of {} total dictations",
                    showing_count, total_count
                )));
            } else {
                win.set_summary_text(SharedString::from(format!(
                    "Found {} matches (out of {} total)",
                    showing_count, total_count
                )));
            }
        }
    };

    // Initial load
    reload("");

    // Wire search-changed
    {
        let reload_cb = reload.clone();
        window.on_search_changed(move |q| {
            reload_cb(q.as_str());
        });
    }

    // Wire refresh
    {
        let reload_cb = reload.clone();
        let window_weak = window.as_weak();
        window.on_refresh(move || {
            if let Some(win) = window_weak.upgrade() {
                let q = win.get_search_query();
                reload_cb(q.as_str());
                win.set_toast_text(SharedString::from("History refreshed"));
            }
        });
    }

    // Wire play-item
    {
        let mgr = Arc::clone(&history_mgr);
        let window_weak = window.as_weak();
        window.on_play_item(move |id| {
            let Some(win) = window_weak.upgrade() else {
                return;
            };
            if let Ok(Some(entry)) = mgr.get_by_id(id as i64) {
                if let Some(ref path_str) = entry.audio_path {
                    let path = std::path::Path::new(path_str);
                    if path.exists() {
                        if let Err(e) = crate::audio::play_wav_file(path) {
                            win.set_toast_text(SharedString::from(format!("Playback error: {}", e)));
                        } else {
                            win.set_toast_text(SharedString::from(format!("Playing recording audio for #{}...", id)));
                        }
                    } else {
                        win.set_toast_text(SharedString::from("Audio recording file not found on disk"));
                    }
                } else {
                    win.set_toast_text(SharedString::from("No audio recording saved for this entry"));
                }
            }
        });
    }

    // Wire copy-item
    {
        let mgr = Arc::clone(&history_mgr);
        let window_weak = window.as_weak();
        window.on_copy_item(move |id| {
            let Some(win) = window_weak.upgrade() else {
                return;
            };
            if let Ok(Some(entry)) = mgr.get_by_id(id as i64) {
                if let Err(e) = set_clipboard(&entry.text) {
                    win.set_toast_text(SharedString::from(format!("Clipboard copy failed: {}", e)));
                } else {
                    win.set_toast_text(SharedString::from("Copied to clipboard!"));
                }
            }
        });
    }

    // Wire delete-item
    {
        let mgr = Arc::clone(&history_mgr);
        let window_weak = window.as_weak();
        let reload_cb = reload.clone();
        window.on_delete_item(move |id| {
            let Some(win) = window_weak.upgrade() else {
                return;
            };
            if let Ok(deleted) = mgr.delete(id as i64) {
                if deleted {
                    win.set_toast_text(SharedString::from("Entry deleted"));
                    let q = win.get_search_query();
                    reload_cb(q.as_str());
                }
            }
        });
    }

    // Wire clear-history
    {
        let mgr = Arc::clone(&history_mgr);
        let window_weak = window.as_weak();
        let reload_cb = reload.clone();
        window.on_clear_history(move || {
            let Some(win) = window_weak.upgrade() else {
                return;
            };
            if let Ok(()) = mgr.clear_all() {
                win.set_toast_text(SharedString::from("All history cleared"));
                reload_cb("");
            }
        });
    }

    // Wire close-window
    {
        let window_weak = window.as_weak();
        window.on_close_window(move || {
            if let Some(win) = window_weak.upgrade() {
                let _ = win.hide();
            }
        });
    }

    window.run()?;
    Ok(())
}
