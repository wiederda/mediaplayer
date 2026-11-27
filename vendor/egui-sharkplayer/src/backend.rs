use eframe::glow::{self, HasContext as _};
use libmpv2::render::{OpenGLInitParams, RenderContext, RenderParam, RenderParamApiType, mpv_render_update};
use libmpv2::{Mpv, mpv_error};
use libmpv2::events::{Event, mpv_event_id};
use libmpv2::mpv_end_file_reason;
use std::cell::RefCell;
use std::ffi::{CString, c_void};
use std::rc::Rc;
use std::sync::Arc;
use tracing::{debug, error, trace};

#[cfg(feature = "wgpu")] mod wgpu_backend;

type GpaFn = dyn Fn(&str) -> *mut c_void + Send + Sync;

/// Free function used as `OpenGLInitParams::get_proc_address`.
/// Receives a reference to the per-instance GPA stored in `PlayerState`.
fn resolve_gl_proc(gpa: &Arc<GpaFn>, name: &str) -> *mut c_void { gpa(name) }

macro_rules! cached_prop {
    ($state:expr, $field:ident, $prop:expr, OptionalStr, $validation:expr) => {{
        cached_prop!(
            $state,
            $field,
            $prop,
            || {
                let $field: Option<Rc<str>> = $state.get_optional_property::<String>($prop)?.map(Rc::from);
                Ok($field)
            },
            $validation
        )
    }};
    ($state:expr, $field:ident, $prop:expr, RcStr, $validation:expr) => {{
        cached_prop!(
            $state,
            $field,
            $prop,
            || { Ok(Rc::from($state.mpv.mpv.get_property::<String>($prop)?)) },
            $validation
        )
    }};
    ($state:expr, $field:ident, $prop:expr, OptionF64, $validation:expr) => {{
        cached_prop!(
            $state,
            $field,
            $prop,
            || { $state.get_optional_property::<f64>($prop) },
            $validation
        )
    }};
    ($state:expr, $field:ident, $prop:expr, f64, $validation:expr) => {{
        cached_prop!(
            $state,
            $field,
            $prop,
            || { Ok($state.mpv.mpv.get_property::<f64>($prop)?) },
            $validation
        )
    }};
    ($state:expr, $field:ident, $prop:expr, bool, $validation:expr) => {{
        cached_prop!(
            $state,
            $field,
            $prop,
            || { Ok($state.mpv.mpv.get_property::<bool>($prop)?) },
            $validation
        )
    }};

    ($state:expr, $field:ident, $prop:expr, $init:expr, $validation:expr) => {{ $state.properties.$field.get_or_try_init($init, $validation) }};
}

macro_rules! def_cached_getters {
    (
        $(
            $(#[$attr:meta])*
            $field:ident, $prop:expr, $type:tt $(, $validation:expr)?
        );+ $(;)?
    ) => {
        $(
            $(#[$attr])*
            /// # Errors
            ///
            /// This function returns `Ok(None)` if the property is
            /// unavailable, and returns an `Err(_)` if a libmpv error occured.
            pub fn $field(&self) -> Result<$type, BackendError> {
                def_cached_getters!(@internal self, $field, $prop, $type $(, $validation)?)
            }
        )+
    };

    (@internal $self:ident, $field:ident, $prop:expr, $type:tt, $validation:expr) => {
        cached_prop!($self, $field, $prop, $type, $validation)
    };
    (@internal $self:ident, $field:ident, $prop:expr, $type:tt) => {
        cached_prop!($self, $field, $prop, $type, |_| true)
    };
}

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("This library only functions when egui is using glow, but the GL context was unavailable.")]
    ExpectedGlow,
    #[error("libmpv2: {0}")]
    Mpv(#[from] libmpv2::Error),
    #[error("glow: {0}")]
    Glow(String),
    #[error("Incomplete framebuffer (0x{0:x}); Ensure RGBA8 support.")]
    IncompleteFramebuffer(u32),
    #[error("Tried to use GL resoures after they were destroyed by PlayerState::destroy_gl_resources.")]
    UseAfterDestroy,

    #[cfg(feature = "wgpu")]
    #[error("Failed to load libEGL.so.1: {0}")]
    EglLoad(#[from] khronos_egl::LoadError<libloading::Error>),
    #[cfg(feature = "wgpu")]
    #[error("EGL initialization error: {0}")]
    EglInit(#[from] khronos_egl::Error),
    #[cfg(feature = "wgpu")]
    #[error("No suitable EGL config found.")]
    NoEglConfig,
}

/// An audio or subtitle stream exposed by the currently loaded media.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaTrack {
    pub id: i64,
    pub title: Option<String>,
    pub language: Option<String>,
    pub codec: Option<String>,
    pub selected: bool,
}

/// A DVD or Blu-ray title exposed by libmpv as an edition.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscTitle {
    pub id: i64,
    pub title: Option<String>,
    pub is_default: bool,
}

/// A chapter in the currently selected disc title.
#[derive(Debug, Clone, PartialEq)]
pub struct ChapterInfo {
    pub index: i64,
    pub title: Option<String>,
    pub start_seconds: f64,
}

/// The rendered video frame, dispatched from [`PlayerState::render_frame`].
pub(crate) enum RenderedFrame {
    /// Fast path (glow renderer): a framebuffer ready to be blitted via a
    /// `egui_glow` paint callback.
    GlFramebuffer(GlResources),
    /// Slow path (wgpu renderer): the frame has been read back to the CPU and
    /// uploaded to egui's texture manager. The [`egui::TextureId`] can be
    /// drawn directly with [`egui::Painter::image`].
    #[cfg(feature = "wgpu")]
    EguiTexture(eframe::egui::TextureId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FramebufferSize {
    pub width:  i32,
    pub height: i32,
}

struct MpvContainer {
    // SAFETY: `render_ctx` MUST be dropped prior to `mpv`. Which means that `render_ctx` MUST be prior to
    // `mpv` in the struct ordering.
    ctx: RenderContext<'static>,
    mpv: Mpv,
}

#[derive(Clone, Copy)]
pub(crate) struct GlResources {
    texture:          glow::NativeTexture,
    framebuffer:      glow::NativeFramebuffer,
    framebuffer_size: FramebufferSize,
}

impl GlResources {
    pub(crate) fn framebuffer(&self) -> glow::NativeFramebuffer { self.framebuffer }

    pub(crate) fn framebuffer_size(&self) -> FramebufferSize { self.framebuffer_size }
}

#[derive(Debug, Default)]
struct CachedProperty<T: Clone + std::fmt::Debug>(RefCell<Option<T>>);
impl<T: Clone + std::fmt::Debug> CachedProperty<T> {
    fn clear(&self) { self.0.replace(None); }

    fn get_or_try_init<E, I, S>(&self, init: I, store: S) -> Result<T, E>
    where
        I: FnOnce() -> Result<T, E>,
        S: FnOnce(&T) -> bool, {
        if let Some(v) = self.0.borrow().clone() {
            return Ok(v);
        }

        match init() {
            Ok(val) if store(&val) => {
                debug!("Caching property: {val:?}");
                self.0.replace(Some(val.clone()));
                Ok(val)
            }
            Ok(val) => Ok(val),
            Err(e) => Err(e),
        }
    }
}

type OptionalStr = Option<Rc<str>>;
type OptionF64 = Option<f64>;

#[derive(Debug, Default)]
struct Properties {
    paused: CachedProperty<bool>,
    volume: CachedProperty<f64>,
    muted:  CachedProperty<bool>,

    // --- File Specific Properties ---
    colormatrix:     CachedProperty<OptionalStr>,
    container_fps:   CachedProperty<Option<f64>>,
    current_demuxer: CachedProperty<OptionalStr>,
    dimensions:      CachedProperty<Option<(i64, i64)>>,
    duration:        CachedProperty<Option<f64>>,
    file_format:     CachedProperty<OptionalStr>,
    filename:        CachedProperty<OptionalStr>,
    hwdec_current:   CachedProperty<OptionalStr>,
    media_title:     CachedProperty<OptionalStr>,
    video_codec:     CachedProperty<OptionalStr>,
    video_format:    CachedProperty<OptionalStr>,
}

impl Properties {
    fn clear(&self) {
        self.paused.clear();
        self.colormatrix.clear();
        self.container_fps.clear();
        self.current_demuxer.clear();
        self.dimensions.clear();
        self.duration.clear();
        self.file_format.clear();
        self.filename.clear();
        self.hwdec_current.clear();
        self.media_title.clear();
        self.video_codec.clear();
        self.video_format.clear();
    }
}

/// The persistant state for the video player. This must be created on the UI
/// thread and persist across frames.
pub struct PlayerState {
    mpv:          MpvContainer,
    default_dvd_device: String,
    default_bluray_device: String,
    gl:           Arc<glow::Context>,
    _gpa:         Arc<GpaFn>,
    gl_resources: Option<GlResources>,
    properties:   Properties,

    #[cfg(feature = "wgpu")]
    wgpu: wgpu_backend::WgpuState,
}

impl PlayerState {
    def_cached_getters!(
        colormatrix, "colormatrix", OptionalStr;
        container_fps, "container-fps", OptionF64;
        current_demuxer, "current-demuxer", OptionalStr;
        /// Get the file format of the currently loaded media.
        file_format, "file-format", OptionalStr;
        /// Get the filename of the currently loaded media.
        filename, "filename", OptionalStr;
        /// Get which hardware decoder is being used, if any.
        hwdec_current, "hwdec-current", OptionalStr;
        /// Get the title of the current media.
        media_title, "media_title", OptionalStr;
        /// Get whether playback is muted.
        muted, "mute", bool;
        /// Get the codec of the current video.
        video_codec, "video-codec", OptionalStr;
        /// Get the format of the current video.
        video_format, "video-format", OptionalStr;
        /// Get the current playback volume
        volume, "volume", f64;
        /// Get whether playback is paused.
        paused, "pause", bool;
        /// Get the total duration of the current video in seconds.
        duration, "duration", OptionF64, |dur| dur.is_some_and(|dur| dur > 0.);
    );

    /// Get the dimensions of the loaded video.
    ///
    /// Upon success, this function returns `Ok(Some(width, height))`.
    ///
    /// # Errors
    ///
    /// If the dimensions are unavailable, it returns `Ok(None)`. An `Err(_)` is
    /// returned in the event of a [`BackendError`].
    pub fn dimensions(&self) -> Result<Option<(i64, i64)>, BackendError> {
        self.properties.dimensions.get_or_try_init(
            || {
                let w = self.get_optional_property::<i64>("video-out-params/dw")?;
                let h = self.get_optional_property::<i64>("video-out-params/dh")?;
                Ok(w.zip(h))
            },
            |dimensions|
            // Only cache valid dimensions. If we request prior to the first frame being
            // decoded, we won't get anything.
            dimensions.is_some_and(|(w, h)| w > 0 && h > 0),
        )
    }

    /// Get the aspect ratio of the loaded video.
    /// # Errors
    ///
    /// If the dimensions are unavailable, it returns `Ok(None)`. An `Err(_)` is
    /// returned in the event of a [`BackendError`].
    #[expect(clippy::cast_precision_loss)]
    pub fn aspect_ratio(&self) -> Result<Option<f64>, BackendError> {
        Ok(self
            .dimensions()?
            .and_then(|(w, h)| if h != 0 { Some(w as f64 / h as f64) } else { None }))
    }

    /// Create a new [`PlayerState`] using an [`eframe::CreationContext`].
    ///
    /// # Panics
    ///
    /// This function should **never** panic. If it does, this is a bug.
    ///
    /// # Errors
    ///
    /// An `Err(_)` is returned if there was an issue initializing the libmpv
    /// backend.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, BackendError> {
        #[allow(unused_variables, clippy::allow_attributes)]
        let (gl, gpa, is_offscreen) = if let Some(gl) = cc.gl.clone() {
            let proc_addr = cc
                .get_proc_address
                .clone()
                .expect("get_proc_address unavailable despite glow being active");
            let gpa: Arc<GpaFn> = Arc::new(move |name: &str| -> *mut c_void {
                CString::new(name)
                    .ok()
                    .map_or(std::ptr::null_mut(), |s| proc_addr(&s).cast_mut())
            });

            (gl, gpa, false)
        } else {
            cfg_select! {
                feature = "wgpu" => {
                    let (gl, gpa) = Self::init_offscreen_egl()?;
                    let is_offscreen = true;
                    (gl, gpa, is_offscreen)
                }
                _ => {
                    return Err(BackendError::ExpectedGlow);
                }
            }
        };

        let mpv = Mpv::new()?;
        let default_dvd_device = mpv.get_property::<String>("options/dvd-device").unwrap_or_default();
        let default_bluray_device = mpv.get_property::<String>("options/bluray-device").unwrap_or_default();
        mpv.enable_event(mpv_event_id::EndFile)?;
        mpv.enable_event(mpv_event_id::FileLoaded)?;
        mpv.set_property("terminal", "no")?;
        mpv.set_property("vo", "libmpv")?;
        mpv.set_property("hwdec", "auto-safe")?;
        // Support HDR video
        mpv.set_property("tone-mapping", "auto")?;
        mpv.set_property("video-timing-offset", 0.0_f64)?;
        // Ensure that completed videos are rewatchable.
        mpv.set_property("keep-open", "yes")?;

        #[cfg(feature = "wgpu")]
        if is_offscreen {
            wgpu_backend::OFFSCREEN_EGL.get().unwrap().make_current()?;
        }
        let render_ctx = mpv.create_render_context([
            RenderParam::ApiType(RenderParamApiType::OpenGl),
            RenderParam::InitParams(OpenGLInitParams {
                get_proc_address: resolve_gl_proc,
                ctx:              Arc::clone(&gpa),
            }),
        ])?;
        #[cfg(feature = "wgpu")]
        if is_offscreen {
            wgpu_backend::OFFSCREEN_EGL.get().unwrap().unbind()?;
        }

        // SAFETY: This transmutation is done to erase the lifetime, but it's safe so
        // long as the render context is dropped before `mpv`. This is the case because
        // it's placed in `MpvContainer`, which is a private struct with the correct
        // drop order.
        let mut render_ctx: RenderContext<'static> = unsafe { std::mem::transmute(render_ctx) };
        let egui_ctx = cc.egui_ctx.clone();
        // This update callback is what ensures we show frames when we need to.
        render_ctx.set_update_callback(move || {
            egui_ctx.request_repaint();
        });

        Ok(Self {
            mpv: MpvContainer { ctx: render_ctx, mpv },
            default_dvd_device,
            default_bluray_device,
            gl,
            _gpa: gpa,
            gl_resources: None,
            properties: Properties::default(),

            #[cfg(feature = "wgpu")]
            wgpu: wgpu_backend::WgpuState {
                is_offscreen,
                frame_texture: None,
                pixel_buffer: Vec::new(),
            },
        })
    }

    pub(crate) fn render_frame(
        &mut self,
        ctx: &eframe::egui::Context,
        size: FramebufferSize,
    ) -> Result<RenderedFrame, BackendError> {
        #[cfg(feature = "wgpu")]
        if self.wgpu.is_offscreen {
            return self.render_frame_offscreen(ctx, size);
        }

        let _ = ctx;
        self.render_glow(size)
            .map(|(res, _)| RenderedFrame::GlFramebuffer(res))
    }

    /// Get the framebuffer associated with the current frame, resizing if
    /// necessary.
    pub(crate) fn render_glow(&mut self, size: FramebufferSize) -> Result<(GlResources, bool), BackendError> {
        let (res, reallocated) = if let Some(res) = self.gl_resources.take() {
            // Reallocate framebuffer
            if res.framebuffer_size != size && size.width > 0 && size.height > 0 {
                // SAFETY:
                //
                // 1. This is on the UI thread because `Self::new` guarantees as much.
                unsafe {
                    self.gl.delete_framebuffer(res.framebuffer);
                    self.gl.delete_texture(res.texture);
                }

                // SAFETY: If allocation fails, then gl_resources is `None` so there are no
                // use-after-free errors.
                let new_res = unsafe { Self::allocate_framebuffer(&self.gl, size)? };
                self.gl_resources = Some(new_res);
                (new_res, true)
            } else {
                self.gl_resources = Some(res);
                (res, false)
            }
        } else {
            let res = unsafe { Self::allocate_framebuffer(&self.gl, size)? };
            self.gl_resources = Some(res);
            (res, true)
        };

        let flags = self.mpv.ctx.update().unwrap_or(0);
        let frame_updated = reallocated || flags & mpv_render_update::Frame != 0;
        if frame_updated {
            match res.framebuffer.0.get().try_into() {
                Ok(fb) => self.mpv.ctx.render::<()>(fb, size.width, size.height, true)?,
                Err(e) => {
                    error!("GL framebuffer ID is too large for libmpv2: {e}");
                }
            }
        }

        Ok((res, frame_updated))
    }

    /// Load the file at `path`.
    ///
    /// # Errors
    ///
    /// An `Err(_)` is returned in the event of a [`BackendError`].
    pub fn load_file(&self, path: impl AsRef<str>) -> Result<(), BackendError> {
        let path = path.as_ref();

        self.properties.clear();

        self.mpv.mpv.command("loadfile", &[path, "replace"])?;
        Ok(())
    }

    /// Stop playback and unload the current file.
    ///
    /// # Errors
    ///
    /// An `Err(_)` is returned in the event of a [`BackendError`].
    pub fn stop(&self) -> Result<(), BackendError> {
        self.properties.clear();
        self.mpv.mpv.command("stop", &[])?;
        Ok(())
    }

    /// Report whether this libmpv build can play audio CDs (`cdda://`).
    ///
    /// Audio CD support is optional in mpv and requires libcdio. The
    /// `cdda-device` option only exists when mpv was built with it.
    pub fn supports_audio_cd(&self) -> bool {
        self.mpv.mpv.get_property::<String>("options/cdda-device").is_ok()
    }

    /// Configure read-ahead and rebuffering before loading an audio CD.
    pub fn configure_audio_cd_buffering(&self) -> Result<(), BackendError> {
        self.mpv.mpv.set_property("cache", "yes")?;
        self.mpv.mpv.set_property("demuxer-max-bytes", "32MiB")?;
        self.mpv.mpv.set_property("cache-secs", 120.0_f64)?;
        self.mpv.mpv.set_property("cache-pause", "yes")?;
        self.mpv.mpv.set_property("cache-pause-initial", "yes")?;
        self.mpv.mpv.set_property("cache-pause-wait", 5.0_f64)?;
        self.mpv.mpv.set_property("cdda-cdtext", "yes")?;
        Ok(())
    }

    /// Select the DVD image/device and request its interactive menu on load.
    pub fn configure_dvd(&self, device: Option<&str>) -> Result<(), BackendError> {
        self.mpv.mpv.set_property("disc-menu", "yes")?;
        self.mpv.mpv.set_property("dvd-device", device.unwrap_or(&self.default_dvd_device))?;
        Ok(())
    }

    /// Select the Blu-ray device and request its interactive menu on load.
    pub fn configure_bluray(&self, device: Option<&str>) -> Result<(), BackendError> {
        self.mpv.mpv.set_property("disc-menu", "yes")?;
        self.mpv.mpv.set_property("bluray-device", device.unwrap_or(&self.default_bluray_device))?;
        Ok(())
    }

    /// Apply the preferred audio and subtitle languages for media playback.
    pub fn configure_playback_languages(
        &self,
        audio_language: &str,
        subtitle_language: &str,
        subtitles_enabled: bool,
    ) -> Result<(), BackendError> {
        self.mpv.mpv.set_property("alang", audio_language)?;
        self.mpv.mpv.set_property("slang", subtitle_language)?;
        self.mpv.mpv.set_property("sub-visibility", subtitles_enabled)?;
        Ok(())
    }

    /// Jump to the currently loaded optical-disc menu (DVD or Blu-ray).
    pub fn show_disc_menu(&self) -> Result<(), BackendError> {
        self.mpv.mpv.command("discnav", &["menu"])?;
        Ok(())
    }

    /// Forward pointer movement or a click to the currently loaded disc menu.
    pub fn dvd_menu_pointer(&self, x: f64, y: f64, click: bool) -> Result<(), BackendError> {
        let action = if click { "mouse-click" } else { "mouse-move" };
        let x = x.clamp(0.0, 1.0).to_string();
        let y = y.clamp(0.0, 1.0).to_string();
        self.mpv.mpv.command("discnav", &[action, &x, &y])?;
        Ok(())
    }

    /// Read the album title exposed by the CDDA demuxer, if available.
    pub fn audio_cd_title(&self) -> Result<Option<String>, BackendError> {
        let album = self.get_optional_property::<String>("metadata/album")?;
        Ok(album.filter(|title| !title.trim().is_empty()))
    }

    /// Drain pending libmpv events and report file loading and natural end-of-file.
    ///
    /// # Errors
    ///
    /// Returns an error when libmpv reports that loading or playback failed.
    pub fn poll_file_events(&self) -> Result<(bool, bool), BackendError> {
        let mut file_loaded = false;
        let mut reached_eof = false;
        while let Some(event) = self.mpv.mpv.wait_event(0.0) {
            match event? {
                Event::FileLoaded => file_loaded = true,
                Event::EndFile(reason) if reason == mpv_end_file_reason::Eof => reached_eof = true,
                _ => {}
            }
        }
        Ok((file_loaded, reached_eof))
    }

    /// List the audio streams of the currently loaded media.
    pub fn audio_tracks(&self) -> Result<Vec<MediaTrack>, BackendError> {
        self.tracks_of_type("audio")
    }

    /// List the subtitle streams of the currently loaded media.
    pub fn subtitle_tracks(&self) -> Result<Vec<MediaTrack>, BackendError> {
        self.tracks_of_type("sub")
    }

    fn tracks_of_type(&self, media_type: &str) -> Result<Vec<MediaTrack>, BackendError> {
        let count = self.mpv.mpv.get_property::<i64>("track-list/count")?;
        let mut tracks = Vec::new();
        for index in 0..count {
            let property = |name: &str| format!("track-list/{index}/{name}");
            if self.get_optional_property::<String>(&property("type"))?.as_deref() != Some(media_type) {
                continue;
            }
            let Some(id) = self.get_optional_property::<i64>(&property("id"))? else { continue; };
            tracks.push(MediaTrack {
                id,
                title: self.get_optional_property::<String>(&property("title"))?,
                language: self.get_optional_property::<String>(&property("lang"))?,
                codec: self.get_optional_property::<String>(&property("codec-desc"))?,
                selected: self.get_optional_property::<bool>(&property("selected"))?.unwrap_or(false),
            });
        }
        Ok(tracks)
    }

    /// Select an audio stream, or restore mpv's automatic stream selection.
    pub fn set_audio_track(&self, id: Option<i64>) -> Result<(), BackendError> {
        match id {
            Some(id) => self.mpv.mpv.set_property("aid", id)?,
            None => self.mpv.mpv.set_property("aid", "auto")?,
        }
        Ok(())
    }

    /// Select a subtitle stream, or turn subtitles off.
    pub fn set_subtitle_track(&self, id: Option<i64>) -> Result<(), BackendError> {
        match id {
            Some(id) => self.mpv.mpv.set_property("sid", id)?,
            None => self.mpv.mpv.set_property("sid", "no")?,
        }
        Ok(())
    }

    /// List DVD titles reported by libmpv as editions.
    pub fn disc_titles(&self) -> Result<Vec<DiscTitle>, BackendError> {
        let count = self.mpv.mpv.get_property::<i64>("edition-list/count")?;
        let mut titles = Vec::new();
        for index in 0..count {
            let property = |name: &str| format!("edition-list/{index}/{name}");
            let id = self.get_optional_property::<i64>(&property("id"))?.unwrap_or(index);
            titles.push(DiscTitle {
                id,
                title: self.get_optional_property::<String>(&property("title"))?,
                is_default: self.get_optional_property::<bool>(&property("default"))?.unwrap_or(false),
            });
        }
        Ok(titles)
    }

    /// Return the currently active disc title (mpv edition index).
    pub fn current_disc_title(&self) -> Result<Option<i64>, BackendError> {
        self.get_optional_property("current-edition")
    }

    /// Select a DVD title. Changing the edition restarts playback at that title.
    pub fn set_disc_title(&self, id: i64) -> Result<(), BackendError> {
        self.mpv.mpv.set_property("edition", id)?;
        Ok(())
    }

    /// List chapters in the currently active title.
    pub fn chapters(&self) -> Result<Vec<ChapterInfo>, BackendError> {
        let count = self.mpv.mpv.get_property::<i64>("chapter-list/count")?;
        let mut chapters = Vec::new();
        for index in 0..count {
            let property = |name: &str| format!("chapter-list/{index}/{name}");
            let Some(start_seconds) = self.get_optional_property::<f64>(&property("time"))? else { continue; };
            chapters.push(ChapterInfo {
                index,
                title: self.get_optional_property::<String>(&property("title"))?,
                start_seconds,
            });
        }
        Ok(chapters)
    }

    /// Return the currently active chapter index.
    pub fn current_chapter(&self) -> Result<Option<i64>, BackendError> {
        self.get_optional_property("chapter")
    }

    /// Seek to a chapter by its zero-based index.
    pub fn set_chapter(&self, index: i64) -> Result<(), BackendError> {
        self.mpv.mpv.set_property("chapter", index)?;
        Ok(())
    }

    /// Toggle playback status.
    ///
    /// # Errors
    ///
    /// An `Err(_)` is returned in the event of a [`BackendError`].
    pub fn toggle_pause(&self) -> Result<(), BackendError> {
        let paused = self.paused()?;
        self.mpv.mpv.set_property("pause", !paused)?;
        self.properties.paused.clear();
        trace!("Toggled playback: {}", if paused { "playing" } else { "paused" });
        Ok(())
    }

    /// # Errors
    ///
    /// An `Err(_)` is returned in the event of a [`BackendError`].
    pub fn play(&self) -> Result<(), BackendError> {
        self.mpv.mpv.set_property("pause", false)?;
        self.properties.paused.clear();
        Ok(())
    }

    /// # Errors
    ///
    /// An `Err(_)` is returned in the event of a [`BackendError`].
    pub fn pause(&self) -> Result<(), BackendError> {
        self.mpv.mpv.set_property("pause", true)?;
        self.properties.paused.clear();
        Ok(())
    }

    /// # Errors
    ///
    /// An `Err(_)` is returned in the event of a [`BackendError`].
    pub fn toggle_mute(&self) -> Result<(), BackendError> {
        let is_muted = self.muted()?;
        self.mpv.mpv.set_property("mute", !is_muted)?;
        self.properties.muted.clear();
        Ok(())
    }

    /// Set the playback volume as a percentage in the range `0.0..=100.0`.
    ///
    /// # Errors
    ///
    /// An `Err(_)` is returned in the event of a [`BackendError`].
    pub fn set_volume(&self, volume: f64) -> Result<(), BackendError> {
        self.mpv.mpv.set_property("volume", volume.clamp(0., 100.))?;
        self.properties.volume.clear();
        trace!("Set volume to {volume}%.");
        Ok(())
    }

    /// Get the current playback position in seconds.
    ///
    /// # Errors
    ///
    /// This function returns `Ok(None)` if the `time-pos` property is
    /// unavailable. An `Err(_)` is returned in the event of a [`BackendError`].
    pub fn time_pos(&self) -> Result<Option<f64>, BackendError> { self.get_optional_property("time-pos") }

    /// Seek to an exact timestamp in the video.
    ///  
    /// # Errors
    ///
    /// An `Err(_)` is returned in the event of a [`BackendError`].
    pub fn seek_to(&self, seconds: f64) -> Result<(), BackendError> {
        self.mpv.mpv.set_property("time-pos", seconds)?;
        trace!("Seeked to {seconds:.3}.");
        Ok(())
    }

    /// Seek to an offset relative to the current position.
    ///
    /// # Errors
    ///
    /// An `Err(_)` is returned in the event of a [`BackendError`].
    pub fn seek_relative(&self, seconds: f64) -> Result<(), BackendError> {
        let seconds = seconds.to_string();
        self.mpv.mpv.command("seek", &[&seconds, "relative+exact"])?;
        trace!("Seeked {seconds:.3}.");
        Ok(())
    }

    /// Get the frame drop count.
    ///
    /// # Errors
    ///
    /// An `Err(_)` is returned in the event of a [`BackendError`].
    pub fn frame_drop_count(&self) -> Result<Option<i64>, BackendError> {
        self.get_optional_property("frame-drop-count")
    }

    /// Call this in your app's [`App::on_exit`][eframe::App::on_exit]
    /// implementation to clean up OpenGL resources.
    ///
    /// ```rust
    /// # use egui_sharkplayer::PlayerState;
    /// struct App {
    ///     player: PlayerState,
    /// }
    ///
    /// impl eframe::App for App {
    ///     fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
    ///         self.player.destroy_gl_resources();
    ///     }
    ///     // ...
    /// #   fn ui(&mut self, _ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) { unimplemented!(); }
    /// }
    /// ```
    pub fn destroy_gl_resources(&mut self) {
        if let Some(GlResources {
            framebuffer,
            texture,
            framebuffer_size: _,
        }) = self.gl_resources.take()
        {
            unsafe {
                self.gl.delete_framebuffer(framebuffer);
                self.gl.delete_texture(texture);
            }
        }
    }

    fn get_optional_property<T: libmpv2::GetData>(&self, name: &str) -> Result<Option<T>, BackendError> {
        match self.mpv.mpv.get_property(name) {
            Ok(value) => Ok(Some(value)),
            Err(libmpv2::Error::Raw(e)) if e == mpv_error::PropertyUnavailable => Ok(None),
            Err(e) => Err(BackendError::Mpv(e)),
        }
    }

    /// Allocate an RGBA8 texture and framebuffer.
    ///
    /// # Safety
    ///
    /// 1. GL context MUST be located on the current thread.
    #[expect(unsafe_op_in_unsafe_fn)]
    unsafe fn allocate_framebuffer(
        gl: &glow::Context,
        size: FramebufferSize,
    ) -> Result<GlResources, BackendError> {
        let FramebufferSize { width, height } = size;

        let texture = gl.create_texture().map_err(BackendError::Glow)?;
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::RGBA8.cast_signed(),
            width,
            height,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(None),
        );

        for (name, val) in [
            (glow::TEXTURE_MIN_FILTER, glow::LINEAR.cast_signed()),
            (glow::TEXTURE_MAG_FILTER, glow::LINEAR.cast_signed()),
            (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE.cast_signed()),
            (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE.cast_signed()),
        ] {
            gl.tex_parameter_i32(glow::TEXTURE_2D, name, val);
        }
        gl.bind_texture(glow::TEXTURE_2D, None);

        let framebuffer = gl.create_framebuffer().map_err(BackendError::Glow)?;
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));

        gl.framebuffer_texture_2d(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::TEXTURE_2D,
            Some(texture),
            0,
        );

        let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
        if status != glow::FRAMEBUFFER_COMPLETE {
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            // NOTE: dellocate to prevent memory leaks.
            gl.delete_framebuffer(framebuffer);
            gl.delete_texture(texture);
            return Err(BackendError::IncompleteFramebuffer(status));
        }

        gl.bind_framebuffer(glow::FRAMEBUFFER, None);

        Ok(GlResources {
            texture,
            framebuffer,
            framebuffer_size: size,
        })
    }
}
