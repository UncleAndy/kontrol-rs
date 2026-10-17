//! Thin wrapper around hidapi, mirroring `hid.c` of the C tool, with two
//! differences: errors are reported instead of being swallowed, and a failed
//! open also says which devices the HID layer *can* see.
//!
//! The device is opened through libusb (the same backend the C tool links), the
//! 22-byte LED report is written with its report ID as the first byte, and reads
//! use a timeout so the main loop stays interruptible.

use hidapi::{HidApi, HidDevice};

/// One device of the system's HID list - enough to tell which interface of
/// which device is which. On Linux the interface number usually is not filled
/// in (the path ends in it instead: `...:0003:0001:00`), and hidapi does not
/// report usage pages here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceSummary {
    pub vendor_id: u16,
    pub product_id: u16,
    pub product: Option<String>,
    pub manufacturer: Option<String>,
    pub serial: Option<String>,
    pub interface: i32,
    pub path: String,
}

/// Everything hidapi can see right now. Needs no permission, so it works even
/// when the devices cannot be opened.
pub fn list_devices() -> Result<Vec<DeviceSummary>, String> {
    let api = HidApi::new().map_err(|err| err.to_string())?;

    let devices = api
        .device_list()
        .map(|device| DeviceSummary {
            vendor_id: device.vendor_id(),
            product_id: device.product_id(),
            product: device.product_string().map(str::to_string),
            manufacturer: device.manufacturer_string().map(str::to_string),
            serial: device.serial_number().map(str::to_string),
            interface: device.interface_number(),
            path: device.path().to_string_lossy().into_owned(),
        })
        .collect();

    Ok(devices)
}

/// Renders [`list_devices`] as text: devices of `vendor_id` first (so the
/// keyboard is at the top of a long list), the one matching both ids marked.
/// Pure, so the layout is testable without any hardware.
pub fn format_device_list(devices: &[DeviceSummary], vendor_id: u16, product_id: u16) -> String {
    let mut sorted: Vec<&DeviceSummary> = devices.iter().collect();
    sorted.sort_by_key(|device| {
        (
            device.vendor_id != vendor_id,
            device.vendor_id,
            device.product_id,
            device.interface,
            device.path.clone(),
        )
    });

    let mut out = format!("HID devices the system exposes: {}\n", devices.len());

    for device in &sorted {
        let name = device.product.as_deref().unwrap_or("(no name)");
        let serial = device
            .serial
            .as_deref()
            .map(|serial| format!(" ser={serial}"))
            .unwrap_or_default();

        let mark = if device.vendor_id == vendor_id && device.product_id == product_id {
            "  <- matches -v/-p"
        } else if device.vendor_id == vendor_id {
            "  <- vendor id matches -v"
        } else {
            ""
        };

        out += &format!(
            "  {:04x}:{:04x} if{:>2}  {}{}  {}{}\n",
            device.vendor_id,
            device.product_id,
            device.interface,
            name,
            serial,
            device.path,
            mark
        );
    }

    let same_vendor = devices
        .iter()
        .filter(|device| device.vendor_id == vendor_id)
        .count();
    let same_both = devices
        .iter()
        .filter(|device| device.vendor_id == vendor_id && device.product_id == product_id)
        .count();
    out += &format!(
        "Vendor id {vendor_id:04x}: {same_vendor} device(s); {vendor_id:04x}:{product_id:04x}: {same_both}.\n"
    );

    out
}

pub struct Hid {
    /// Kept alive: `HidDevice` borrows from the context internally.
    _api: HidApi,
    device: HidDevice,
    /// Remembered, because a handle to a keyboard that was unplugged is dead
    /// while the ids are not: this is what finds it again after it is back.
    vendor: u16,
    product: u16,
}

impl Hid {
    /// Opens the device with the given USB ids. The error text comes from
    /// hidapi/libusb and is meant to be shown to the user as it is.
    pub fn open(vid: u16, pid: u16) -> Result<Self, String> {
        let api = HidApi::new().map_err(|err| err.to_string())?;
        let device = api.open(vid, pid).map_err(|err| err.to_string())?;
        // The main loop wants blocking reads with a timeout, not polled reads.
        let _ = device.set_blocking_mode(true);
        Ok(Hid {
            _api: api,
            device,
            vendor: vid,
            product: pid,
        })
    }

    /// The USB ids this handle was opened with, so the same keyboard can be
    /// found again once it comes back.
    pub fn ids(&self) -> (u16, u16) {
        (self.vendor, self.product)
    }

    /// Returns the number of bytes written, or the error hidapi reported.
    pub fn write_raw(&self, buffer: &[u8]) -> Result<usize, String> {
        self.device.write(buffer).map_err(|err| err.to_string())
    }

    /// Returns the number of bytes read (0 on timeout), or the error.
    pub fn read_timeout(&self, buffer: &mut [u8], millis: i32) -> Result<usize, String> {
        self.device
            .read_timeout(buffer, millis)
            .map_err(|err| err.to_string())
    }

    /// Devices the HID layer can see with this vendor id, as `"17cc:1740 name"`
    /// lines, plus whether the exact `vid:pid` is among them.
    ///
    /// Enumeration needs no permission, so this separates "the keyboard is not
    /// there / not in the right mode" from "it is there but opening was
    /// refused" - the two cases that a single "opening failed" message cannot
    /// tell apart. hidapi's own error text is useless for the libusb backend
    /// ("hid_error is not implemented yet"), which is why the list is printed
    /// instead of a reason.
    pub fn enumerate_matching(vid: u16, pid: u16) -> Result<(Vec<String>, bool), String> {
        let api = HidApi::new().map_err(|err| err.to_string())?;

        let mut present = false;
        let mut lines = Vec::new();
        for device in api.device_list().filter(|device| device.vendor_id() == vid) {
            if device.product_id() == pid {
                present = true;
            }
            lines.push(format!(
                "{:04x}:{:04x} {}{}",
                device.vendor_id(),
                device.product_id(),
                device.product_string().unwrap_or("(no name)"),
                device
                    .serial_number()
                    .map(|serial| format!(" [serial {serial}]"))
                    .unwrap_or_default()
            ));
        }

        Ok((lines, present))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(vendor: u16, product: u16, interface: i32) -> DeviceSummary {
        DeviceSummary {
            vendor_id: vendor,
            product_id: product,
            product: Some("Komplete Kontrol A49".to_string()),
            manufacturer: Some("Native Instruments".to_string()),
            serial: None,
            interface,
            path: format!("/dev/hidraw{iface}", iface = interface),
        }
    }

    #[test]
    fn the_keyboard_is_listed_first_and_marked() {
        let devices = [
            device(0x046d, 0xc52b, 1),
            device(0x17cc, 0x1740, 0),
            device(0x17cc, 0x1750, 1),
        ];

        let text = format_device_list(&devices, 0x17cc, 0x1740);
        let lines: Vec<&str> = text.lines().collect();

        assert_eq!(lines[0], "HID devices the system exposes: 3");
        assert!(lines[1].contains("17cc:1740"));
        assert!(lines[1].contains("<- matches -v/-p"));
        assert!(lines[2].contains("17cc:1750"));
        assert!(lines[2].contains("<- vendor id matches -v"));
        assert!(lines[3].contains("046d:c52b"));
        assert!(!lines[3].contains("<-"));
        assert!(lines[4].contains("Vendor id 17cc: 2 device(s); 17cc:1740: 1."));
    }

    #[test]
    fn an_empty_list_still_says_so() {
        let text = format_device_list(&[], 0x17cc, 0x1740);
        assert!(text.starts_with("HID devices the system exposes: 0"));
        assert!(text.contains("Vendor id 17cc: 0 device(s); 17cc:1740: 0."));
    }

    #[test]
    fn devices_of_other_vendors_are_sorted_by_pid_and_interface() {
        let devices = [
            device(0x046d, 0xc52b, 2),
            device(0x046d, 0xc52b, 1),
            device(0x046d, 0xc31c, 0),
        ];

        let text = format_device_list(&devices, 0x17cc, 0x1740);

        // 0xc31c before 0xc52b, and inside 0xc52b interface 1 before 2.
        assert!(text.find("c31c").unwrap() < text.find("c52b").unwrap());
        assert!(text.find("if 1").unwrap() < text.find("if 2").unwrap());
    }
}
