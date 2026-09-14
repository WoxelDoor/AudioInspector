//! Property keys the scanners read by name, and naming of any key for the Raw section.
//!
//! SDK keys are checked against the generated table by a test. The Bluetooth keys
//! come from the WDK headers and from probing live devices; the A2DP codec keys are
//! undocumented and were read from a live headset.

#![allow(dead_code)]

use windows::core::GUID;
use windows::Win32::Devices::Properties::DEVPROPKEY;

use super::keynames::KEY_NAMES;

pub const fn key(fmtid: u128, pid: u32) -> DEVPROPKEY {
    DEVPROPKEY { fmtid: GUID::from_u128(fmtid), pid }
}

const DEV: u128 = 0xa45c254e_df1c_4efd_8020_67d146a850e0;
const DEV_REL: u128 = 0x4340a6c5_93fa_4706_972c_7b648008a5a7;
const DEV_INST: u128 = 0x83da6326_97a6_4088_9453_a1923f573b29;
const DRV: u128 = 0xa8b865dd_2e3d_4094_ad97_e593a70c75d6;
const DEV_BUS: u128 = 0x540b947e_8b40_45bc_a8a2_6a0b894cbda2;

pub const NAME: DEVPROPKEY = key(0xb725f130_47ef_101a_a5f1_02608c9eebac, 10);
pub const DEVICE_DESC: DEVPROPKEY = key(DEV, 2);
pub const HARDWARE_IDS: DEVPROPKEY = key(DEV, 3);
pub const COMPATIBLE_IDS: DEVPROPKEY = key(DEV, 4);
pub const SERVICE: DEVPROPKEY = key(DEV, 6);
pub const CLASS: DEVPROPKEY = key(DEV, 9);
pub const MANUFACTURER: DEVPROPKEY = key(DEV, 13);
pub const FRIENDLY_NAME: DEVPROPKEY = key(DEV, 14);
pub const LOCATION_INFO: DEVPROPKEY = key(DEV, 15);
pub const UPPER_FILTERS: DEVPROPKEY = key(DEV, 19);
pub const LOWER_FILTERS: DEVPROPKEY = key(DEV, 20);
pub const ENUMERATOR_NAME: DEVPROPKEY = key(DEV, 24);
pub const ADDRESS: DEVPROPKEY = key(DEV, 30);
pub const LOCATION_PATHS: DEVPROPKEY = key(DEV, 37);
pub const BUS_REPORTED_DESC: DEVPROPKEY = key(DEV_BUS, 4);
pub const IS_PRESENT: DEVPROPKEY = key(DEV_BUS, 5);
pub const STACK: DEVPROPKEY = key(DEV_BUS, 14);
pub const CONTAINER_ID: DEVPROPKEY = key(0x8c7ed206_3f8a_4827_b3ab_ae9e1faefc6c, 2);
pub const IN_LOCAL_MACHINE_CONTAINER: DEVPROPKEY = key(0x8c7ed206_3f8a_4827_b3ab_ae9e1faefc6c, 4);
pub const DEVNODE_STATUS: DEVPROPKEY = key(DEV_REL, 2);
pub const PROBLEM_CODE: DEVPROPKEY = key(DEV_REL, 3);
pub const PARENT: DEVPROPKEY = key(DEV_REL, 8);
pub const CHILDREN: DEVPROPKEY = key(DEV_REL, 9);
pub const INSTALL_DATE: DEVPROPKEY = key(DEV_INST, 100);
pub const FIRST_INSTALL_DATE: DEVPROPKEY = key(DEV_INST, 101);
pub const LAST_ARRIVAL_DATE: DEVPROPKEY = key(DEV_INST, 102);
pub const LAST_REMOVAL_DATE: DEVPROPKEY = key(DEV_INST, 103);
pub const DRIVER_DATE: DEVPROPKEY = key(DRV, 2);
pub const DRIVER_VERSION: DEVPROPKEY = key(DRV, 3);
pub const DRIVER_DESC: DEVPROPKEY = key(DRV, 4);
pub const DRIVER_INF_PATH: DEVPROPKEY = key(DRV, 5);
pub const DRIVER_INF_SECTION: DEVPROPKEY = key(DRV, 6);
pub const DRIVER_PROVIDER: DEVPROPKEY = key(DRV, 9);
pub const CONTAINER_CATEGORY: DEVPROPKEY = key(0x78c34fc8_104a_4aca_9ea4_524d52996e57, 90);

// Core Audio endpoint property store (IMMDevice::OpenPropertyStore).
pub const PKEY_FRIENDLY_NAME: DEVPROPKEY = key(DEV, 14);
pub const PKEY_DEVICE_DESC: DEVPROPKEY = key(DEV, 2);
pub const PKEY_FORM_FACTOR: DEVPROPKEY = key(0x1da5d803_d492_4edd_8c23_e0c0ffee7f0e, 0);
pub const PKEY_JACK_SUBTYPE: DEVPROPKEY = key(0x1da5d803_d492_4edd_8c23_e0c0ffee7f0e, 8);
pub const PKEY_DISABLE_SYSFX: DEVPROPKEY = key(0x1da5d803_d492_4edd_8c23_e0c0ffee7f0e, 5);
pub const PKEY_PHYSICAL_SPEAKERS: DEVPROPKEY = key(0x1da5d803_d492_4edd_8c23_e0c0ffee7f0e, 3);
pub const PKEY_EVENT_DRIVEN: DEVPROPKEY = key(0x1da5d803_d492_4edd_8c23_e0c0ffee7f0e, 7);
pub const PKEY_DEVICE_FORMAT: DEVPROPKEY = key(0xf19f064d_082c_4e27_bc73_6882a1bb8e4c, 0);
pub const PKEY_OEM_FORMAT: DEVPROPKEY = key(0xe4870e26_3cc5_4cd2_ba46_ca0a9a70ed04, 3);
/// Undocumented, measured: "{1}.<KS function instance id>".
pub const PKEY_EP_FUNCTION: DEVPROPKEY = key(0xb3f8fa53_0004_438e_9003_51a46e139bfc, 2);
/// Undocumented, measured: the device name without the endpoint prefix.
pub const PKEY_EP_DEVICE_NAME: DEVPROPKEY = key(0xb3f8fa53_0004_438e_9003_51a46e139bfc, 6);
/// Undocumented, measured: "{2}.\\?\<KS filter interface path>".
pub const PKEY_EP_FILTER: DEVPROPKEY = key(0x233164c8_1b2c_4c7d_bc68_b671687a2567, 1);
/// Undocumented, measured: the endpoint's WinRT device interface id.
pub const PKEY_EP_INTERFACE_ID: DEVPROPKEY = key(0x9c119480_ddc2_4954_a150_5bd240d454ad, 9);

// Bluetooth devnode keys (WDK bthguid.h; not in the SDK).
const BT: u128 = 0x2bd67d8b_8beb_48d5_87e0_6cda3428040a;
pub const BT_ADDRESS: DEVPROPKEY = key(BT, 1);
pub const BT_SERVICE_GUID: DEVPROPKEY = key(BT, 2);
pub const BT_DEVICE_FLAGS: DEVPROPKEY = key(BT, 3);
pub const BT_VID_SOURCE: DEVPROPKEY = key(BT, 6);
pub const BT_VID: DEVPROPKEY = key(BT, 7);
pub const BT_PID: DEVPROPKEY = key(BT, 8);
pub const BT_PRODUCT_VERSION: DEVPROPKEY = key(BT, 9);
pub const BT_CLASS_OF_DEVICE: DEVPROPKEY = key(BT, 10);
pub const BT_LAST_CONNECTED: DEVPROPKEY = key(BT, 11);
pub const BT_LAST_SEEN: DEVPROPKEY = key(BT, 12);
/// Battery percentage (BYTE) and its update time (FILETIME), on the Hands-Free AG devnode.
pub const BT_BATTERY: DEVPROPKEY = key(0x104ea319_6ee2_4701_bd47_8ddbf425bbe5, 2);
pub const BT_BATTERY_UPDATED: DEVPROPKEY = key(0x104ea319_6ee2_4701_bd47_8ddbf425bbe5, 7);
// Local radio.
const RADIO: u128 = 0xa92f26ca_eda7_4b1d_9db2_27b68aa5a2eb;
pub const RADIO_ADDRESS: DEVPROPKEY = key(RADIO, 1);
pub const RADIO_MANUFACTURER: DEVPROPKEY = key(RADIO, 2);
pub const RADIO_LMP_FEATURES: DEVPROPKEY = key(RADIO, 3);
pub const RADIO_LMP_VERSION: DEVPROPKEY = key(RADIO, 4);
pub const RADIO_LMP_SUBVERSION: DEVPROPKEY = key(RADIO, 5);
pub const RADIO_HCI_VERSION: DEVPROPKEY = key(RADIO, 6);
pub const RADIO_HCI_REVISION: DEVPROPKEY = key(RADIO, 7);
pub const RADIO_LE_FEATURES: DEVPROPKEY = key(RADIO, 8);
/// Undocumented A2DP codec keys on the BthA2dp KS filter interface.
pub const A2DP_CODEC_FLAG: DEVPROPKEY = key(0x29ce83d4_7a82_4744_bd1d_abec85321dd6, 2);
pub const A2DP_CODEC_ACTIVE: DEVPROPKEY = key(0x29ce83d4_7a82_4744_bd1d_abec85321dd6, 3);
pub const A2DP_CODEC_LIST: DEVPROPKEY = key(0x29ce83d4_7a82_4744_bd1d_abec85321dd6, 4);

/// Names for keys the SDK does not ship.
static HAND_NAMES: &[(u128, u32, &str)] = &[
    (BT, 1, "DEVPKEY_Bluetooth_DeviceAddress"),
    (BT, 2, "DEVPKEY_Bluetooth_ServiceGUID"),
    (BT, 3, "DEVPKEY_Bluetooth_DeviceFlags"),
    (BT, 4, "DEVPKEY_Bluetooth_ClassOfDevice_Deprecated"),
    (BT, 5, "DEVPKEY_Bluetooth_LastConnectedTime_Deprecated"),
    (BT, 6, "DEVPKEY_Bluetooth_DeviceVIDSource"),
    (BT, 7, "DEVPKEY_Bluetooth_DeviceVID"),
    (BT, 8, "DEVPKEY_Bluetooth_DevicePID"),
    (BT, 9, "DEVPKEY_Bluetooth_DeviceProductVersion"),
    (BT, 10, "DEVPKEY_Bluetooth_ClassOfDevice"),
    (BT, 11, "DEVPKEY_Bluetooth_LastConnectedTime"),
    (BT, 12, "DEVPKEY_Bluetooth_LastSeenTime"),
    (0x104ea319_6ee2_4701_bd47_8ddbf425bbe5, 2, "Bluetooth battery level (%)"),
    (0x104ea319_6ee2_4701_bd47_8ddbf425bbe5, 7, "Bluetooth battery level updated"),
    (RADIO, 1, "DEVPKEY_BluetoothRadio_Address"),
    (RADIO, 2, "DEVPKEY_BluetoothRadio_Manufacturer"),
    (RADIO, 3, "DEVPKEY_BluetoothRadio_LmpSupportedFeatures"),
    (RADIO, 4, "DEVPKEY_BluetoothRadio_LmpVersion"),
    (RADIO, 5, "DEVPKEY_BluetoothRadio_LmpSubversion"),
    (RADIO, 6, "DEVPKEY_BluetoothRadio_HciVersion"),
    (RADIO, 7, "DEVPKEY_BluetoothRadio_HciRevision"),
    (RADIO, 8, "DEVPKEY_BluetoothRadio_LeSupportedFeatures"),
    (0x29ce83d4_7a82_4744_bd1d_abec85321dd6, 2, "A2DP codec info present (undocumented)"),
    (0x29ce83d4_7a82_4744_bd1d_abec85321dd6, 3, "A2DP active codec (undocumented)"),
    (0x29ce83d4_7a82_4744_bd1d_abec85321dd6, 4, "A2DP headset codecs (undocumented)"),
    (0xb3f8fa53_0004_438e_9003_51a46e139bfc, 2, "Endpoint function devnode (undocumented)"),
    (0xb3f8fa53_0004_438e_9003_51a46e139bfc, 6, "Endpoint device name (undocumented)"),
    (0x233164c8_1b2c_4c7d_bc68_b671687a2567, 1, "Endpoint KS filter interface (undocumented)"),
    (0x9c119480_ddc2_4954_a150_5bd240d454ad, 9, "Endpoint interface id (undocumented)"),
    (0x9c119480_ddc2_4954_a150_5bd240d454ad, 10, "Endpoint instance id (undocumented)"),
    (0xa35996ab_11cf_4935_8b61_a6761081ecdf, 6, "System.Devices.Aep.SignalStrength"),
    (0xa35996ab_11cf_4935_8b61_a6761081ecdf, 12, "System.Devices.Aep.DeviceAddress"),
];

pub fn key_name(k: &DEVPROPKEY) -> Option<&'static str> {
    let g = k.fmtid.to_u128();
    KEY_NAMES
        .iter()
        .chain(HAND_NAMES.iter())
        .find(|(f, p, _)| *f == g && *p == k.pid)
        .map(|(_, _, n)| *n)
}

/// `{GUID} pid` for keys without a name.
pub fn key_label(k: &DEVPROPKEY) -> String {
    match key_name(k) {
        Some(n) => n.to_string(),
        None => format!("{{{:?}}} {}", k.fmtid, k.pid),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sdk(name: &str) -> (u128, u32) {
        let (g, p, _) = KEY_NAMES.iter().find(|(_, _, n)| *n == name).unwrap_or_else(|| panic!("{name} not in SDK table"));
        (*g, *p)
    }

    #[test]
    fn hand_constants_match_the_sdk_headers() {
        let pairs: &[(&str, DEVPROPKEY)] = &[
            ("DEVPKEY_NAME", NAME),
            ("DEVPKEY_Device_DeviceDesc", DEVICE_DESC),
            ("DEVPKEY_Device_HardwareIds", HARDWARE_IDS),
            ("DEVPKEY_Device_CompatibleIds", COMPATIBLE_IDS),
            ("DEVPKEY_Device_Service", SERVICE),
            ("DEVPKEY_Device_Class", CLASS),
            ("DEVPKEY_Device_Manufacturer", MANUFACTURER),
            ("DEVPKEY_Device_FriendlyName", FRIENDLY_NAME),
            ("DEVPKEY_Device_LocationInfo", LOCATION_INFO),
            ("DEVPKEY_Device_UpperFilters", UPPER_FILTERS),
            ("DEVPKEY_Device_LowerFilters", LOWER_FILTERS),
            ("DEVPKEY_Device_EnumeratorName", ENUMERATOR_NAME),
            ("DEVPKEY_Device_Address", ADDRESS),
            ("DEVPKEY_Device_LocationPaths", LOCATION_PATHS),
            ("DEVPKEY_Device_BusReportedDeviceDesc", BUS_REPORTED_DESC),
            ("DEVPKEY_Device_IsPresent", IS_PRESENT),
            ("DEVPKEY_Device_Stack", STACK),
            ("DEVPKEY_Device_ContainerId", CONTAINER_ID),
            ("DEVPKEY_Device_InLocalMachineContainer", IN_LOCAL_MACHINE_CONTAINER),
            ("DEVPKEY_Device_DevNodeStatus", DEVNODE_STATUS),
            ("DEVPKEY_Device_ProblemCode", PROBLEM_CODE),
            ("DEVPKEY_Device_Parent", PARENT),
            ("DEVPKEY_Device_Children", CHILDREN),
            ("DEVPKEY_Device_InstallDate", INSTALL_DATE),
            ("DEVPKEY_Device_FirstInstallDate", FIRST_INSTALL_DATE),
            ("DEVPKEY_Device_LastArrivalDate", LAST_ARRIVAL_DATE),
            ("DEVPKEY_Device_LastRemovalDate", LAST_REMOVAL_DATE),
            ("DEVPKEY_Device_DriverDate", DRIVER_DATE),
            ("DEVPKEY_Device_DriverVersion", DRIVER_VERSION),
            ("DEVPKEY_Device_DriverDesc", DRIVER_DESC),
            ("DEVPKEY_Device_DriverInfPath", DRIVER_INF_PATH),
            ("DEVPKEY_Device_DriverInfSection", DRIVER_INF_SECTION),
            ("DEVPKEY_Device_DriverProvider", DRIVER_PROVIDER),
            ("DEVPKEY_DeviceContainer_Category", CONTAINER_CATEGORY),
            ("PKEY_AudioEndpoint_FormFactor", PKEY_FORM_FACTOR),
            ("PKEY_AudioEndpoint_JackSubType", PKEY_JACK_SUBTYPE),
            ("PKEY_AudioEndpoint_Disable_SysFx", PKEY_DISABLE_SYSFX),
            ("PKEY_AudioEndpoint_PhysicalSpeakers", PKEY_PHYSICAL_SPEAKERS),
            ("PKEY_AudioEndpoint_Supports_EventDriven_Mode", PKEY_EVENT_DRIVEN),
            ("PKEY_AudioEngine_DeviceFormat", PKEY_DEVICE_FORMAT),
            ("PKEY_AudioEngine_OEMFormat", PKEY_OEM_FORMAT),
        ];
        for (name, k) in pairs {
            assert_eq!(sdk(name), (k.fmtid.to_u128(), k.pid), "{name}");
        }
    }

    #[test]
    fn names_resolve() {
        assert_eq!(key_name(&BT_BATTERY), Some("Bluetooth battery level (%)"));
        assert_eq!(key_name(&FRIENDLY_NAME), Some("DEVPKEY_Device_FriendlyName"));
        assert!(key_label(&key(0x1234, 1)).contains("00001234"));
    }
}
