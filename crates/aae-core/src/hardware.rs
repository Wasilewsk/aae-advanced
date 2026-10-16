//! A device's advanced hardware: memory, processor cores, storage, the
//! screen's size and density, kept in the emulator's `config.ini`, and
//! graphics, kept in AAE's record. Changed only while the device is stopped;
//! it takes effect when it next starts.

use crate::device::Device;
use crate::error::{Error, Result};

/// A device's hardware.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hardware {
    /// Memory, in megabytes.
    pub memory_mb: u32,
    /// Processor cores.
    pub cores: u32,
    /// Storage for apps and data, in megabytes.
    pub storage_mb: u32,
    /// The screen, in pixels, as held upright.
    pub width: u32,
    pub height: u32,
    /// Pixels per inch, which sets how big things are drawn.
    pub density: u32,
    /// Draw the screen in software instead of with the computer's graphics
    /// adapter. Slower, but WebViews, such as Google's sign-in, crash on
    /// Android 16 and later with the graphics adapter on some Macs.
    pub software_graphics: bool,
}

/// A new device's storage, in megabytes: 6 GB, or with Google Play, which
/// fills most of 8 GB once it updates its apps, 16 GB. The disk file of a
/// recent Android version only takes the space that's used.
pub fn default_storage_mb(play: bool) -> u32 {
    if play { 16_384 } else { 6144 }
}

/// Limits, so a typing slip can't make a device that won't start.
pub const MEMORY_MB: (u32, u32) = (1024, 16384);
pub const CORES: (u32, u32) = (1, 16);
pub const STORAGE_MB: (u32, u32) = (2048, 131_072);
pub const SIDE: (u32, u32) = (320, 4096);
pub const DENSITY: (u32, u32) = (120, 640);

fn config(device: &Device) -> Result<String> {
    let path = device.dir.join("config.ini");
    std::fs::read_to_string(&path)
        .map_err(|e| Error::Message(format!("{} can't be read: {e}", path.display())))
}

fn value<'a>(config: &'a str, key: &str) -> Option<&'a str> {
    config.lines().find_map(|line| {
        let (k, v) = line.split_once('=')?;
        (k.trim() == key).then_some(v.trim())
    })
}

/// A size such as "6G", "2048M", "512MB" or "6442450944", in megabytes.
pub fn parse_size_mb(text: &str) -> Option<u32> {
    let text = text.trim().to_uppercase();
    let text = text.trim_end_matches('B');
    let (number, unit) = match text.chars().last()? {
        'G' => (&text[..text.len() - 1], 1024.0),
        'M' => (&text[..text.len() - 1], 1.0),
        'K' => (&text[..text.len() - 1], 1.0 / 1024.0),
        _ => (text, 1.0 / 1024.0 / 1024.0),
    };
    let mb = number.trim().parse::<f64>().ok()? * unit;
    (mb > 0.0).then_some(mb.round() as u32)
}

/// The device's hardware, from its `config.ini`.
pub fn read(device: &Device) -> Result<Hardware> {
    let config = config(device)?;
    let number = |key: &str, default: u32| {
        value(&config, key)
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    };
    Ok(Hardware {
        memory_mb: number("hw.ramSize", 2048),
        cores: device
            .meta
            .cores
            .unwrap_or_else(|| crate::setup::device_cores() as u32),
        storage_mb: value(&config, "disk.dataPartition.size")
            .and_then(parse_size_mb)
            .unwrap_or(6144),
        width: number("hw.lcd.width", 1080),
        height: number("hw.lcd.height", 2400),
        density: number("hw.lcd.density", 420),
        software_graphics: device.meta.software_graphics,
    })
}

/// Checks hardware is within AAE's limits. Says what isn't.
pub fn check(hardware: &Hardware) -> Result<()> {
    let within = |name: &str, value: u32, (low, high): (u32, u32), unit: &str| {
        if (low..=high).contains(&value) {
            Ok(())
        } else {
            Err(Error::Message(format!(
                "{name} must be from {low} to {high}{unit}."
            )))
        }
    };
    within("Memory", hardware.memory_mb, MEMORY_MB, " megabytes")?;
    within("Processor cores", hardware.cores, CORES, "")?;
    within("Storage", hardware.storage_mb, STORAGE_MB, " megabytes")?;
    within("The screen's width", hardware.width, SIDE, " pixels")?;
    within("The screen's height", hardware.height, SIDE, " pixels")?;
    within(
        "The screen's density",
        hardware.density,
        DENSITY,
        " dots per inch",
    )?;
    Ok(())
}

/// Hardware chosen when a device is created. None keeps AAE's default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Choice {
    pub memory_mb: Option<u32>,
    pub cores: Option<u32>,
    pub storage_mb: Option<u32>,
}

/// The memory, cores and storage a new device of this size gets unless
/// chosen, with or without Google Play.
pub fn defaults(profile: crate::device::Profile, play: bool) -> Choice {
    Choice {
        memory_mb: Some(profile.hardware().3),
        cores: Some(crate::setup::device_cores() as u32),
        storage_mb: Some(default_storage_mb(play)),
    }
}

/// Checks the hardware chosen for a new device is within AAE's limits, so a
/// typing slip is caught before an Android version downloads.
pub fn check_choice(choice: &Choice) -> Result<()> {
    check(&Hardware {
        memory_mb: choice.memory_mb.unwrap_or(MEMORY_MB.0),
        cores: choice.cores.unwrap_or(CORES.0),
        storage_mb: choice.storage_mb.unwrap_or(STORAGE_MB.0),
        width: SIDE.0,
        height: SIDE.0,
        density: DENSITY.0,
        software_graphics: false,
    })
}

/// Sets a new device's memory, cores and storage, before its first start.
pub fn choose(device: &mut Device, choice: &Choice) -> Result<()> {
    if *choice == Choice::default() {
        return Ok(());
    }
    let mut hardware = read(device)?;
    hardware.memory_mb = choice.memory_mb.unwrap_or(hardware.memory_mb);
    hardware.cores = choice.cores.unwrap_or(hardware.cores);
    hardware.storage_mb = choice.storage_mb.unwrap_or(hardware.storage_mb);
    check(&hardware)?;
    save(device, &hardware)?;
    if let Some(cores) = choice.cores {
        device.meta.cores = Some(cores);
        device.save_meta()?;
    }
    Ok(())
}

/// Whether a storage change waits for a wipe. The emulator grows the disk
/// of a device that has started, but not the encrypted data partition of
/// later Android versions, whose key it keeps in `encryptionkey.img`; the
/// partition only gets the new size when it's made again. It never shrinks.
fn storage_needs_wipe(dir: &std::path::Path, before: u32, after: u32) -> bool {
    after < before || (after > before && dir.join("encryptionkey.img").exists())
}

/// Writes the device's hardware into its `config.ini`, keeping everything
/// else. The device must be stopped. Returns what to say, including when a
/// storage change waits for a wipe.
pub fn write(device: &mut Device, hardware: &Hardware) -> Result<String> {
    if device.runtime().is_some() {
        return Err(Error::MustStop(device.meta.name.clone(), "changed"));
    }
    check(hardware)?;
    let before = read(device)?;
    // The folded screen is a fixed part of the unfolded one (see device.rs).
    if device.meta.profile == crate::device::Profile::Foldable
        && (hardware.width != before.width || hardware.height != before.height)
    {
        return Err(Error::Message(
            "A foldable's screen size can't be changed. Its density, memory, cores and storage can.".into(),
        ));
    }
    save(device, hardware)?;
    // AAE starts devices with as many cores as suit the computer, unless
    // they were chosen.
    let cores = (hardware.cores != before.cores || device.meta.cores.is_some())
        .then_some(hardware.cores);
    let software = hardware.software_graphics;
    if software != before.software_graphics {
        // The quick-boot snapshot holds the other graphics' state.
        let quick_boot = device.dir.join("snapshots").join("default_boot");
        if quick_boot.exists() {
            std::fs::remove_dir_all(&quick_boot)
                .map_err(|e| Error::Message(format!("{} can't be deleted: {e}", quick_boot.display())))?;
        }
    }
    device.update_meta(|meta| {
        if cores.is_some() {
            meta.cores = cores;
        }
        meta.software_graphics = software;
    })?;
    let mut said = format!(
        "{}: {} MB memory, {} cores, {} MB storage, {}x{} at {} dpi, {}, from next start.",
        device.meta.name,
        hardware.memory_mb,
        hardware.cores,
        hardware.storage_mb,
        hardware.width,
        hardware.height,
        hardware.density,
        if software {
            "software graphics"
        } else {
            "graphics adapter"
        }
    );
    if storage_needs_wipe(&device.dir, before.storage_mb, hardware.storage_mb) {
        said.push_str(" The storage change takes effect after a wipe.");
    }
    if hardware != &before {
        said.push_str(" The next start is a cold boot.");
    }
    Ok(said)
}

/// Writes the hardware into the device's `config.ini`, keeping everything else.
fn save(device: &Device, hardware: &Hardware) -> Result<()> {
    let config = config(device)?;
    let changes = [
        ("hw.ramSize", hardware.memory_mb.to_string()),
        ("hw.cpu.ncore", hardware.cores.to_string()),
        (
            "disk.dataPartition.size",
            format!("{}M", hardware.storage_mb),
        ),
        ("hw.lcd.width", hardware.width.to_string()),
        ("hw.lcd.height", hardware.height.to_string()),
        ("hw.lcd.density", hardware.density.to_string()),
    ];
    let mut lines: Vec<String> = config.lines().map(str::to_string).collect();
    for (key, new) in &changes {
        match lines
            .iter_mut()
            .find(|l| l.split_once('=').is_some_and(|(k, _)| k.trim() == *key))
        {
            Some(line) => *line = format!("{key}={new}"),
            None => lines.push(format!("{key}={new}")),
        }
    }
    let path = device.dir.join("config.ini");
    std::fs::write(&path, lines.join("\n") + "\n")
        .map_err(|e| Error::Message(format!("{} can't be written: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_as_the_emulator_writes_them() {
        assert_eq!(parse_size_mb("6G"), Some(6144));
        assert_eq!(parse_size_mb("2048M"), Some(2048));
        assert_eq!(parse_size_mb("512MB"), Some(512));
        assert_eq!(parse_size_mb("6442450944"), Some(6144));
        assert_eq!(parse_size_mb("lots"), None);
    }

    #[test]
    fn hardware_outside_the_limits_is_refused() {
        let good = Hardware {
            memory_mb: 4096,
            cores: 4,
            storage_mb: 8192,
            width: 1080,
            height: 2400,
            density: 420,
            software_graphics: false,
        };
        assert!(check(&good).is_ok());
        assert!(
            check(&Hardware {
                memory_mb: 64,
                ..good.clone()
            })
            .is_err()
        );
        assert!(
            check(&Hardware {
                density: 2000,
                ..good
            })
            .is_err()
        );
    }

    #[test]
    fn storage_changes_that_wait_for_a_wipe() {
        let dir = std::env::temp_dir().join(format!("aae-hardware-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Android 7 and earlier: the emulator grows the data partition.
        assert!(!storage_needs_wipe(&dir, 6144, 10240));
        assert!(storage_needs_wipe(&dir, 10240, 6144));
        assert!(!storage_needs_wipe(&dir, 6144, 6144));
        // Encrypted data, as on Android 16: only a wipe makes it bigger.
        std::fs::write(dir.join("encryptionkey.img"), b"").unwrap();
        assert!(storage_needs_wipe(&dir, 6144, 10240));
        assert!(!storage_needs_wipe(&dir, 6144, 6144));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
