//! A device's accessibility services: which are installed and on, turning
//! any of them on or off, and switching screen readers. Names and
//! descriptions come from AAE's helper, as Android's settings show them.

use serde::Deserialize;

use crate::adb::{Adb, same_component};
use crate::device::Device;
use crate::error::{Error, Result};
use crate::provision::SERVICE_TIMEOUT;

const HELPER_RECEIVER: &str = "io.github.aaron_gh.aae.helper/.CommandReceiver";
const LIST_SERVICES: &str = "io.github.aaron_gh.aae.helper.LIST_SERVICES";

/// An installed accessibility service.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Service {
    /// As `package/class`.
    pub component: String,
    /// The name people see, such as "TalkBack".
    pub label: String,
    #[serde(default)]
    pub description: String,
    /// Requests touch exploration (a screen reader).
    #[serde(default)]
    pub screen_reader: bool,
    /// Turned on in Android's settings.
    #[serde(skip)]
    pub on: bool,
    /// The device's screen reader, as AAE keeps it.
    #[serde(skip)]
    pub current_screen_reader: bool,
}

impl Service {
    pub fn package(&self) -> &str {
        self.component.split('/').next().unwrap_or(&self.component)
    }
}

/// Every installed accessibility service, screen readers first, then by name.
pub async fn list(adb: &Adb, device: &Device) -> Result<Vec<Service>> {
    let out = adb
        .shell(&format!(
            "am broadcast -n {HELPER_RECEIVER} -a {LIST_SERVICES}"
        ))
        .await?;
    if !out.contains("result=1") {
        return Err(Error::Adb(
            "AAE's helper didn't answer. Starting the device again updates it.".into(),
        ));
    }
    let json = out
        .split_once("data=\"")
        .and_then(|(_, rest)| rest.rsplit_once('"'))
        .map(|(json, _)| json)
        .ok_or_else(|| Error::Adb("AAE's helper sent no answer.".into()))?;
    let mut services: Vec<Service> = serde_json::from_str(json)
        .map_err(|e| Error::Adb(format!("AAE's helper sent a list AAE can't read: {e}")))?;
    // Stable component identities keep native pickers safe even if Android
    // reports the same service twice during a package replacement.
    let mut seen = std::collections::HashSet::new();
    services.retain(|s| !s.component.is_empty() && seen.insert(s.component.clone()));
    let enabled = adb.enabled_services().await?;
    for service in &mut services {
        service.on = enabled
            .iter()
            .any(|c| same_component(c, &service.component));
        service.current_screen_reader = device
            .meta
            .screen_reader
            .as_deref()
            .is_some_and(|c| same_component(c, &service.component));
    }
    services.sort_by_key(|s| (!s.screen_reader, s.label.to_lowercase()));
    Ok(services)
}

/// Finds a service by its name, package or component, ignoring case.
pub fn find<'a>(services: &'a [Service], name: &str) -> Option<&'a Service> {
    let name = name.trim();
    services
        .iter()
        .find(|s| same_component(&s.component, name))
        .or_else(|| services.iter().find(|s| s.label.eq_ignore_ascii_case(name)))
        .or_else(|| {
            services
                .iter()
                .find(|s| s.package().eq_ignore_ascii_case(name))
        })
}

/// Turns a service on, to stay on, or off, to stay off. Turning on a
/// screen reader makes it the device's screen reader instead of the one it
/// had, as two can't run at once. Turning off the device's screen reader
/// leaves it with none, and AAE won't offer one.
pub async fn set(device: &mut Device, adb: &Adb, service: &Service, on: bool) -> Result<()> {
    if on && service.screen_reader {
        return use_screen_reader(device, adb, service).await;
    }
    let component = service.component.clone();
    device.meta.app_choices.insert(component.clone(), on);
    if on {
        device.keep_service_enabled(&component);
        device.save_meta()?;
        adb.ensure_services(std::slice::from_ref(&component), SERVICE_TIMEOUT)
            .await?;
    } else {
        device
            .meta
            .keep_enabled
            .retain(|c| !same_component(c, &component));
        if device
            .meta
            .screen_reader
            .as_deref()
            .is_some_and(|c| same_component(c, &component))
        {
            device.meta.screen_reader = None;
            device.meta.screen_reader_declined = true;
        }
        device.save_meta()?;
        adb.disable_service(&component).await?;
    }
    Ok(())
}

/// Makes an installed screen reader the device's screen reader, turning off
/// any other, and keeps it on.
pub async fn use_screen_reader(device: &mut Device, adb: &Adb, service: &Service) -> Result<()> {
    let component = service.component.clone();
    let previous = device.clone();
    let previous_enabled = adb.enabled_services().await?;
    // Every other screen reader goes off, the old one included.
    for other in list(adb, device).await? {
        if other.screen_reader && other.on && !same_component(&other.component, &component) {
            device
                .meta
                .keep_enabled
                .retain(|c| !same_component(c, &other.component));
            device
                .meta
                .app_choices
                .insert(other.component.clone(), false);
            adb.disable_service(&other.component).await?;
        }
    }
    if let Some(old) = device.meta.screen_reader.take() {
        device
            .meta
            .keep_enabled
            .retain(|c| !same_component(c, &old));
    }
    device.meta.screen_reader = Some(component.clone());
    device.meta.screen_reader_declined = false;
    device.meta.app_choices.insert(component.clone(), true);
    device.keep_service_enabled(&component);
    device.save_meta()?;
    if let Err(error) = adb
        .ensure_services(std::slice::from_ref(&component), SERVICE_TIMEOUT)
        .await
    {
        // A service that crashes on startup must not leave the device with
        // the old working reader disabled and the failed choice persisted.
        let _ = adb.disable_service(&component).await;
        let _ = adb.enable_services(&previous_enabled).await;
        *device = previous;
        device.save_meta()?;
        return Err(error);
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[tokio::test]
    async fn failed_reader_switch_restores_previous_reader_and_preferences() {
        let dir = std::env::temp_dir().join(format!("aae-reader-rollback-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("adb");
        // An adb stand-in that accepts settings changes, but fails when AAE
        // checks whether the newly enabled reader started.
        std::fs::write(&bin, r#"#!/usr/bin/env python3
import json, pathlib, shlex, sys
state = pathlib.Path(__file__).with_name('enabled')
args = shlex.split(sys.argv[-1])
if args[:2] == ['am', 'broadcast']:
    services = [dict(component='old.reader/.Service', label='Old', screenReader=True), dict(component='new.reader/.Service', label='New', screenReader=True)]
    print('Broadcast completed: result=1, data="' + json.dumps(services) + '"')
elif args[:4] == ['settings', 'get', '--user', 'current']:
    print(state.read_text() if state.exists() else '')
elif args[:2] == ['settings', 'get']:
    print(state.read_text() if state.exists() else '')
elif args[:2] == ['settings', 'put'] and 'enabled_accessibility_services' in args:
    i = args.index('enabled_accessibility_services')
    state.write_text(args[i+1] if len(args) > i+1 else '')
elif args[:2] == ['dumpsys', 'accessibility']:
    sys.exit(1)
"#).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(dir.join("enabled"), "old.reader/.Service:helper/.Service").unwrap();
        let meta = serde_json::from_value(serde_json::json!({
            "name": "Test", "api": 35, "tag": "google_apis", "abi": "arm64-v8a",
            "sysdir": "unused", "created": 0, "screen_reader": "old.reader/.Service",
            "keep_enabled": ["old.reader/.Service", "helper/.Service"],
            "app_choices": {"old.reader/.Service": true}
        }))
        .unwrap();
        let mut device = Device {
            id: "test".into(),
            dir: dir.clone(),
            meta,
        };
        device.save_meta().unwrap();
        let before = std::fs::read(dir.join("aae.toml")).unwrap();
        let adb = Adb::new(bin, "test");
        let service: Service = serde_json::from_value(serde_json::json!({
            "component": "new.reader/.Service", "label": "New", "screenReader": true
        }))
        .unwrap();
        assert!(
            use_screen_reader(&mut device, &adb, &service)
                .await
                .is_err()
        );
        assert_eq!(
            device.meta.screen_reader.as_deref(),
            Some("old.reader/.Service")
        );
        assert_eq!(std::fs::read(dir.join("aae.toml")).unwrap(), before);
        let enabled = adb.enabled_services().await.unwrap();
        assert!(
            enabled
                .iter()
                .any(|s| same_component(s, "old.reader/.Service"))
        );
        assert!(enabled.iter().any(|s| same_component(s, "helper/.Service")));
        assert!(
            !enabled
                .iter()
                .any(|s| same_component(s, "new.reader/.Service"))
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
