use std::ffi::CStr;
use std::ptr;

use crate::MediaError;

/// Information about an available codec.
#[derive(Debug, Clone)]
pub struct CodecInfo {
    pub name: String,
    pub long_name: String,
    pub is_encoder: bool,
    pub is_decoder: bool,
}

/// List available codecs via `av_codec_iterate`.
pub fn list_codecs() -> Result<Vec<CodecInfo>, MediaError> {
    let mut codecs = Vec::new();
    let mut opaque: *mut libc::c_void = ptr::null_mut();

    loop {
        let codec = unsafe { ffmpeg_next::ffi::av_codec_iterate(&mut opaque) };
        if codec.is_null() {
            break;
        }
        unsafe {
            let name = CStr::from_ptr((*codec).name)
                .to_str()
                .unwrap_or("")
                .to_string();

            let long_name_ptr = (*codec).long_name;
            let long_name = if long_name_ptr.is_null() {
                String::new()
            } else {
                CStr::from_ptr(long_name_ptr)
                    .to_str()
                    .unwrap_or("")
                    .to_string()
            };

            let is_encoder = ffmpeg_next::ffi::av_codec_is_encoder(codec) != 0;
            let is_decoder = ffmpeg_next::ffi::av_codec_is_decoder(codec) != 0;

            codecs.push(CodecInfo {
                name,
                long_name,
                is_encoder,
                is_decoder,
            });
        }
    }

    Ok(codecs)
}

/// List available hardware acceleration methods.
pub fn list_hwaccels() -> Result<Vec<String>, MediaError> {
    let mut accels = Vec::new();
    let mut hw_type = ffmpeg_next::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_NONE;

    loop {
        hw_type = unsafe { ffmpeg_next::ffi::av_hwdevice_iterate_types(hw_type) };
        if hw_type == ffmpeg_next::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_NONE {
            break;
        }
        let name_ptr = unsafe { ffmpeg_next::ffi::av_hwdevice_get_type_name(hw_type) };
        if !name_ptr.is_null() {
            let name = unsafe { CStr::from_ptr(name_ptr).to_str().unwrap_or("").to_string() };
            if !name.is_empty() {
                accels.push(name);
            }
        }
    }

    Ok(accels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_codecs_returns_non_empty() {
        let codecs = list_codecs().unwrap();
        assert!(!codecs.is_empty(), "should find at least one codec");
    }

    #[test]
    fn test_list_codecs_contains_h264() {
        let codecs = list_codecs().unwrap();
        let has_h264 = codecs
            .iter()
            .any(|c| c.name == "h264" || c.name == "libx264");
        assert!(has_h264, "should find h264 codec");
    }

    #[test]
    fn test_list_codecs_has_encoders_and_decoders() {
        let codecs = list_codecs().unwrap();
        let has_encoder = codecs.iter().any(|c| c.is_encoder);
        let has_decoder = codecs.iter().any(|c| c.is_decoder);
        assert!(has_encoder, "should find at least one encoder");
        assert!(has_decoder, "should find at least one decoder");
    }

    #[test]
    fn test_codec_info_clone_and_debug() {
        let info = CodecInfo {
            name: "test".to_string(),
            long_name: "Test Codec".to_string(),
            is_encoder: true,
            is_decoder: false,
        };
        let cloned = info.clone();
        assert_eq!(cloned.name, "test");
        assert_eq!(cloned.long_name, "Test Codec");
        assert!(cloned.is_encoder);
        assert!(!cloned.is_decoder);

        let debug = format!("{info:?}");
        assert!(debug.contains("CodecInfo"));
    }

    #[test]
    fn test_list_hwaccels_returns_ok() {
        // On macOS with VideoToolbox, this should return at least one entry.
        // On other systems it may be empty, but should not error.
        let accels = list_hwaccels().unwrap();
        // Just verify it's a valid Vec.
        let _ = accels.len();
    }

    #[test]
    fn test_list_hwaccels_on_macos_has_videotoolbox() {
        let accels = list_hwaccels().unwrap();
        if cfg!(target_os = "macos") {
            let has_vt = accels.iter().any(|a| a == "videotoolbox");
            assert!(has_vt, "macOS should have videotoolbox, got: {accels:?}");
        }
    }

    #[test]
    fn test_list_codecs_long_names_not_all_empty() {
        let codecs = list_codecs().unwrap();
        let has_long_name = codecs.iter().any(|c| !c.long_name.is_empty());
        assert!(has_long_name, "at least some codecs should have long names");
    }
}
