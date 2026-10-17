//! Native, keyboard-accessible browser for physical Samsung firmware.
use crate::app::{self, announce, spawn};
use crate::forms::{self, Field, Form, Role};
use crate::speech::Tone;
use crate::ui::run_on_ui;
use aae_ffi::{SamsungChoice, SamsungFirmware};

pub fn show() {
    announce("Reading Samsung models…", Tone::Info);
    spawn(async {
        let result = aae_ffi::samsung_models().await;
        run_on_ui(move || match result {
            Ok(models) => choose_model(models),
            Err(e) => announce(&e.to_string(), Tone::Failure),
        });
    });
}

fn choose_model(models: Vec<SamsungChoice>) {
    let owner = app::selected().1;
    let mut query = String::new();
    loop {
        let filtered: Vec<_> = models
            .iter()
            .filter(|m| {
                m.label.to_lowercase().contains(&query.to_lowercase())
                    || m.id.to_lowercase().contains(&query.to_lowercase())
            })
            .collect();
        let answer = forms::run(owner, Form::new("Samsung Galaxy Firmware")
            .text("Firmware for physical Samsung devices. These files cannot run in AAE's emulator.")
            .field(Field::Edit { label: "Search &models".into(), value: query.clone() })
            .field(Field::Choice { label: "Samsung &model".into(), items: filtered.iter().map(|m| m.label.clone()).collect(), selected: 0 })
            .button("&Search", 2, Role::Close)
            .button("&Choose Model", 1, Role::Default)
            .button("Cancel", 0, Role::Cancel));
        match answer.button {
            2 => query = answer.values[1].text(),
            1 => {
                let Some(model) = answer.values[2]
                    .choice()
                    .and_then(|i| filtered.get(i))
                    .map(|m| (*m).clone())
                else {
                    continue;
                };
                load_countries(model);
                return;
            }
            _ => return,
        }
    }
}

fn load_countries(model: SamsungChoice) {
    announce("Reading countries and carriers…", Tone::Info);
    spawn(async move {
        let result = aae_ffi::samsung_countries(model.id.clone()).await;
        run_on_ui(move || match result {
            Ok(countries) => {
                let answer = forms::run(
                    app::selected().1,
                    Form::new(&model.label)
                        .field(Field::Choice {
                            label: "&Country or carrier".into(),
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
                        announce("No countries are listed for this model.", Tone::Info);
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
