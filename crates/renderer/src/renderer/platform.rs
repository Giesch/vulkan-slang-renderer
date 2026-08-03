use std::ffi::CStr;

#[cfg(target_os = "linux")]
pub const OS_SURFACE_EXT: &CStr = ash::khr::xlib_surface::NAME;
#[cfg(target_os = "windows")]
pub const OS_SURFACE_EXT: &CStr = ash::khr::win32_surface::NAME;
#[cfg(target_os = "macos")]
pub const OS_SURFACE_EXT: &CStr = ash::mvk::macos_surface::NAME;

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
pub const ADDITIONAL_INSTANCE_EXTENSIONS: [&CStr; 0] = [];
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub const ADDITIONAL_INSTANCE_EXTENSIONS: [&CStr; 2] = [
    ash::khr::portability_enumeration::NAME,
    ash::khr::get_physical_device_properties2::NAME,
];

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
pub fn instance_create_flags() -> ash::vk::InstanceCreateFlags {
    ash::vk::InstanceCreateFlags::default()
}
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub fn instance_create_flags() -> ash::vk::InstanceCreateFlags {
    ash::vk::InstanceCreateFlags::ENUMERATE_PORTABILITY_KHR
}
