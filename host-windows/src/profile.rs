/// Adaptive stream profile: device resolution + transport + encoder caps.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportKind {
    Usb,
    Wifi,
    Unknown,
}

impl TransportKind {
    pub fn parse(value: Option<&str>) -> Self {
        match value.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
            Some("usb") => Self::Usb,
            Some("wifi") | Some("wi-fi") | Some("lan") => Self::Wifi,
            _ => Self::Unknown,
        }
    }

    pub fn default_fps(self) -> u32 {
        match self {
            Self::Usb => 60,
            Self::Wifi | Self::Unknown => 30,
        }
    }

    pub fn default_bitrate_kbps(self) -> u32 {
        match self {
            Self::Usb => 10_000,
            Self::Wifi | Self::Unknown => 5_000,
        }
    }
}

#[derive(Clone, Debug)]
pub struct BaseProfileRequest {
    pub preset_active: bool,
    pub preset_w: Option<u32>,
    pub preset_h: Option<u32>,
    pub preset_fps: Option<u32>,
    pub preset_bitrate: Option<u32>,
    pub manual_w: Option<u32>,
    pub manual_h: Option<u32>,
    pub manual_bitrate: Option<u32>,
    pub env_w: Option<u32>,
    pub env_h: Option<u32>,
    pub env_fps: Option<u32>,
    pub env_bitrate: Option<u32>,
    pub client_w: Option<u32>,
    pub client_h: Option<u32>,
    pub client_fps: Option<u32>,
    pub client_bitrate: Option<u32>,
    pub mirror_host_w: Option<u32>,
    pub mirror_host_h: Option<u32>,
    pub transport: TransportKind,
    pub adaptive: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamProfile {
    pub w: u32,
    pub h: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
}

pub fn align_dim(value: u32, min: u32, max: u32) -> u32 {
    let v = value.clamp(min, max);
    let aligned = (v + 8) & !15;
    aligned.clamp(min, max & !15)
}

/// Fit (w,h) inside max box preserving aspect ratio.
pub fn fit_inside(w: u32, h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    if w == 0 || h == 0 {
        return (align_dim(max_w, 320, 3840), align_dim(max_h, 240, 2160));
    }
    let scale = (max_w as f32 / w as f32)
        .min(max_h as f32 / h as f32)
        .min(1.0);
    (
        align_dim((w as f32 * scale).round() as u32, 320, 3840),
        align_dim((h as f32 * scale).round() as u32, 240, 2160),
    )
}

/// Resolve width/height/fps/bitrate before encoder-specific caps.
pub fn resolve_base_profile(req: &BaseProfileRequest) -> StreamProfile {
    if req.preset_active {
        let w = align_dim(req.preset_w.unwrap_or(1280), 320, 3840);
        let h = align_dim(req.preset_h.unwrap_or(720), 240, 2160);
        let fps = req.preset_fps.unwrap_or(60).clamp(10, 60);
        let bitrate = req.preset_bitrate.unwrap_or(10_000).clamp(1000, 50_000);
        return StreamProfile {
            w,
            h,
            fps,
            bitrate_kbps: bitrate,
        };
    }

    let client_w = req.client_w.filter(|&v| v >= 320);
    let client_h = req.client_h.filter(|&v| v >= 240);

    let (mut w, mut h) = match (req.adaptive, client_w, client_h) {
        (true, Some(cw), Some(ch)) => (align_dim(cw, 320, 3840), align_dim(ch, 240, 2160)),
        _ => {
            let rw = req
                .manual_w
                .or(req.env_w)
                .or(req.mirror_host_w)
                .or(client_w)
                .unwrap_or(960);
            let rh = req
                .manual_h
                .or(req.env_h)
                .or(req.mirror_host_h)
                .or(client_h)
                .unwrap_or(540);
            (align_dim(rw, 320, 3840), align_dim(rh, 240, 2160))
        }
    };

    // Soft cap by transport before encoder pass (Android already scales; host double-checks).
    let (max_w, max_h) = match req.transport {
        TransportKind::Usb => (1920, 1200),
        TransportKind::Wifi | TransportKind::Unknown => (1280, 720),
    };
    (w, h) = fit_inside(w, h, max_w, max_h);

    let fps = req
        .client_fps
        .or(req.env_fps)
        .unwrap_or_else(|| req.transport.default_fps())
        .clamp(10, 60);
    let bitrate = req
        .client_bitrate
        .or(req.manual_bitrate)
        .or(req.env_bitrate)
        .unwrap_or_else(|| req.transport.default_bitrate_kbps())
        .clamp(1000, 50_000);

    StreamProfile {
        w,
        h,
        fps,
        bitrate_kbps: bitrate,
    }
}

/// Apply per-encoder + transport limits to produce the FFmpeg profile.
pub fn apply_encoder_profile_caps(
    encoder: &str,
    transport: TransportKind,
    base: StreamProfile,
    preset: Option<&str>,
) -> StreamProfile {
    let mut w = base.w;
    let mut h = base.h;
    let mut fps = base.fps;
    let mut bitrate = base.bitrate_kbps;

    match encoder {
        "libx264" => match preset {
            Some("ahorro" | "cpu_safe") => {
                (w, h) = fit_inside(w, h, 960, 544);
                fps = fps.min(30);
                bitrate = bitrate.clamp(4_000, 6_000);
            }
            Some("equilibrado" | "alta_720p") => {
                (w, h) = fit_inside(w, h, 1280, 720);
                fps = match transport {
                    TransportKind::Usb => fps.min(60),
                    TransportKind::Wifi | TransportKind::Unknown => fps.min(30),
                };
                bitrate = bitrate.clamp(5_000, 15_000);
            }
            Some("fluido_900p") => {
                (w, h) = fit_inside(w, h, 1600, 900);
                fps = match transport {
                    TransportKind::Usb => fps.min(60),
                    TransportKind::Wifi | TransportKind::Unknown => fps.min(30),
                };
                bitrate = bitrate.clamp(8_000, 12_000);
            }
            Some("full_hd" | "full_hd_max") => {
                // 1080p60 is not sustainable on CPU — best-effort 720p60 on USB.
                (w, h) = fit_inside(w, h, 1280, 720);
                fps = match transport {
                    TransportKind::Usb => fps.min(60),
                    TransportKind::Wifi | TransportKind::Unknown => fps.min(30),
                };
                bitrate = bitrate.clamp(6_000, 10_000);
            }
            _ => {
                (w, h) = fit_inside(w, h, 1280, 720);
                fps = fps.min(30);
                bitrate = bitrate.clamp(4_000, 6_000);
            }
        },
        "h264_nvenc" => {
            (w, h) = match transport {
                TransportKind::Usb => fit_inside(w, h, 1920, 1200),
                TransportKind::Wifi | TransportKind::Unknown => fit_inside(w, h, 1280, 720),
            };
            fps = match transport {
                TransportKind::Usb => fps.min(60),
                TransportKind::Wifi | TransportKind::Unknown => fps.min(30),
            };
            bitrate = bitrate.clamp(4_000, 10_000);
        }
        "h264_qsv" => {
            (w, h) = match transport {
                TransportKind::Usb => fit_inside(w, h, 1920, 1200),
                TransportKind::Wifi | TransportKind::Unknown => fit_inside(w, h, 1280, 720),
            };
            fps = match transport {
                TransportKind::Usb => fps.min(60),
                TransportKind::Wifi | TransportKind::Unknown => fps.min(30),
            };
            bitrate = bitrate.clamp(5_000, 10_000);
        }
        "h264_amf" => {
            (w, h) = match transport {
                TransportKind::Usb => fit_inside(w, h, 1920, 1200),
                TransportKind::Wifi | TransportKind::Unknown => fit_inside(w, h, 1280, 720),
            };
            fps = match transport {
                TransportKind::Usb => fps.min(60),
                TransportKind::Wifi | TransportKind::Unknown => fps.min(30),
            };
            bitrate = bitrate.clamp(5_000, 18_000);
        }
        _ => {}
    }

    StreamProfile {
        w,
        h,
        fps,
        bitrate_kbps: bitrate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adaptive_prefers_client_over_mirror() {
        let req = BaseProfileRequest {
            preset_active: false,
            preset_w: None,
            preset_h: None,
            preset_fps: None,
            preset_bitrate: None,
            manual_w: None,
            manual_h: None,
            manual_bitrate: None,
            env_w: None,
            env_h: None,
            env_fps: None,
            env_bitrate: None,
            client_w: Some(1920),
            client_h: Some(1200),
            client_fps: Some(60),
            client_bitrate: Some(8000),
            mirror_host_w: Some(2560),
            mirror_host_h: Some(1440),
            transport: TransportKind::Usb,
            adaptive: true,
        };
        let base = resolve_base_profile(&req);
        assert_eq!(base.w, 1920);
        assert_eq!(base.h, 1200);
        let eff = apply_encoder_profile_caps("h264_nvenc", TransportKind::Usb, base, None);
        assert_eq!(eff.w, 1920);
        assert_eq!(eff.h, 1200);
    }

    #[test]
    fn libx264_full_hd_usb_caps_to_720p60() {
        let base = StreamProfile {
            w: 1920,
            h: 1088,
            fps: 60,
            bitrate_kbps: 25_000,
        };
        let eff = apply_encoder_profile_caps("libx264", TransportKind::Usb, base, Some("full_hd"));
        assert!(eff.w <= 1280);
        assert_eq!(eff.h, 720);
        assert_eq!(eff.fps, 60);
        assert!(eff.bitrate_kbps <= 10_000);
    }

    #[test]
    fn libx264_equilibrado_usb_allows_60fps() {
        let base = StreamProfile {
            w: 1280,
            h: 720,
            fps: 60,
            bitrate_kbps: 10_000,
        };
        let eff = apply_encoder_profile_caps("libx264", TransportKind::Usb, base, Some("equilibrado"));
        assert_eq!(eff.w, 1280);
        assert_eq!(eff.h, 720);
        assert_eq!(eff.fps, 60);
    }

    #[test]
    fn align_dim_rounds_to_nearest_sixteen() {
        assert_eq!(align_dim(1080, 240, 2160), 1088);
        assert_eq!(align_dim(1072, 240, 2160), 1072);
    }

    #[test]
    fn wifi_caps_resolution() {
        let req = BaseProfileRequest {
            preset_active: false,
            preset_w: None,
            preset_h: None,
            preset_fps: None,
            preset_bitrate: None,
            manual_w: None,
            manual_h: None,
            manual_bitrate: None,
            env_w: None,
            env_h: None,
            env_fps: None,
            env_bitrate: None,
            client_w: Some(1920),
            client_h: Some(1200),
            client_fps: None,
            client_bitrate: None,
            mirror_host_w: None,
            mirror_host_h: None,
            transport: TransportKind::Wifi,
            adaptive: true,
        };
        let base = resolve_base_profile(&req);
        assert!(base.w <= 1280);
        assert!(base.h <= 720);
        assert_eq!(base.fps, 30);
    }
}
