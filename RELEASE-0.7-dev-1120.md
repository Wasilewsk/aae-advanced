# AAE Advanced 0.7 — development build 1120

Desktop targets: macOS and Windows. Linux desktop support is planned for a future release.

- Samsung Galaxy Firmware: browse only models verified to have United Kingdom firmware, choose a UK carrier/product code, and list firmware versions and build codes. Open the selected download on SamMobile in your browser. On Mac, use File → Samsung Galaxy Firmware; on Windows, use Android Versions → Samsung Galaxy Firmware. Search by model number or a specific phone name. Country lists are verified before a matching model appears; non-UK models and firmware are excluded. Searches are bounded to avoid bulk requests and country records are cached for a day; Load More retrieves further firmware pages. These are physical-device firmware packages, not emulator images. The catalogue reports Android versions where supplied; AAE does not invent One UI version numbers.
- TV is available in the new-device type selector. Choose an Android TV or Google TV system image. The Mac selector filters images by TV type. Only images available for the host architecture are offered; some hosts may have no TV images.
- Accessibility Services: the Mac screen reader selector uses an explicit apply action. Service refreshes discard outdated responses, and reader changes are serialized. Windows controls update without holding state borrows across native UI callbacks. A failed reader startup restores the previously enabled services and saved reader choice.

SamMobile is an external catalogue; connection errors are reported and can be retried. Its website controls download availability and any account requirements. No Samsung firmware is flashed by AAE. QEMU CPU translation alone cannot boot these hardware-specific phone images; no UK Galaxy phone emulation target has been verified for this release.

The development workflow identifies the release as 0.7.0-dev.1120, build 1120. Increment its build number before publishing the next development release. Stable update feeds are unchanged.

See [Samsung/QEMU feasibility](docs/samsung-emulation.md) for host/guest CPU differences, specification spoofing and the experimental J5 limitations.
