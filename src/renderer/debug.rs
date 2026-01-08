use ash::vk;

use std::ffi::CStr;
use std::marker::PhantomData;
use std::os::raw::c_void;
use std::ptr;

use ash::ext::debug_utils;
use log::*;

// based on the logging callback here:
// https://github.com/unknownue/vulkan-tutorial-rust/blob/master/src/utility/debug.rs#L8

unsafe extern "system" fn vulkan_debug_utils_callback(
    message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    message_type: vk::DebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _p_user_data: *mut c_void,
) -> vk::Bool32 {
    use vk::DebugUtilsMessageTypeFlagsEXT as MessageType;
    let message_type = match message_type {
        MessageType::GENERAL => "[General]",
        MessageType::PERFORMANCE => "[Performance]",
        MessageType::VALIDATION => "[Validation]",
        _ => "[Unknown]",
    };

    let message = unsafe { CStr::from_ptr((*p_callback_data).p_message) };
    let message = message.to_string_lossy();
    let prefixed_message = format!("{message_type} {message}");

    use vk::DebugUtilsMessageSeverityFlagsEXT as Severity;
    match message_severity {
        Severity::ERROR => error!("{prefixed_message}"),
        Severity::WARNING => warn!("{prefixed_message}"),
        Severity::VERBOSE => debug!("{prefixed_message}"),

        Severity::INFO if prefixed_message.contains("DebugPrintf:\n") => {
            let mut split = prefixed_message.split("DebugPrintf:");
            let _vulkan_prefix = split.next().unwrap();
            let printf_message = split.next().unwrap();
            info!("{printf_message}")
        }

        _info_or_other => info!("{prefixed_message}"),
    };

    vk::FALSE
}

pub fn maybe_create_debug_messager_extension(
    entry: &ash::Entry,
    vk_instance: &ash::Instance,
    messenger_create_info: &vk::DebugUtilsMessengerCreateInfoEXT<'static>,
) -> (debug_utils::Instance, vk::DebugUtilsMessengerEXT) {
    let loader = debug_utils::Instance::new(entry, vk_instance);
    if !super::ENABLE_VALIDATION {
        return (loader, vk::DebugUtilsMessengerEXT::null());
    }

    let extension = unsafe {
        loader
            .create_debug_utils_messenger(messenger_create_info, None)
            .expect("failed to create Debug Utils extension messenger")
    };

    (loader, extension)
}

pub fn build_messenger_create_info() -> vk::DebugUtilsMessengerCreateInfoEXT<'static> {
    vk::DebugUtilsMessengerCreateInfoEXT {
        s_type: vk::StructureType::DEBUG_UTILS_MESSENGER_CREATE_INFO_EXT,
        p_next: ptr::null(),
        flags: vk::DebugUtilsMessengerCreateFlagsEXT::empty(),
        message_severity: vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
            | vk::DebugUtilsMessageSeverityFlagsEXT::INFO
            | vk::DebugUtilsMessageSeverityFlagsEXT::VERBOSE
            | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING,
        message_type: vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
            | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE
            | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION,
        pfn_user_callback: Some(vulkan_debug_utils_callback),
        p_user_data: ptr::null_mut(),
        _marker: PhantomData,
    }
}
