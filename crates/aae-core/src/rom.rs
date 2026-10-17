//! Android images from outside Google's catalogue: GrapheneOS, /e/OS,
//! LineageOS, and images people build themselves.
//!
//! Google's emulator boots a folder of disk images: `system.img`, `vendor.img`,
//! `ramdisk.img`, `kernel-ranchu` and the rest. Projects that build Android
//! themselves can package that folder (LineageOS's emulator build makes a zip
//! of one), and a single `system.img` can be put on an installed version's
//! other files. AAE copies what it is given into the SDK's `system-images`
//! folder, with the `source.properties` the SDK expects, so `aae`, the apps
//! and the emulator see it as they see an Android version from Google.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::device::dir_size;
use crate::error::{Error, IoContext, Result};
use crate::sdk::{self, Sdk, SystemImage, host_abi};

/// The file where AAE records which Android an imported image is, beside
/// `source.properties` in the image folder.
pub const FILE: &str = "aae-rom.properties";

/// Which Android an imported image is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rom {
    GrapheneOs,
    Eos,
    LineageOs,
    Custom,
}

impl Rom {
    pub const ALL: [Rom; 4] = [Rom::GrapheneOs, Rom::Eos, Rom::LineageOs, Rom::Custom];

    /// The image type's tag in the SDK, as `source.properties` holds it.
    pub fn tag(self) -> &'static str {
        match self {
            Rom::GrapheneOs => "grapheneos",
            Rom::Eos => "eos",
            Rom::LineageOs => "lineageos",
            Rom::Custom => "custom",
        }
    }

    /// Its name in plain words.
    pub fn name(self) -> &'static str {
        match self {
            Rom::GrapheneOs => "GrapheneOS",
            Rom::Eos => "/e/OS",
            Rom::LineageOs => "LineageOS",
            Rom::Custom => "Custom image",
        }
    }

    /// The ROM a tag names. A tag of `lineageos-2`, for a second LineageOS
    /// image of the same Android version, is LineageOS too.
    pub fn from_tag(tag: &str) -> Option<Rom> {
        Self::ALL.into_iter().find(|rom| {
            tag.strip_prefix(rom.tag())
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('-'))
        })
    }

    /// Where to get an image of it, for the person importing one.
    pub fn source_hint(self) -> &'static str {
        match self {
            Rom::GrapheneOs => {
                "GrapheneOS publishes no ready-made emulator image: build its emulator target \
                 (sdk_phone64_x86_64), then import the folder it makes. See \
                 https://grapheneos.org/build."
            }
            Rom::Eos => {
                "/e/OS publishes system images for phones, not emulator images. Import a system \
                 image you built for the emulator, or put a /e/OS system image on an installed \
                 Android version's files with --base. See https://doc.e.foundation."
            }
            Rom::LineageOs => {
                "LineageOS's emulator build makes a zip you can import: `mka emu_img_zip`. See \
                 https://wiki.lineageos.org/emulator."
            }
            Rom::Custom => {
                "Import a folder of emulator image files (system.img, vendor.img, kernel-ranchu, \
                 ramdisk.img), a zip of one, or a single system.img to put on an installed \
                 Android version's files with --base."
            }
        }
    }
}

/// What an import can choose.
#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    /// A name to show beside the Android version, such as "LineageOS 23".
    pub name: Option<String>,
    /// The Android version, when the image doesn't say.
    pub api: Option<u32>,
    /// For a single `system.img`: the installed Android version whose kernel,
    /// ramdisk, vendor and other files it uses.
    pub base: Option<SystemImage>,
}

/// Progress while importing an image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    /// Unpacking a downloaded zip.
    Unpacking,
    /// Copying bytes so far, and the total.
    Copying { done: u64, total: u64 },
}

impl Progress {
    /// The percentage copied, while copying.
    pub fn percent(&self) -> Option<u32> {
        match self {
            Progress::Copying { done, total } if *total > 0 => Some((done * 100 / total) as u32),
            _ => None,
        }
    }
}

/// What the person pointed AAE at.
enum Given {
    /// A folder of image files, from an emulator build.
    Folder(PathBuf),
    /// A system image on its own, which needs an installed version's other files.
    System(PathBuf),
}

/// Installs an image into the SDK. Blocks while it copies, so call it off the
/// async runtime. The image is labelled with `rom` and appears with the
/// installed Android versions.
pub fn install(
    sdk: &Sdk,
    source: &Path,
    rom: Rom,
    options: &ImportOptions,
    mut progress: impl FnMut(Progress),
) -> Result<SystemImage> {
    if !source.exists() {
        return Err(Error::Message(format!(
            "{} doesn't exist. Check the path and try again.",
            source.display()
        )));
    }
    static CALLS: AtomicU64 = AtomicU64::new(0);
    let temp = sdk.root.join(".temp").join(format!(
        "aae-import-{}-{}",
        std::process::id(),
        CALLS.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let given = unpack_source(source, &temp, &mut progress)?;
        install_given(sdk, given, rom, options, &mut progress)
    })();
    let _ = std::fs::remove_dir_all(&temp);
    result
}

/// Unpacks a zip into `temp`, and says what the source is. A single image,
/// compressed or not, stays where it is.
fn unpack_source(source: &Path, temp: &Path, progress: &mut impl FnMut(Progress)) -> Result<Given> {
    if source.is_dir() {
        // A build's output may be the image folder itself, or hold it in a
        // folder named after the processor type, as Google's downloads do.
        return Ok(Given::Folder(
            find_image_dir(source).unwrap_or_else(|| source.to_path_buf()),
        ));
    }
    let name = source
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if name.ends_with(".zip") {
        progress(Progress::Unpacking);
        std::fs::create_dir_all(temp).context(|| format!("Creating {}", temp.display()))?;
        let file =
            std::fs::File::open(source).context(|| format!("Opening {}", source.display()))?;
        let mut archive = zip::ZipArchive::new(file).map_err(|e| {
            Error::Message(format!("{} is not a valid zip file: {e}", source.display()))
        })?;
        archive.extract(temp).map_err(|e| {
            Error::Message(format!("{} couldn't be unpacked: {e}", source.display()))
        })?;
        let dir = find_image_dir(temp).ok_or_else(|| {
            Error::Message(format!(
                "{} has no Android image in it: there is no system.img, in the zip or a folder \
                 in it.",
                source.display()
            ))
        })?;
        return Ok(Given::Folder(dir));
    }
    if name.ends_with(".img.gz") {
        progress(Progress::Unpacking);
        std::fs::create_dir_all(temp).context(|| format!("Creating {}", temp.display()))?;
        let file =
            std::fs::File::open(source).context(|| format!("Opening {}", source.display()))?;
        let mut image = std::fs::File::create(temp.join("system.img"))
            .context(|| format!("Creating {}", temp.join("system.img").display()))?;
        std::io::copy(&mut flate2::read::GzDecoder::new(file), &mut image).map_err(|e| {
            Error::Message(format!("{} couldn't be unpacked: {e}", source.display()))
        })?;
        return Ok(Given::System(temp.join("system.img")));
    }
    Ok(Given::System(source.to_path_buf()))
}

/// The folder holding `system.img` in an unpacked zip: the zip's own folder,
/// or one inside it.
fn find_image_dir(root: &Path) -> Option<PathBuf> {
    let mut level = vec![root.to_path_buf()];
    for _ in 0..3 {
        let mut next = Vec::new();
        for dir in level {
            if dir.join("system.img").is_file() {
                return Some(dir);
            }
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if path.join("system.img").is_file() {
                        return Some(path);
                    }
                    next.push(path);
                }
            }
        }
        level = next;
    }
    None
}

fn install_given(
    sdk: &Sdk,
    given: Given,
    rom: Rom,
    options: &ImportOptions,
    progress: &mut impl FnMut(Progress),
) -> Result<SystemImage> {
    let (folder, system, full) = match &given {
        Given::Folder(dir) => {
            if !dir.join("system.img").is_file() {
                return Err(Error::Message(format!(
                    "{} has no system.img. An emulator image folder holds system.img, and \
                     usually vendor.img, kernel-ranchu and ramdisk.img.",
                    dir.display()
                )));
            }
            let full = [
                "vendor.img",
                "kernel-ranchu",
                "kernel-ranchu-64",
                "ramdisk.img",
            ]
            .iter()
            .any(|file| dir.join(file).is_file());
            (Some(dir.clone()), dir.join("system.img"), full)
        }
        Given::System(image) => {
            if !image.is_file() {
                return Err(Error::Message(format!(
                    "{} isn't a file AAE can read.",
                    image.display()
                )));
            }
            (None, image.clone(), false)
        }
    };
    if full && options.base.is_some() {
        return Err(Error::Message(
            "That folder has its own kernel, ramdisk and vendor files, so it doesn't need --base."
                .into(),
        ));
    }
    if !full && options.base.is_none() {
        return Err(Error::Message(format!(
            "A single system image needs the rest of an emulator image (kernel, ramdisk, vendor) \
             to start. Put it on an installed Android version's files with --base, or import a \
             whole emulator image folder instead.\n{}",
            rom.source_hint()
        )));
    }

    let properties = folder
        .as_ref()
        .and_then(|dir| sdk::read_properties(&dir.join("source.properties")).ok())
        .unwrap_or_default();
    let build_props = folder
        .as_ref()
        .map(|dir| read_build_prop(dir))
        .unwrap_or_default();
    let base = options.base.as_ref();
    let api = options
        .api
        .or_else(|| {
            properties
                .get("AndroidVersion.ApiLevel")
                .and_then(|t| api_level(t))
        })
        .or_else(|| {
            build_props
                .get("ro.build.version.sdk")
                .and_then(|t| api_level(t))
        })
        .or_else(|| {
            build_props
                .get("ro.system.build.version.sdk")
                .and_then(|t| api_level(t))
        })
        .or_else(|| base.map(|b| b.api))
        .ok_or_else(|| {
            Error::Message(format!(
                "{} doesn't say which Android version it is. Give it with --api, such as \
                 --api 35.",
                match &folder {
                    Some(dir) => dir.display().to_string(),
                    None => system.display().to_string(),
                }
            ))
        })?;
    let abi = properties
        .get("SystemImage.Abi")
        .cloned()
        .or_else(|| build_props.get("ro.product.cpu.abi").cloned())
        .or_else(|| build_props.get("ro.system.product.cpu.abi").cloned())
        .or_else(|| base.map(|b| b.abi.clone()))
        .unwrap_or_else(|| host_abi().to_string());
    let tag = free_tag(sdk, rom, api, &abi, options.name.as_deref());
    let dest = sdk
        .root
        .join("system-images")
        .join(format!("android-{api}"))
        .join(&tag)
        .join(&abi);

    let source_bytes = match &folder {
        Some(dir) => dir_size(dir),
        None => std::fs::metadata(&system).map(|m| m.len()).unwrap_or(0),
    };
    let total = source_bytes + base.map_or(0, |b| dir_size(&b.path));
    check_space(sdk, total)?;

    if let Some(base) = base {
        let mut done = 0;
        copy_tree(&base.path, &dest, &mut done, total, progress)?;
        // The base's build is not this image's.
        let _ = std::fs::remove_file(dest.join("package.xml"));
    }
    match &folder {
        Some(dir) => {
            let mut done = base.map_or(0, |b| dir_size(&b.path));
            copy_tree(dir, &dest, &mut done, total, progress)?;
        }
        None => {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)
                    .context(|| format!("Creating {}", parent.display()))?;
            }
            let mut done = base.map_or(0, |b| dir_size(&b.path));
            std::fs::copy(&system, dest.join("system.img"))
                .context(|| format!("Copying {}", system.display()))?;
            done += std::fs::metadata(&system).map(|m| m.len()).unwrap_or(0);
            progress(Progress::Copying { done, total });
        }
    }
    // Google's or the ROM's own package record describes another image.
    let _ = std::fs::remove_file(dest.join("package.xml"));
    write_source_properties(&dest, api, &tag, &abi)?;
    write_rom_file(&dest, rom, options.name.as_deref())?;

    SystemImage::read_dir(&sdk.root, &dest).ok_or_else(|| {
        Error::Message(format!(
            "{} was copied to {}, but AAE can't read it back. Check that the folder holds a \
             whole Android image.",
            rom.name(),
            dest.display()
        ))
    })
}

fn check_space(sdk: &Sdk, bytes: u64) -> Result<()> {
    let Some(free) = crate::platform::free_space(&sdk.root) else {
        return Ok(());
    };
    // Room for the copy and the files the new device will write.
    let needed = bytes + bytes / 10;
    if free < needed {
        return Err(Error::Message(format!(
            "The image needs about {} of free disk space, and only {} is free.",
            crate::device::human_size(needed),
            crate::device::human_size(free)
        )));
    }
    Ok(())
}

/// The first tag with this Android version and processor type that is free,
/// such as `lineageos`, then `lineageos-2`.
fn free_tag(sdk: &Sdk, rom: Rom, api: u32, abi: &str, name: Option<&str>) -> String {
    let base = match (rom, name.and_then(crate::device::slug)) {
        (Rom::Custom, Some(slug)) => format!("custom-{slug}"),
        _ => rom.tag().to_string(),
    };
    let parent = sdk
        .root
        .join("system-images")
        .join(format!("android-{api}"));
    let mut tag = base.clone();
    let mut number = 2;
    while parent.join(&tag).join(abi).exists() {
        tag = format!("{base}-{number}");
        number += 1;
    }
    tag
}

/// Copies a folder of image files, reporting the bytes copied.
fn copy_tree(
    from: &Path,
    to: &Path,
    done: &mut u64,
    total: u64,
    progress: &mut impl FnMut(Progress),
) -> Result<()> {
    std::fs::create_dir_all(to).context(|| format!("Creating {}", to.display()))?;
    let entries = std::fs::read_dir(from).context(|| format!("Reading {}", from.display()))?;
    for entry in entries.flatten() {
        let source = entry.path();
        let dest = to.join(entry.file_name());
        if source.is_dir() {
            copy_tree(&source, &dest, done, total, progress)?;
        } else {
            std::fs::copy(&source, &dest).context(|| format!("Copying {}", source.display()))?;
            *done += std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
            progress(Progress::Copying { done: *done, total });
        }
    }
    Ok(())
}

/// The image folder's `build.prop`, or the one in its system folder.
fn read_build_prop(dir: &Path) -> std::collections::HashMap<String, String> {
    for path in [dir.join("build.prop"), dir.join("system/build.prop")] {
        if let Ok(text) = std::fs::read_to_string(&path) {
            let props = sdk::parse_properties(&text);
            if !props.is_empty() {
                return props;
            }
        }
    }
    Default::default()
}

/// An API level as `source.properties` or `build.prop` writes it, such as
/// "35" or "36.1", as 35 or 36.
fn api_level(text: &str) -> Option<u32> {
    sdk::parse_api_level(text).map(|(api, _)| api)
}

/// Writes the `source.properties` the SDK and the emulator read, keeping what
/// the image said about itself.
fn write_source_properties(dir: &Path, api: u32, tag: &str, abi: &str) -> Result<()> {
    let path = dir.join("source.properties");
    let mut text = std::fs::read_to_string(&path).unwrap_or_default();
    set_property(&mut text, "AndroidVersion.ApiLevel", &api.to_string());
    set_property(&mut text, "SystemImage.Abi", abi);
    set_property(&mut text, "SystemImage.TagId", tag);
    std::fs::write(&path, text).context(|| format!("Writing {}", path.display()))
}

/// Records which Android the image is, for AAE.
fn write_rom_file(dir: &Path, rom: Rom, name: Option<&str>) -> Result<()> {
    let mut text = format!("rom={}\n", rom.tag());
    if let Some(name) = name.map(str::trim).filter(|n| !n.is_empty()) {
        // One line, so the properties file stays readable.
        let name = name.replace(['\r', '\n'], " ");
        text.push_str(&format!("name={name}\n"));
    }
    let path = dir.join(FILE);
    std::fs::write(&path, text).context(|| format!("Writing {}", path.display()))
}

/// Sets a key in a Java-style properties text, replacing it if it is there.
fn set_property(text: &mut String, key: &str, value: &str) {
    let mut out = String::new();
    let mut found = false;
    for line in text.lines() {
        if line.trim_start().starts_with(&format!("{key}=")) {
            if !found {
                out.push_str(&format!("{key}={value}\n"));
                found = true;
            }
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !found {
        out.push_str(&format!("{key}={value}\n"));
    }
    *text = out;
}

/// Reads the properties file beside an image, if it has one.
pub fn read_rom(dir: &Path) -> (Option<Rom>, Option<String>) {
    let Ok(text) = std::fs::read_to_string(dir.join(FILE)) else {
        return (None, None);
    };
    let props = sdk::parse_properties(&text);
    (
        props.get("rom").and_then(|tag| Rom::from_tag(tag)),
        props.get("name").cloned().filter(|n| !n.trim().is_empty()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aae-rom-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fake_image(dir: &Path, api: &str, full: bool) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("system.img"), vec![7u8; 16 * 1024]).unwrap();
        if full {
            std::fs::write(dir.join("vendor.img"), vec![1u8; 4096]).unwrap();
            std::fs::write(dir.join("kernel-ranchu"), vec![2u8; 4096]).unwrap();
            std::fs::write(dir.join("ramdisk.img"), vec![3u8; 4096]).unwrap();
        }
        std::fs::write(
            dir.join("source.properties"),
            format!(
                "Pkg.Desc=Test image\nAndroidVersion.ApiLevel={api}\nSystemImage.Abi={}\nSystemImage.TagId=default\n",
                host_abi()
            ),
        )
        .unwrap();
    }

    #[test]
    fn installs_a_built_image_and_names_it() {
        let root = temp("folder");
        let sdk = Sdk {
            root: root.join("sdk"),
        };
        std::fs::create_dir_all(&sdk.root).unwrap();
        let source = root.join("sdk_phone_x86_64");
        fake_image(&source, "35", true);

        let image = install(
            &sdk,
            &source,
            Rom::LineageOs,
            &ImportOptions {
                name: Some("LineageOS 23".into()),
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();

        assert_eq!(image.api, 35);
        assert_eq!(image.tag, "lineageos");
        assert_eq!(image.kind(), "LineageOS");
        assert_eq!(image.abi, host_abi());
        assert!(image.to_string().contains("LineageOS 23"), "{image}");
        assert!(image.path.join("vendor.img").is_file());
        assert!(image.path.join("system.img").is_file());
        assert!(!image.path.join("package.xml").is_file());
        let props = std::fs::read_to_string(image.path.join("source.properties")).unwrap();
        assert!(props.contains("SystemImage.TagId=lineageos"));
        assert!(props.contains("SystemImage.Abi="));
        assert!(
            props.contains("Pkg.Desc=Test image"),
            "other properties stay"
        );
        assert_eq!(sdk.system_images().len(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_lone_system_image_needs_an_installed_version() {
        let root = temp("system");
        let sdk = Sdk {
            root: root.join("sdk"),
        };
        std::fs::create_dir_all(&sdk.root).unwrap();
        let source = root.join("img");
        fake_image(&source, "34", true);
        let base = install(
            &sdk,
            &source,
            Rom::GrapheneOs,
            &ImportOptions::default(),
            |_| {},
        )
        .unwrap();

        let system = root.join("system.img");
        std::fs::write(&system, vec![9u8; 8 * 1024]).unwrap();
        let error = install(
            &sdk,
            &system,
            Rom::Custom,
            &ImportOptions::default(),
            |_| {},
        )
        .unwrap_err();
        assert!(error.to_string().contains("--base"), "{error}");

        let image = install(
            &sdk,
            &system,
            Rom::Custom,
            &ImportOptions {
                base: Some(base.clone()),
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
        assert_eq!(image.api, 34);
        assert_eq!(image.tag, "custom");
        assert_eq!(image.kind(), "Custom image");
        assert!(
            image.path.join("kernel-ranchu").is_file(),
            "the base's files"
        );
        assert_eq!(
            std::fs::read(image.path.join("system.img")).unwrap().len(),
            8 * 1024
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn numbers_a_second_image_of_the_same_kind() {
        let root = temp("numbers");
        let sdk = Sdk {
            root: root.join("sdk"),
        };
        std::fs::create_dir_all(&sdk.root).unwrap();
        let source = root.join("image");
        fake_image(&source, "35", true);
        let first = install(
            &sdk,
            &source,
            Rom::GrapheneOs,
            &ImportOptions::default(),
            |_| {},
        )
        .unwrap();
        let second = install(
            &sdk,
            &source,
            Rom::GrapheneOs,
            &ImportOptions::default(),
            |_| {},
        )
        .unwrap();
        assert_eq!(first.tag, "grapheneos");
        assert_eq!(second.tag, "grapheneos-2");
        assert_eq!(second.kind(), "GrapheneOS");
        assert_eq!(Rom::from_tag(&second.tag), Some(Rom::GrapheneOs));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reads_the_version_from_build_prop() {
        let root = temp("buildprop");
        let sdk = Sdk {
            root: root.join("sdk"),
        };
        std::fs::create_dir_all(&sdk.root).unwrap();
        let source = root.join("image");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("system.img"), vec![1u8; 4096]).unwrap();
        std::fs::write(source.join("vendor.img"), vec![1u8; 4096]).unwrap();
        std::fs::write(
            source.join("build.prop"),
            "ro.build.version.sdk=33\nro.product.cpu.abi=x86_64\n",
        )
        .unwrap();
        let image = install(&sdk, &source, Rom::Eos, &ImportOptions::default(), |_| {}).unwrap();
        assert_eq!(image.api, 33);
        assert_eq!(image.abi, "x86_64");
        assert_eq!(image.kind(), "/e/OS");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn unpacks_a_zip_of_an_image() {
        let root = temp("zip");
        let sdk = Sdk {
            root: root.join("sdk"),
        };
        std::fs::create_dir_all(&sdk.root).unwrap();
        let zip_path = root.join("lineage-22.2.zip");
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options: zip::write::SimpleFileOptions = Default::default();
        zip.start_file("sdk_phone_x86_64/system.img", options)
            .unwrap();
        std::io::Write::write_all(&mut zip, &vec![4u8; 8192]).unwrap();
        zip.start_file("sdk_phone_x86_64/vendor.img", options)
            .unwrap();
        std::io::Write::write_all(&mut zip, &vec![5u8; 4096]).unwrap();
        zip.start_file("sdk_phone_x86_64/build.prop", options)
            .unwrap();
        std::io::Write::write_all(&mut zip, b"ro.build.version.sdk=36\n").unwrap();
        zip.finish().unwrap();

        let image = install(
            &sdk,
            &zip_path,
            Rom::LineageOs,
            &ImportOptions::default(),
            |_| {},
        )
        .unwrap();
        assert_eq!(image.api, 36);
        assert!(image.path.join("vendor.img").is_file());
        assert!(
            image
                .sysdir
                .starts_with("system-images/android-36/lineageos/")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn keeps_other_properties_and_replaces_its_own() {
        let mut text =
            "Pkg.Revision=4\nSystemImage.TagId=default\nAndroidVersion.ApiLevel=30\n".to_string();
        set_property(&mut text, "SystemImage.TagId", "eos");
        set_property(&mut text, "AndroidVersion.ApiLevel", "35");
        set_property(&mut text, "SystemImage.Abi", "arm64-v8a");
        assert_eq!(
            text,
            "Pkg.Revision=4\nSystemImage.TagId=eos\nAndroidVersion.ApiLevel=35\nSystemImage.Abi=arm64-v8a\n"
        );
    }
}
