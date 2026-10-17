# AAE Advanced 0.7 — development build 1120

Desktop targets: macOS and Windows. Linux desktop support is planned for a future release.

- Samsung Galaxy Firmware: browse every model returned by SamMobile's model selector, choose a country/carrier, and list firmware versions and build codes. Open the selected download on SamMobile in your browser. On Mac, use File → Samsung Galaxy Firmware; on Windows, use Android Versions → Samsung Galaxy Firmware. Search filters the complete model list; Load More retrieves further firmware pages. These are physical-device firmware packages, not emulator images. The catalogue reports Android versions where supplied; AAE does not invent One UI version numbers.
- TV is available in the new-device type selector. Choose an Android TV or Google TV system image. The Mac selector filters images by TV type. Only images available for the host architecture are offered; some hosts may have no TV images.
- Accessibility Services: the Mac screen reader selector uses an explicit apply action. Service refreshes discard outdated responses, and reader changes are serialized. Windows controls update without holding state borrows across native UI callbacks. A failed reader startup restores the previously enabled services and saved reader choice.

SamMobile is an external catalogue; connection errors are reported and can be retried. Its website controls download availability and any account requirements. No Samsung firmware is flashed by AAE.

The development workflow identifies the release as 0.7.0-dev.1120, build 1120. Increment its build number before publishing the next development release. Stable update feeds are unchanged.
