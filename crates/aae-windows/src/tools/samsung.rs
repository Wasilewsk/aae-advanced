//! Native, keyboard-accessible browser for physical Samsung firmware.
use crate::app::{self, announce, spawn};
use crate::forms::{self, Field, Form, Role};
use crate::speech::Tone;
use crate::ui::run_on_ui;
use aae_ffi::{SamsungChoice, SamsungFirmware};

pub fn show() {
    let answer = forms::run(app::selected().1, Form::new("Samsung Galaxy Firmware — UK")
        .text("Only models with verified United Kingdom firmware are shown. Enter a model number or a specific phone name, such as SM-G781B or Galaxy S20 FE.")
        .field(Field::Edit { label: "Search &models".into(), value: String::new() })
        .button("&Search UK Models", 1, Role::Default).button("Cancel", 0, Role::Cancel));
    if answer.button == 1 {
        search(answer.values[1].text());
    }
}

fn search(query: String) {
    announce("Checking UK Samsung models…", Tone::Info);
    spawn(async move {
        let result = aae_ffi::samsung_models(query.clone()).await;
        run_on_ui(move || match result {
            Ok(models) => choose_model(models, query),
            Err(e) => announce(&e.to_string(), Tone::Failure),
        });
    });
}

fn choose_model(models: Vec<SamsungChoice>, query: String) {
    let owner = app::selected().1;
    let answer = forms::run(owner, Form::new("Samsung Galaxy Firmware — UK")
        .text(if models.is_empty() { "No matching UK models were found. Try a different model name or number." } else { "Verified UK variants. Firmware is for physical Samsung devices; it requires device-specific hardware support to run in QEMU." })
        .field(Field::Edit { label: "Search &models".into(), value: query })
        .field(Field::Choice { label: "UK Samsung &model".into(), items: models.iter().map(|m| m.label.clone()).collect(), selected: 0 })
        .button("&Search UK Models", 2, Role::Close)
        .button("&Choose Model", 1, Role::Default)
        .button("Cancel", 0, Role::Cancel));
    match answer.button {
        2 => search(answer.values[1].text()),
        1 => {
            if let Some(model) = answer.values[2]
                .choice()
                .and_then(|i| models.get(i))
                .cloned()
            {
                load_countries(model);
            }
        }
        _ => {}
    }
}

fn load_countries(model: SamsungChoice) {
    announce("Reading UK carriers and product codes…", Tone::Info);
    spawn(async move {
        let result = aae_ffi::samsung_countries(model.id.clone()).await;
        run_on_ui(move || match result {
            Ok(countries) => {
                let answer = forms::run(
                    app::selected().1,
                    Form::new(&model.label)
                        .field(Field::Choice {
                            label: "UK &carrier or product code".into(),
                            items: countries.iter().map(|c| c.label.clone()).collect(),
                            selected: 0,
                        })
                        .button("Show &Firmware", 1, Role::Default)
                        .button("Cancel", 0, Role::Cancel),
                );
                if answer.button == 1 {
                    if let Some(country) = answer.values[0].choice().and_then(|i| countries.get(i))
                    {
                        load_firmware(model.id, country.id.clone(), 1, Vec::new());
                    } else {
                        announce(
                            "No UK firmware regions are listed for this model.",
                            Tone::Info,
                        );
                    }
                }
            }
            Err(e) => announce(&e.to_string(), Tone::Failure),
        });
    });
}

fn load_firmware(model: String, country: String, page: u32, mut firmware: Vec<SamsungFirmware>) {
    announce("Reading firmware versions…", Tone::Info);
    spawn(async move {
        let result = aae_ffi::samsung_firmwares(model.clone(), country.clone(), page).await;
        run_on_ui(move || match result {
            Ok(items) => {
                let before = firmware.len();
                for item in items {
                    if !firmware.iter().any(|f| f.url == item.url) {
                        firmware.push(item);
                    }
                }
                let has_more = firmware.len() > before;
                loop {
                    let mut form = Form::new(&format!("Samsung firmware: {model}, {country}"))
                        .text("Versions and build codes are shown as SamMobile supplies them. Downloads open on SamMobile in your browser.")
                        .field(Field::Choice { label: "Firmware &version".into(), items: firmware.iter().map(|f| format!("{}, {}, {}, {}", f.version, f.build, f.country, f.product_code)).collect(), selected: 0 })
                        .button("&View Download", 1, Role::Default);
                    if has_more {
                        form = form.button("Load &More", 2, Role::Close);
                    }
                    let answer =
                        forms::run(app::selected().1, form.button("Close", 0, Role::Cancel));
                    match answer.button {
                        1 => {
                            if let Some(item) =
                                answer.values[1].choice().and_then(|i| firmware.get(i))
                            {
                                // The shared API validates this URL before returning it.
                                let url = crate::ui::wide(&item.url);
                                unsafe {
                                    windows::Win32::UI::Shell::ShellExecuteW(
                                        None,
                                        windows::core::w!("open"),
                                        windows::core::PCWSTR(url.as_ptr()),
                                        None,
                                        None,
                                        windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
                                    );
                                }
                            }
                        }
                        2 if has_more => {
                            load_firmware(model, country, page + 1, firmware);
                            return;
                        }
                        _ => return,
                    }
                }
            }
            Err(e) => announce(&e.to_string(), Tone::Failure),
        });
    });
}
