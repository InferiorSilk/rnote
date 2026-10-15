// Imports
use crate::workspacebrowser::widgethelper;
use crate::{RnAppWindow, RnWorkspaceBrowser};
use gettextrs::gettext;
use gtk4::{Align, Entry, Label, gio, glib, glib::clone, pango, prelude::*};
use rnote_engine::fileformats::{FileFormatSaver, rnoteformat};
use rnote_engine::EngineSnapshot;
use std::io::Write;
use tracing::{debug, warn};

/// Create a new `create_document` action.
pub(crate) fn create_document(
    workspacebrowser: &RnWorkspaceBrowser,
    appwindow: &RnAppWindow,
) -> gio::SimpleAction {
    let new_document_action = gio::SimpleAction::new("create-document", None);

    new_document_action.connect_activate(clone!(
        #[weak]
        workspacebrowser,
        #[weak]
        appwindow,
        move |_, _| {
            if let Some(parent_path) = workspacebrowser.dir_list_file().and_then(|f| f.path()) {
                let document_name_entry = create_document_name_entry();
                let dialog_title_label = create_dialog_title_label();
                let (apply_button, popover) =
                    widgethelper::create_entry_dialog(&document_name_entry, &dialog_title_label);

                // at first don't allow applying, since the user did not enter any text yet.
                apply_button.set_sensitive(false);

                workspacebrowser.dir_controls_actions_box().append(&popover);

                document_name_entry.connect_changed(clone!(
                    #[weak]
                    apply_button,
                    #[strong]
                    parent_path,
                    move |entry| {
                        let entry_text = entry.text();
                        let new_document_path = parent_path.join(format!("{}.rnote", entry_text));

                        if new_document_path.exists() || entry_text.is_empty() {
                            apply_button.set_sensitive(false);
                            entry.add_css_class("error");
                        } else {
                            // Only allow creating valid document names
                            apply_button.set_sensitive(true);
                            entry.remove_css_class("error");
                        }
                    }
                ));

                apply_button.connect_clicked(clone!(
                    #[weak]
                    popover,
                    #[weak]
                    document_name_entry,
                    #[weak]
                    appwindow,
                    #[weak]
                    workspacebrowser,
                    move |_| {
                        let document_name = document_name_entry.text();
                        let new_document_path = parent_path.join(format!("{}.rnote", document_name.as_str()));

                        if new_document_path.exists() {
                            // Should have been caught earlier, but making sure
                            appwindow
                                .overlays()
                                .dispatch_toast_error("Can't create document that already exists.");
                            debug!(
                                "Couldn't create new document with name `{}`, it already exists.",
                                document_name.as_str()
                            );
                        } else {
                            // Create a minimal valid .rnote file with default document config preset
                            let doc_config_preset = appwindow.document_config_preset_ref().clone();
                            let mut snapshot = EngineSnapshot::default();
                            snapshot.document.config = doc_config_preset;
                            
                            match save_snapshot_to_path(&snapshot, &new_document_path) {
                                Ok(_) => {
                                    // Refresh the directory list to show the new file
                                    workspacebrowser.refresh_dir_list_selected_workspace();
                                    
                                    // Find the newly created file and trigger rename mode
                                    if let Some(file) = workspacebrowser.dir_list_file() {
                                        if let Some(dir_path) = file.path() {
                                            let new_file_gio = gio::File::for_path(&new_document_path);
                                            
                                            // Wait a bit for the directory list to refresh, then select and rename
                                            glib::spawn_future_local(clone!(
                                                #[weak]
                                                workspacebrowser,
                                                #[weak]
                                                appwindow,
                                                async move {
                                                    // Small delay to allow directory list to refresh
                                                    glib::timeout_future(std::time::Duration::from_millis(100)).await;
                                                    
                                                    // Try to find and select the new file row
                                                    if let Some(model) = workspacebrowser.imp().list_selection_model.model() {
                                                        let n_items = model.n_items();
                                                        for i in 0..n_items {
                                                            if let Some(item) = model.item(i) {
                                                                if let Some(file_info) = item.downcast_ref::<gio::FileInfo>() {
                                                                    if let Some(attr_file) = file_info.attribute_object("standard::file") {
                                                                        if let Some(item_file) = attr_file.downcast_ref::<gio::File>() {
                                                                            if item_file.equal(&new_file_gio) {
                                                                                workspacebrowser.files_list_set_selected(Some(i));
                                                                                
                                                                                // Trigger rename on the file row
                                                                                if let Some(selected_row) = workspacebrowser
                                                                                    .files_scroller()
                                                                                    .first_child()
                                                                                    .and_then(|w| w.downcast::<gtk4::ListView>().ok())
                                                                                    .and_then(|lv| lv.widget_at_position(0.0, 0.0))
                                                                                    .and_then(|w| w.downcast::<crate::RnFileRow>().ok())
                                                                                {
                                                                                    selected_row.start_rename();
                                                                                }
                                                                                break;
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            ));
                                        }
                                    }
                                }
                                Err(e) => {
                                    appwindow
                                        .overlays()
                                        .dispatch_toast_error("Creating new document failed");
                                    debug!("Couldn't create document, Err: {e:?}");
                                }
                            }

                            popover.popdown();
                        }
                    }
                ));

                popover.popup();
            } else {
                warn!("Can't create new document when there currently is no workspace selected");
            }
        }
    ));

    new_document_action
}

fn save_snapshot_to_path(snapshot: &EngineSnapshot, path: &std::path::Path) -> anyhow::Result<()> {
    let rnote_file = rnoteformat::RnoteFile {
        engine_snapshot: ijson::to_value(snapshot)?,
    };
    
    let bytes = rnote_file.save_as_bytes(path.file_name().unwrap().to_str().unwrap())?;
    
    let mut file = std::fs::File::create(path)?;
    file.write_all(&bytes)?;
    
    Ok(())
}

fn create_document_name_entry() -> Entry {
    Entry::builder()
        .placeholder_text(gettext("Document Name"))
        .text("New Document")
        .build()
}

fn create_dialog_title_label() -> Label {
    let label = Label::builder()
        .margin_bottom(12)
        .halign(Align::Center)
        .label(gettext("New Document"))
        .width_chars(24)
        .ellipsize(pango::EllipsizeMode::End)
        .build();
    label.add_css_class("title-4");
    label
}
