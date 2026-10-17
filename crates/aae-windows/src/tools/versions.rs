//! The Android Versions window: the installed Android versions, how much
//! space each takes, which devices use it, and a way to import an image built
//! outside Google's catalogue or delete a version no device needs. Below them,
//! whether the emulator and tools have updates.

use std::cell::RefCell;
use std::path::Path;
use std::sync::Arc;

use aae_ffi::{ImageInfo, InstalledImageInfo, RomKind, SetupStatus};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_F5, VK_R};

use crate::app::{self, Download, announce, engine, say, spawn};
use crate::forms::{self, Field, Form, Role};
use crate::panels::{self, Handler, Panel};
use crate::speech::Tone;
use crate::ui::{self, run_on_ui};

pub const KIND: &str = "versions";

const LIST: u16 = 1000;
const DELETE: u16 = 1001;
const REFRESH: u16 = 1002;
const UPDATE: u16 = 1003;
const IMPORT: u16 = 1004;

#[derive(Clone, Copy)]
struct Controls {
    status: HWND,
    list: HWND,
    tools: HWND,
    update: HWND,
}

#[derive(Default)]
struct State {
    window: Option<Controls>,
    images: Vec<InstalledImageInfo>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn row(image: &InstalledImageInfo) -> String {
    let mut parts = vec![image.size.clone()];
    if image.devices.is_empty() {
        parts.push("No AAE devices use it".into());
    } else {
        parts.push(format!("Used by {}", image.devices.join(", ")));
    }
    if !image.other_devices.is_empty() {
        parts.push(format!(
            "Also used by other emulator devices: {}",
            image.other_devices.join(", ")
        ));
    }
    format!("{}. {}.", image.description, parts.join(". "))
}

/// Opens the window, or brings it forward.
pub fn show() {
    if panels::bring_forward(KIND).is_some() {
        return;
    }
    let mut panel = Panel::new(KIND, "Android Versions", 600, 520);
    panel.text("A version can be deleted once none of AAE's devices use it. Android Studio shares these files, so its devices may use them too.");
    panel.text("Images built outside Google's catalogue, such as /e/OS, LineageOS or GrapheneOS, can be imported here.");
    let status = panel.text("Reading…");
    let list = panel.list("Installed Android versions", LIST, 10, true);
    panel.buttons(
        &[
            ("Import Image…", IMPORT),
            ("Delete…", DELETE),
            ("Refresh", REFRESH),
        ],
        None,
    );
    panel.text("Emulator and Tools");
    let tools = panel.text("Checking for updates…");
    let update = panel.buttons(&[("Update the Emulator and Tools", UPDATE)], None)[0];
    ui::enable(update, false);
    panel.shortcut(VK_F5.0, false, REFRESH);
    panel.shortcut(VK_R.0, true, REFRESH);
    STATE.with(|s| {
        s.borrow_mut().window = Some(Controls {
            status,
            list,
            tools,
            update,
        })
    });
    load();
    panel.show(Window, Some(list));
}

/// Reads the installed versions, and whether the tools have updates.
fn load() {
    let Some(engine) = engine() else { return };
    spawn(async move {
        let images = engine.installed_images().await;
        let status = engine.setup_status(false).await;
        run_on_ui(move || {
            STATE.with(|s| {
                let mut s = s.borrow_mut();
                let Some(c) = s.window else { return };
                match images {
                    Ok(images) => {
                        let selected = ui::list_selection(c.list)
                            .and_then(|i| s.images.get(i))
                            .map(|image| image.sysdir.clone());
                        let rows: Vec<String> = images.iter().map(row).collect();
                        let index = selected
                            .and_then(|d| images.iter().position(|x| x.sysdir == d))
                            .or((!images.is_empty()).then_some(0));
                        ui::set_list_items(c.list, &rows, index);
                        let text = match images.len() {
                            0 => "No Android versions are installed.".to_string(),
                            1 => "1 version.".to_string(),
                            n => format!("{n} versions."),
                        };
                        panels::set_text(c.status, &text);
                        s.images = images;
                    }
                    Err(e) => panels::set_text(c.status, &e.to_string()),
                }
                let (text, updates) = match status {
                    Ok(status) => (tools_text(&status), !status.updates.is_empty()),
                    Err(e) => (e.to_string(), false),
                };
                panels::set_text(c.tools, &text);
                ui::enable(c.update, updates);
            })
        });
    });
}

fn tools_text(status: &SetupStatus) -> String {
    let mut lines = Vec::new();
    let elsewhere = &status.managed_elsewhere;
    if !elsewhere.is_empty() {
        lines.push(format!("Managed outside AAE: {}.", elsewhere.join(", ")));
    }
    if status.updates.is_empty() {
        if elsewhere.is_empty() {
            lines.push("The emulator and tools are up to date.".into());
        }
    } else {
        let updates: Vec<String> = status
            .updates
            .iter()
            .map(|t| format!("{} {}, {}", t.name, t.revision, t.size))
            .collect();
        lines.push(format!("Updates: {}.", updates.join("; ")));
        lines.push("Stop every device before updating.".into());
    }
    lines.join("\r\n")
}

struct Window;

impl Handler for Window {
    fn command(&mut self, panel: &Panel, id: u16) {
        match id {
            REFRESH => load(),
            IMPORT => import(panel.hwnd),
            UPDATE => {
                if let Some(c) = STATE.with(|s| s.borrow().window) {
                    ui::enable(c.update, false);
                }
                app::run_setup(true);
            }
            DELETE => {
                let image = STATE.with(|s| {
                    let s = s.borrow();
                    let c = s.window?;
                    s.images.get(ui::list_selection(c.list)?).cloned()
                });
                match image {
                    Some(image) => delete(panel.hwnd, image),
                    None => announce("Select an Android version first.", Tone::Failure),
                }
            }
            _ => {}
        }
    }

    fn closed(&mut self) {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.window = None;
            s.images.clear();
        });
    }
}

/// Asks what to import, then copies an Android image from outside Google's
/// catalogue into the SDK, where New Device offers it.
fn import(owner: HWND) {
    let Some(engine) = engine() else { return };
    const CHOOSE_FILE: i32 = 2;
    const CHOOSE_FOLDER: i32 = 3;

    let images = engine.images();
    let mut base_items = vec!["No, this is a whole emulator image".to_string()];
    base_items.extend(images.iter().map(|image: &ImageInfo| image.description.clone()));

    let mut form = Form::new("Import an Android Image")
        .text(
            "Choose an emulator image you built or downloaded: a folder of its files, a zip of \
             one, as /e/OS and LineageOS's emulator builds make, or a single system image to put \
             on an installed version's files. A single phone system image may not boot: it expects \
             a phone's kernel and vendor.",
        )
        .field(Field::Edit {
            label: "The image folder, zip or system image".into(),
            value: String::new(),
        })
        .field(Field::Choice {
            label: "Which Android it is".into(),
            items: vec![
                "GrapheneOS".into(),
                "/e/OS".into(),
                "LineageOS".into(),
                "Custom image".into(),
            ],
            selected: 3,
        })
        .field(Field::Edit {
            label: "A name to show beside it, such as /e/OS 4.2 (optional)".into(),
            value: String::new(),
        })
        .field(Field::Edit {
            label: "The Android version, if the image doesn't say, such as 36 (optional)".into(),
            value: String::new(),
        })
        .field(Field::Choice {
            label: "A single system image uses the files of".into(),
            items: base_items,
            selected: 0,
        });
    form.action = Some(Box::new(|action, handle| {
        let chosen = if action == CHOOSE_FILE {
            ui::open_files(
                handle.dialog,
                "Choose an Android Image",
                &[
                    ("Android images", "*.zip;*.img;*.gz"),
                    ("All files", "*.*"),
                ],
                false,
            )
            .into_iter()
            .next()
        } else {
            ui::choose_folder(handle.dialog, "Choose the Folder of Image Files")
        };
        if let Some(path) = chosen {
            // Field 0 is the paragraph; the path is field 1.
            handle.set_text(1, &path);
            if let Some(field) = handle.control(1) {
                ui::focus(field);
            }
        }
    }));
    let answer = forms::run(
        owner,
        form.button("Choose Image File…", CHOOSE_FILE, Role::Action)
            .button("Choose Image Folder…", CHOOSE_FOLDER, Role::Action)
            .button("Import", 1, Role::Default)
            .button("Cancel", 0, Role::Cancel),
    );
    if answer.button != 1 {
        return;
    }
    let path = answer.values[1].text().trim().trim_matches('"').to_string();
    if path.is_empty() || !Path::new(&path).exists() {
        announce(
            "Choose the image's folder, its zip, or its system image first. The path must \
             exist.",
            Tone::Failure,
        );
        return;
    }
    let api = match answer.values[4].text().trim() {
        "" => None,
        text => match text.split('.').next().unwrap_or(text).parse::<u32>() {
            Ok(api) => Some(api),
            Err(_) => {
                announce(
                    "The Android version must be a number, such as 36. Leave it empty if the \
                     image says its version.",
                    Tone::Failure,
                );
                return;
            }
        },
    };
    let rom = match answer.values[2].choice() {
        Some(0) => RomKind::GrapheneOs,
        Some(1) => RomKind::Eos,
        Some(2) => RomKind::LineageOs,
        _ => RomKind::Custom,
    };
    let base = answer.values[5]
        .choice()
        .and_then(|choice| choice.checked_sub(1))
        .and_then(|index| images.get(index))
        .map(|image| image.sysdir.clone());
    let name = match answer.values[3].text().trim() {
        "" => None,
        name => Some(name.to_string()),
    };
    let what = name.clone().unwrap_or_else(|| {
        match rom {
            RomKind::GrapheneOs => "the GrapheneOS image",
            RomKind::Eos => "the /e/OS image",
            RomKind::LineageOs => "the LineageOS image",
            RomKind::Custom => "the Android image",
        }
        .to_string()
    });
    app::with(|app| {
        app.download = Some((what.clone(), 0));
        app.render();
    });
    announce(
        &format!(
            "Importing {what}. The image is copied, which can take a while. Control Shift I \
             reports progress."
        ),
        Tone::Info,
    );
    spawn(async move {
        match engine
            .import_image(path, rom, name, api, base, Arc::new(Download))
            .await
        {
            Ok(image) => say(
                format!(
                    "{} is installed. Create a device from it in New Device.",
                    image.description
                ),
                Tone::Success,
            ),
            Err(e) => say(format!("{what} wasn't imported. {e}"), Tone::Failure),
        }
        run_on_ui(|| {
            app::with(|app| {
                app.download = None;
                app.render();
            });
            load();
        });
    });
}

/// Asks, then deletes an installed Android version.
fn delete(owner: HWND, image: InstalledImageInfo) {
    let Some(engine) = engine() else { return };
    if !image.devices.is_empty() {
        announce(
            &format!(
                "{} can't be deleted while devices use it: {}. Delete those devices first.",
                image.description,
                image.devices.join(", ")
            ),
            Tone::Failure,
        );
        return;
    }
    let mut detail = format!(
        "This deletes its {} of files. You can download it again later.",
        image.size
    );
    if !image.other_devices.is_empty() {
        detail += &format!(
            " Other emulator devices, such as Android Studio's, use it and won't start without it: {}.",
            image.other_devices.join(", ")
        );
    }
    if !forms::confirm(
        owner,
        &format!("Delete {}?", image.description),
        &detail,
        "Delete",
    ) {
        return;
    }
    announce(&format!("Deleting {}.", image.description), Tone::Info);
    spawn(async move {
        match engine.remove_image(image.sysdir).await {
            Ok(freed) => say(
                format!("Deleted {}. Freed {freed}.", image.description),
                Tone::Success,
            ),
            Err(e) => say(e.to_string(), Tone::Failure),
        }
        run_on_ui(|| {
            app::with(|app| {
                app.refresh();
                app.render();
            });
            load();
        });
    });
}
