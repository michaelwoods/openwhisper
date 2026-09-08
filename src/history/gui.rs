use anyhow::Result;
use chrono::{DateTime, Local};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::gui::{HistoryItem, HistoryWindow};
use crate::history::{HistoryEntry, HistoryManager};
use crate::output::clipboard::set_clipboard;

struct ActivePlayback {
    id: i32,
    stop_flag: Arc<AtomicBool>,
}

fn show_toast(win: &HistoryWindow, msg: &str, duration_secs: u64) {
    win.set_toast_text(SharedString::from(msg));
    let win_weak = win.as_weak();
    let current_msg = msg.to_string();
    slint::Timer::single_shot(Duration::from_secs(duration_secs), move || {
        if let Some(w) = win_weak.upgrade()
            && w.get_toast_text().as_str() == current_msg
        {
            w.set_toast_text(SharedString::default());
        }
    });
}

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
        .is_some_and(|p| std::path::Path::new(p).exists());

    let meta = format!(
        "{:.1}s • {} chars • {}{}",
        entry.duration_secs,
        entry.char_count,
        if entry.model.is_empty() {
            "whisper"
        } else {
            &entry.model
        },
        if has_audio { " • Audio" } else { "" }
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

    let active_playback: Arc<Mutex<Option<ActivePlayback>>> = Arc::new(Mutex::new(None));

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
        let window_weak = window.as_weak();
        window.on_search_changed(move |q| {
            if let Some(win) = window_weak.upgrade() {
                win.set_toast_text(SharedString::default());
            }
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
                show_toast(&win, "History refreshed", 3);
            }
        });
    }

    // Wire play-item (toggles between Play and Stop)
    {
        let mgr = Arc::clone(&history_mgr);
        let window_weak = window.as_weak();
        let active_pb = Arc::clone(&active_playback);
        window.on_play_item(move |id| {
            let Some(win) = window_weak.upgrade() else {
                return;
            };

            // Check if this item is currently playing: if so, stop it!
            if let Some(active) = active_pb.lock().unwrap().take() {
                active.stop_flag.store(true, Ordering::SeqCst);
                if active.id == id {
                    win.set_playing_id(-1);
                    show_toast(&win, "Playback stopped", 2);
                    return;
                }
            }

            // Start playing the selected item
            if let Ok(Some(entry)) = mgr.get_by_id(id as i64) {
                if let Some(ref path_str) = entry.audio_path {
                    let path = std::path::Path::new(path_str);
                    if path.exists() {
                        let stop_flag = Arc::new(AtomicBool::new(false));
                        *active_pb.lock().unwrap() = Some(ActivePlayback {
                            id,
                            stop_flag: Arc::clone(&stop_flag),
                        });

                        win.set_playing_id(id);
                        show_toast(&win, &format!("▶ Playing audio for #{}...", id), 4);

                        let path_buf = path.to_path_buf();
                        let win_weak_bg = window_weak.clone();
                        let active_pb_bg = Arc::clone(&active_pb);

                        std::thread::spawn(move || {
                            let completed =
                                crate::audio::play_wav_file_cancellable(&path_buf, stop_flag)
                                    .unwrap_or(false);

                            let _ = win_weak_bg.upgrade_in_event_loop(move |win| {
                                let mut pb = active_pb_bg.lock().unwrap();
                                let is_current = pb.as_ref().is_some_and(|a| a.id == id);
                                if is_current {
                                    *pb = None;
                                    win.set_playing_id(-1);
                                    if completed {
                                        win.set_toast_text(SharedString::default());
                                    }
                                }
                            });
                        });
                    } else {
                        win.set_playing_id(-1);
                        show_toast(&win, "Audio recording file not found on disk", 4);
                    }
                } else {
                    win.set_playing_id(-1);
                    show_toast(&win, "No audio recording saved for this entry", 4);
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
                    show_toast(&win, &format!("Clipboard copy failed: {}", e), 4);
                } else {
                    show_toast(&win, "Copied to clipboard!", 3);
                }
            }
        });
    }

    // Wire delete-item
    {
        let mgr = Arc::clone(&history_mgr);
        let window_weak = window.as_weak();
        let reload_cb = reload.clone();
        let active_pb = Arc::clone(&active_playback);
        window.on_delete_item(move |id| {
            let Some(win) = window_weak.upgrade() else {
                return;
            };

            let mut pb = active_pb.lock().unwrap();
            if let Some(active) = pb.as_ref()
                && active.id == id
            {
                active.stop_flag.store(true, Ordering::SeqCst);
                *pb = None;
                win.set_playing_id(-1);
            }
            drop(pb);

            if let Ok(deleted) = mgr.delete(id as i64)
                && deleted
            {
                show_toast(&win, "Entry deleted", 3);
                let q = win.get_search_query();
                reload_cb(q.as_str());
            }
        });
    }

    // Wire clear-history
    {
        let mgr = Arc::clone(&history_mgr);
        let window_weak = window.as_weak();
        let reload_cb = reload.clone();
        let active_pb = Arc::clone(&active_playback);
        window.on_clear_history(move || {
            let Some(win) = window_weak.upgrade() else {
                return;
            };

            if let Some(active) = active_pb.lock().unwrap().take() {
                active.stop_flag.store(true, Ordering::SeqCst);
                win.set_playing_id(-1);
            }

            if let Ok(()) = mgr.clear_all() {
                show_toast(&win, "All history cleared (saved recordings preserved)", 4);
                reload_cb("");
            }
        });
    }

    // Wire close-window
    {
        let window_weak = window.as_weak();
        let active_pb = Arc::clone(&active_playback);
        window.on_close_window(move || {
            if let Some(active) = active_pb.lock().unwrap().take() {
                active.stop_flag.store(true, Ordering::SeqCst);
            }
            if let Some(win) = window_weak.upgrade() {
                win.set_playing_id(-1);
                let _ = win.hide();
            }
        });
    }

    window.run()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entry_to_item_formatting() {
        let entry = HistoryEntry {
            id: Some(123),
            timestamp: "2026-09-07T22:00:00Z".to_string(),
            text: "Hello world dictation".to_string(),
            raw_text: None,
            duration_secs: 1.45,
            char_count: 21,
            model: "whisper-tiny".to_string(),
            output_mode: "Paste".to_string(),
            audio_path: None,
        };
        let item = entry_to_item(&entry);
        assert_eq!(item.id, 123);
        assert_eq!(item.text.as_str(), "Hello world dictation");
        assert!(!item.has_audio);
        assert!(item.meta_info.contains("1.5s"));
        assert!(item.meta_info.contains("21 chars"));
        assert!(item.meta_info.contains("whisper-tiny"));
        assert!(!item.meta_info.contains("Audio"));

        let ts = format_timestamp("2026-09-07T22:00:00Z");
        assert!(!ts.is_empty());
    }
}
