use std::ffi::c_void;
use std::mem::size_of;

use windows::UI::Input::TouchpadGesturesController;
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize};
use windows::Win32::UI::Controls::{POINTER_DEVICE_INFO, POINTER_DEVICE_TYPE_TOUCH_PAD};
use windows::Win32::UI::Input::Pointer::GetPointerDevices;
use windows::Win32::UI::Input::{
    GetRawInputDeviceInfoW, GetRawInputDeviceList, RAWINPUTDEVICELIST, RID_DEVICE_INFO,
    RID_DEVICE_INFO_HID, RIDI_DEVICEINFO, RIDI_DEVICENAME, RIM_TYPEHID,
};

const HID_USAGE_PAGE_DIGITIZER: u16 = 0x0d;
const HID_USAGE_TOUCH_PAD: u16 = 0x05;
const RAW_INPUT_ERROR: u32 = u32::MAX;

#[derive(Debug)]
struct HidDevice {
    name: String,
    vendor_id: u32,
    product_id: u32,
    usage_page: u16,
    usage: u16,
}

impl HidDevice {
    fn is_touchpad(&self) -> bool {
        self.usage_page == HID_USAGE_PAGE_DIGITIZER && self.usage == HID_USAGE_TOUCH_PAD
    }
}

fn main() -> Result<(), String> {
    println!("DISPOSABLE TOUCHPAD PROBE");
    println!("os={}", std::env::consts::OS);

    let hid_devices = enumerate_hid_devices()?;
    let touchpads = hid_devices
        .iter()
        .filter(|device| device.is_touchpad())
        .collect::<Vec<_>>();

    println!("raw_input_hid_count={}", hid_devices.len());
    for device in &hid_devices {
        println!(
            "hid usage_page=0x{:04x} usage=0x{:04x} vendor=0x{:04x} product=0x{:04x} touchpad={} name={}",
            device.usage_page,
            device.usage,
            device.vendor_id,
            device.product_id,
            device.is_touchpad(),
            device.name,
        );
    }
    println!("raw_input_touchpad_count={}", touchpads.len());

    let pointer_devices = enumerate_pointer_devices()?;
    let pointer_touchpads = pointer_devices
        .iter()
        .filter(|device| device.pointerDeviceType == POINTER_DEVICE_TYPE_TOUCH_PAD)
        .count();
    println!("pointer_device_count={}", pointer_devices.len());
    for device in &pointer_devices {
        let product = utf16z(&device.productString);
        println!(
            "pointer type={} contacts={} touchpad={} product={}",
            device.pointerDeviceType.0,
            device.maxActiveContacts,
            device.pointerDeviceType == POINTER_DEVICE_TYPE_TOUCH_PAD,
            product,
        );
    }
    println!("pointer_touchpad_count={pointer_touchpads}");

    let winrt_supported = touchpad_gestures_supported()?;
    println!("touchpad_gestures_controller_supported={winrt_supported}");
    println!(
        "verdict={}",
        if touchpads.is_empty() && pointer_touchpads == 0 {
            "no_present_precision_touchpad"
        } else {
            "touchpad_path_present_requires_physical_gesture_run"
        }
    );

    Ok(())
}

fn enumerate_hid_devices() -> Result<Vec<HidDevice>, String> {
    let mut count = 0;
    let list_size = u32::try_from(size_of::<RAWINPUTDEVICELIST>())
        .map_err(|_| "raw input device-list size exceeds u32".to_owned())?;
    let first = unsafe { GetRawInputDeviceList(None, &mut count, list_size) };
    if first == RAW_INPUT_ERROR {
        return Err("GetRawInputDeviceList count failed".to_owned());
    }

    let mut list = vec![RAWINPUTDEVICELIST::default(); count as usize];
    let returned = unsafe { GetRawInputDeviceList(Some(list.as_mut_ptr()), &mut count, list_size) };
    if returned == RAW_INPUT_ERROR {
        return Err("GetRawInputDeviceList data failed".to_owned());
    }
    list.truncate(returned as usize);

    list.into_iter()
        .filter(|device| device.dwType == RIM_TYPEHID)
        .map(|device| {
            let mut info = RID_DEVICE_INFO {
                cbSize: u32::try_from(size_of::<RID_DEVICE_INFO>())
                    .map_err(|_| "raw input device-info size exceeds u32".to_owned())?,
                ..Default::default()
            };
            let mut info_size = info.cbSize;
            let info_result = unsafe {
                GetRawInputDeviceInfoW(
                    Some(device.hDevice),
                    RIDI_DEVICEINFO,
                    Some((&mut info as *mut RID_DEVICE_INFO).cast::<c_void>()),
                    &mut info_size,
                )
            };
            if info_result == RAW_INPUT_ERROR {
                return Err("GetRawInputDeviceInfoW info failed".to_owned());
            }

            let hid: RID_DEVICE_INFO_HID = unsafe { info.Anonymous.hid };
            Ok(HidDevice {
                name: raw_device_name(device.hDevice)?,
                vendor_id: hid.dwVendorId,
                product_id: hid.dwProductId,
                usage_page: hid.usUsagePage,
                usage: hid.usUsage,
            })
        })
        .collect()
}

fn raw_device_name(device: windows::Win32::Foundation::HANDLE) -> Result<String, String> {
    let mut chars = 0;
    let first = unsafe { GetRawInputDeviceInfoW(Some(device), RIDI_DEVICENAME, None, &mut chars) };
    if first == RAW_INPUT_ERROR {
        return Err("GetRawInputDeviceInfoW name size failed".to_owned());
    }

    let mut name = vec![0u16; chars as usize];
    let returned = unsafe {
        GetRawInputDeviceInfoW(
            Some(device),
            RIDI_DEVICENAME,
            Some(name.as_mut_ptr().cast::<c_void>()),
            &mut chars,
        )
    };
    if returned == RAW_INPUT_ERROR {
        return Err("GetRawInputDeviceInfoW name failed".to_owned());
    }
    name.truncate(returned as usize);
    Ok(utf16z(&name))
}

fn enumerate_pointer_devices() -> Result<Vec<POINTER_DEVICE_INFO>, String> {
    let mut count = 0;
    unsafe { GetPointerDevices(&mut count, None) }
        .map_err(|error| format!("GetPointerDevices count failed: {error}"))?;
    if count == 0 {
        return Ok(Vec::new());
    }
    let mut devices = vec![POINTER_DEVICE_INFO::default(); count as usize];
    unsafe { GetPointerDevices(&mut count, Some(devices.as_mut_ptr())) }
        .map_err(|error| format!("GetPointerDevices data failed: {error}"))?;
    devices.truncate(count as usize);
    Ok(devices)
}

fn touchpad_gestures_supported() -> Result<bool, String> {
    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }
        .map_err(|error| format!("RoInitialize failed: {error}"))?;
    let supported = TouchpadGesturesController::IsSupported()
        .map_err(|error| format!("TouchpadGesturesController.IsSupported failed: {error}"));
    unsafe { RoUninitialize() };
    supported
}

fn utf16z(value: &[u16]) -> String {
    let end = value
        .iter()
        .position(|code| *code == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..end])
}
