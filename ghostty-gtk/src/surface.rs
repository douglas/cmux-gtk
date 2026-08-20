//! GhosttyGlSurface — a GtkGLArea-based widget that hosts a ghostty terminal.
//!
//! This is the core rendering widget. It:
//! - Creates a GtkGLArea for OpenGL rendering
//! - Connects keyboard, mouse, scroll, and IME event controllers
//! - Forwards all events to the ghostty surface via FFI
//! - Manages the ghostty_surface_t lifecycle
//!
//! # Safety
//!
//! All ghostty FFI calls require:
//! - The `ghostty_surface_t` pointer is non-null (stored in `Cell`, checked before each call).
//! - Calls happen on the GTK main thread (enforced by GtkGLArea signal handlers).
//! - `mem::zeroed()` is used for `#[repr(C)]` config structs where all-zeros is valid.
//! - `from_raw_parts()` trusts length values from ghostty (FFI contract).

use ghostty_sys::*;
use glib::translate::IntoGlib;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use std::cell::{Cell, RefCell};
use std::os::raw::c_char;
use std::os::raw::c_void;
use std::ptr;
use std::rc::Rc;

use crate::callbacks::ClipboardContent;
use crate::keys;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ImeKeyEventState {
    #[default]
    Idle,
    NotComposing,
    Composing,
}

fn cstring_input(text: &str, context: &'static str) -> Option<std::ffi::CString> {
    match std::ffi::CString::new(text) {
        Ok(cstr) => Some(cstr),
        Err(_) => {
            tracing::warn!("Ignoring {} containing interior NUL", context);
            None
        }
    }
}

// Minimal GL bindings for viewport setup.
// GtkGLArea does NOT set glViewport before emitting the render signal,
// but ghostty's renderer reads GL_VIEWPORT to determine the surface size.
#[cfg(feature = "link-ghostty")]
mod gl_raw {
    pub type GLint = i32;
    pub type GLsizei = i32;
    pub type GLfloat = f32;
    pub type GLbitfield = u32;
    pub const GL_COLOR_BUFFER_BIT: GLbitfield = 0x00004000;

    #[link(name = "GL")]
    extern "C" {
        pub fn glViewport(x: GLint, y: GLint, width: GLsizei, height: GLsizei);
        pub fn glClearColor(red: GLfloat, green: GLfloat, blue: GLfloat, alpha: GLfloat);
        pub fn glClear(mask: GLbitfield);
    }
}

// -----------------------------------------------------------------------
// Spawn-once guard
// -----------------------------------------------------------------------

/// Which of the two paths that can start a terminal's child process owns the
/// spawn: the first visible allocation (`ensure_spawned`, the normal case) or
/// `spawn_headless` (a workspace that is never mapped).
///
/// Exactly one may call `ghostty_surface_new` — doing it twice would fork a
/// second pty and child and orphan the first — but a *failed* attempt must not
/// lock the surface out for good: a background spawn that could not get a GL
/// context has to be retryable by the allocation path, which is why failure
/// returns to `Idle` rather than sticking at `Spawning`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum SpawnState {
    /// Nothing has tried yet.
    #[default]
    Idle,
    /// An attempt is in flight (re-entrant calls must not start a second one).
    Spawning,
    /// The `ghostty_surface_t` exists: pty open, child running.
    Spawned,
}

impl SpawnState {
    /// May the caller start a spawn attempt now?
    fn may_start(self) -> bool {
        matches!(self, SpawnState::Idle)
    }

    /// State after an attempt that did (or did not) create the surface.
    fn settled(created: bool) -> Self {
        if created {
            SpawnState::Spawned
        } else {
            SpawnState::Idle
        }
    }
}

// -----------------------------------------------------------------------
// GObject subclass for the GL surface widget
// -----------------------------------------------------------------------

mod imp {
    use super::*;

    /// Everything needed to spawn the terminal's pty + child, captured by
    /// `initialize_with_env` and consumed by whichever path starts the child
    /// first: the first visible allocation (normal) or `spawn_headless`
    /// (background workspaces, which are never mapped or allocated).
    pub(super) struct SpawnConfig {
        pub app: ghostty_app_t,
        pub working_directory: Option<String>,
        pub command: Option<String>,
        pub env: Vec<(String, String)>,
    }

    #[derive(Default)]
    pub struct GhosttyGlSurface {
        pub(super) surface: Cell<ghostty_surface_t>,
        pub(super) app: Cell<ghostty_app_t>,
        pub(super) callback_userdata: RefCell<Option<Box<crate::callbacks::SurfaceUserdata>>>,
        pub(super) pending_text: RefCell<Vec<String>>,
        pub(super) title: RefCell<String>,
        pub(super) im_context: RefCell<Option<gtk4::IMMulticontext>>,
        pub(super) im_composing: Cell<bool>,
        pub(super) in_keyevent: Cell<ImeKeyEventState>,
        pub(super) im_commit_text: RefCell<Vec<u8>>,
        #[allow(clippy::type_complexity)]
        pub(super) close_handler: RefCell<Option<Rc<dyn Fn(bool)>>>,
        pub(super) focused: Cell<bool>,
        pub(super) focus_idle_queued: Cell<bool>,
        pub(super) focus_restore_armed: Cell<bool>,
        pub(super) focus_disarm_source: RefCell<Option<glib::SourceId>>,
        pub(super) resize_focus_restore_source: RefCell<Option<glib::SourceId>>,
        /// Grace period after surface creation during which the render
        /// callback paints the initial background color instead of
        /// drawing terminal content, giving the shell time to initialize.
        #[allow(dead_code)] // read via imp() in render callback
        pub(super) created_at: Cell<Option<std::time::Instant>>,
        /// Background color (r, g, b) to paint during the grace period.
        pub(super) initial_bg: Cell<(f32, f32, f32)>,
        /// Backing storage for the C strings passed to `ghostty_surface_new`
        /// via `ghostty_surface_config_s` (working_directory, command, env
        /// vars). Ghostty may read these after `ghostty_surface_new` returns
        /// (the command is spawned lazily), so they must outlive
        /// `create_surface` — keep them alive for the surface's lifetime.
        pub(super) config_cstrings: RefCell<Vec<std::ffi::CString>>,
        /// Backing storage for the `ghostty_env_var_s` array referenced by
        /// `config.env_vars` (same lifetime concern as `config_cstrings`).
        pub(super) config_env_array: RefCell<Vec<ghostty_env_var_s>>,
        /// What to spawn, once. `None` until `initialize_with_env` runs.
        pub(super) pending_spawn: RefCell<Option<SpawnConfig>>,
        /// Spawn-once guard shared by the allocation path and the headless path
        /// (see `SpawnState`): a successful headless spawn is never repeated when
        /// the widget is finally mapped.
        pub(super) spawn: Cell<super::SpawnState>,
        /// True once the widget has had its first non-empty allocation, so the
        /// one-shot focus grab that rides it happens exactly once.
        pub(super) first_allocation: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GhosttyGlSurface {
        const NAME: &'static str = "GhosttyGlSurface";
        type Type = super::GhosttyGlSurface;
        type ParentType = gtk4::GLArea;
    }

    impl ObjectImpl for GhosttyGlSurface {
        fn constructed(&self) {
            self.parent_constructed();

            let gl_area = self.obj();
            // Match Ghostty's GTK surface behavior so resizes and renderer-driven
            // invalidations can produce fresh frames without our own manual loop.
            gl_area.set_auto_render(true);
            gl_area.set_has_depth_buffer(false);
            gl_area.set_has_stencil_buffer(false);
            // Request OpenGL 4.3 (required by ghostty renderer)
            gl_area.set_required_version(4, 3);
            gl_area.set_focusable(true);
            gl_area.set_can_focus(true);
            // Expand to fill the parent container. Without this, GTK gives
            // the GLArea a 0-height natural size inside vertical Boxes, so
            // the resize/render signals never fire → permanent black screen.
            gl_area.set_hexpand(true);
            gl_area.set_vexpand(true);

            // Set up IME context
            let im_context = gtk4::IMMulticontext::new();
            *self.im_context.borrow_mut() = Some(im_context);
            gl_area.setup_ime();
        }

        fn dispose(&self) {
            if let Some(source) = self.focus_disarm_source.borrow_mut().take() {
                source.remove();
            }
            if let Some(source) = self.resize_focus_restore_source.borrow_mut().take() {
                source.remove();
            }
            if let Some(im_context) = self.im_context.borrow().as_ref() {
                im_context.set_client_widget(Option::<&gtk4::Widget>::None);
            }

            let surface = self.surface.get();
            if !surface.is_null() {
                // Confirms the surface (and its up-to-10 MB scrollback grid) is
                // actually reclaimed on panel close — the event-controller/IME/
                // action closures used to form a reference cycle that pinned the
                // widget forever, leaking to multi-GB RSS and OOM kills.
                tracing::debug!(?surface, "freeing ghostty surface on dispose");
                #[cfg(feature = "link-ghostty")]
                unsafe {
                    ghostty_surface_free(surface);
                }
                self.surface.set(ptr::null_mut());
            }
            self.callback_userdata.borrow_mut().take();
            self.close_handler.borrow_mut().take();
        }
    }

    impl WidgetImpl for GhosttyGlSurface {
        fn realize(&self) {
            self.parent_realize();
            tracing::debug!("GLArea realize");
            let widget = self.obj();
            widget.make_current();
            if widget.error().is_some() {
                tracing::error!("Failed to make GL context current");
                return;
            }
            // (Re-)initialize renderer GL state while context is current.
            let surface = self.surface.get();
            if !surface.is_null() {
                #[cfg(feature = "link-ghostty")]
                unsafe {
                    ghostty_surface_display_realized(surface);
                }
            }
        }

        fn unrealize(&self) {
            tracing::debug!("GLArea unrealize");
            // Release the renderer GL state that realize() allocated via
            // ghostty_surface_display_realized (glyph atlas, cell buffers, FBOs).
            // GtkGLArea gives no guarantee the context is current on unrealize,
            // so make it current first — mirroring ghostty's own apprt. Without
            // this the renderer's textures orphan in the driver on every
            // reparent/workspace switch (measured: ~27 MB leaked per unrealize,
            // the dominant heap leak over a long session). Must run before
            // parent_unrealize tears the context down.
            let widget = self.obj();
            let surface = self.surface.get();
            if !surface.is_null() {
                widget.make_current();
                if widget.error().is_some() {
                    tracing::warn!(
                        "make_current failed in unrealize — OpenGL resources likely leaked"
                    );
                } else {
                    #[cfg(feature = "link-ghostty")]
                    unsafe {
                        ghostty_surface_display_unrealized(surface);
                    }
                }
            }
            self.parent_unrealize();
        }
    }

    impl GLAreaImpl for GhosttyGlSurface {
        fn create_context(&self) -> Option<gdk4::GLContext> {
            use gdk4::prelude::GLContextExt;
            use gtk4::prelude::NativeExt;
            let widget = self.obj();
            let native = widget.native()?;
            let surface = native.surface()?;
            match surface.create_gl_context() {
                Ok(ctx) => {
                    // Force desktop OpenGL (not GLES) and require 4.3 core profile
                    ctx.set_use_es(0); // 0 = desktop GL, not GLES
                    ctx.set_required_version(4, 3);
                    // Do NOT call ctx.realize() here — GtkGLArea handles that
                    // during its own realize phase with proper FBO setup.
                    Some(ctx)
                }
                Err(e) => {
                    tracing::error!("Failed to create GL context: {}", e);
                    None
                }
            }
        }

        fn render(&self, _context: &gdk4::GLContext) -> glib::Propagation {
            let surface = self.surface.get();
            if !surface.is_null() {
                #[cfg(feature = "link-ghostty")]
                unsafe {
                    let widget = self.obj();
                    let scale = widget.scale_factor();
                    let w = widget.width() * scale;
                    let h = widget.height() * scale;
                    gl_raw::glViewport(0, 0, w, h);

                    // During the creation grace period, show the terminal
                    // background color instead of the mispositioned prompt.
                    if self.created_at.get().is_some() {
                        let (r, g, b) = self.initial_bg.get();
                        gl_raw::glClearColor(r, g, b, 1.0);
                        gl_raw::glClear(gl_raw::GL_COLOR_BUFFER_BIT);
                    } else {
                        ghostty_surface_draw(surface);
                    }
                }
            }
            glib::Propagation::Stop
        }

        fn resize(&self, width: i32, height: i32) {
            let surface = self.surface.get();
            if !surface.is_null() && width > 0 && height > 0 {
                #[cfg(feature = "link-ghostty")]
                unsafe {
                    // GTK4's GLArea resize signal passes physical pixels
                    // (logical_width * scale_factor). Pass them directly
                    // to ghostty — do not multiply by scale again.
                    let scale = self.obj().scale_factor() as f64;
                    ghostty_surface_set_content_scale(surface, scale, scale);
                    ghostty_surface_set_size(surface, width as u32, height as u32);
                }

                self.obj().schedule_resize_focus_restore();
            }
        }
    }
}

glib::wrapper! {
    /// A GtkGLArea that renders a ghostty terminal surface.
    pub struct GhosttyGlSurface(ObjectSubclass<imp::GhosttyGlSurface>)
        @extends gtk4::GLArea, gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget;
}

impl GhosttyGlSurface {
    /// Create a new terminal surface widget.
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    /// Set the background color shown during the initial grace period.
    /// `hex` should be a CSS hex color like `#1a1a2e`.
    pub fn set_initial_bg(&self, hex: &str) {
        if let Some((r, g, b)) = parse_hex_color(hex) {
            self.imp().initial_bg.set((r, g, b));
        }
    }

    /// True once `initialize_with_env` has run: the surface has a ghostty app
    /// and will spawn its shell on first allocation. A widget cached before
    /// ghostty was ready reports false and must be initialized before use, or
    /// it renders as a permanently blank terminal.
    pub fn is_initialized(&self) -> bool {
        !self.imp().app.get().is_null()
    }

    /// True once the `ghostty_surface_t` exists — i.e. the pty is open and the
    /// child (shell or `command=`) has been spawned.
    pub fn has_spawned(&self) -> bool {
        !self.imp().surface.get().is_null()
    }

    /// Spawn the pty + child now, without waiting for a visible allocation.
    ///
    /// A GtkGLArea only gets an allocation when it is mapped, and a workspace
    /// that was created in the background is never mapped — so the agent of a
    /// background goal/graph node would never start. This starts it anyway:
    ///
    /// 1. GTK realizes on *map*, not on parenting, so an unmapped widget has no
    ///    GL context. `realize()` forces one (it walks up and realizes the
    ///    ancestors) without mapping or allocating anything, which is what
    ///    `ghostty_surface_new` needs — libghostty's OpenGL backend loads GLAD
    ///    from the *current* context in `surfaceInit`.
    /// 2. `ghostty_surface_new` opens the pty and spawns the child.
    /// 3. The size that a first allocation would have delivered is applied by
    ///    hand (`width_px`/`height_px`), so the child sees a sane grid instead
    ///    of ghostty's 800×600 fallback.
    ///
    /// Idempotent, and mutually exclusive with the allocation path: the
    /// spawn-once guard means a later real allocation resizes the terminal
    /// (normal ghostty resize) but never spawns a second child.
    ///
    /// Requires the widget to already be in a realized window (park it in an
    /// unmapped container first). Returns false — having changed nothing — when
    /// the surface cannot be created, so the caller can fall back to making the
    /// workspace visible.
    pub fn spawn_headless(&self, width_px: u32, height_px: u32) -> bool {
        let imp = self.imp();
        if self.has_spawned() {
            return true;
        }
        if imp.pending_spawn.borrow().is_none() {
            tracing::warn!(
                "spawn_headless before initialize_with_env — nothing to spawn"
            );
            return false;
        }
        if self.root().is_none() {
            tracing::debug!(
                "spawn_headless: surface is not in a window yet — deferring"
            );
            return false;
        }
        if !self.is_realized() {
            self.realize();
        }
        if !self.is_realized() || self.context().is_none() {
            tracing::warn!(
                realized = self.is_realized(),
                has_context = self.context().is_some(),
                error = ?self.error().map(|e| e.to_string()),
                "spawn_headless: no GL context — cannot create the ghostty surface"
            );
            return false;
        }
        // Never call into libghostty's renderer without a current context: a
        // failed GLAD load would unload the *global* function-pointer table and
        // take every other surface's rendering down with it.
        self.make_current();
        if let Some(err) = self.error() {
            tracing::warn!(error = %err, "spawn_headless: make_current failed");
            return false;
        }

        if !imp.spawn.get().may_start() {
            tracing::debug!("spawn_headless: a spawn is already in flight");
            return false;
        }
        imp.spawn.set(SpawnState::Spawning);
        self.spawn_from_pending();
        imp.spawn.set(SpawnState::settled(self.has_spawned()));
        if !self.has_spawned() {
            // Creation failed — the guard is back to Idle so a real allocation
            // can retry the normal way.
            return false;
        }

        // No allocation will ever arrive while unmapped, so hand ghostty the
        // synthetic geometry the pty should start with. The `resize` vfunc
        // replaces it with the real one if the widget is ever mapped.
        #[cfg(feature = "link-ghostty")]
        {
            let surface = imp.surface.get();
            let scale = self.scale_factor() as f64;
            unsafe {
                ghostty_surface_set_content_scale(surface, scale, scale);
                ghostty_surface_set_size(surface, width_px, height_px);
            }
        }
        tracing::info!(
            width_px,
            height_px,
            "spawned terminal headlessly (no visible allocation)"
        );
        true
    }

    /// Spawn the child if it hasn't started yet. Called by the first-allocation
    /// path; `spawn_headless` is the background equivalent.
    fn ensure_spawned(&self) {
        let imp = self.imp();
        if !imp.spawn.get().may_start() || self.has_spawned() {
            return;
        }
        imp.spawn.set(SpawnState::Spawning);
        self.spawn_from_pending();
        imp.spawn.set(SpawnState::settled(self.has_spawned()));
    }

    /// Create the ghostty surface from the stashed spawn config.
    fn spawn_from_pending(&self) {
        let pending = self.imp().pending_spawn.borrow();
        let Some(cfg) = pending.as_ref() else {
            tracing::warn!("spawn requested before initialize_with_env");
            return;
        };
        let (app, wd, cmd) = (
            cfg.app,
            cfg.working_directory.clone(),
            cfg.command.clone(),
        );
        let env = cfg.env.clone();
        // `create_surface` runs GTK/ghostty callbacks that can re-enter this
        // widget, so the borrow must be released first.
        drop(pending);
        self.create_surface(app, wd.as_deref(), cmd.as_deref(), &env);
    }

    /// Initialize the ghostty surface with the given app.
    ///
    /// This creates the underlying `ghostty_surface_t` and connects all
    /// input event controllers.
    ///
    /// # Safety
    /// The `app` must be a valid ghostty_app_t that outlives this surface.
    pub fn initialize(
        &self,
        app: ghostty_app_t,
        working_directory: Option<&str>,
        command: Option<&str>,
    ) {
        self.initialize_with_env(app, working_directory, command, &[]);
    }

    pub fn initialize_with_env(
        &self,
        app: ghostty_app_t,
        working_directory: Option<&str>,
        command: Option<&str>,
        env_vars: &[(&str, &str)],
    ) {
        let imp = self.imp();
        imp.app.set(app);
        self.setup_event_controllers();

        // Stash what to spawn. Normally the child starts on the first resize —
        // that's when GTK has allocated real pixel dimensions, which the
        // `resize` vfunc then hands to ghostty. A surface that will never be
        // mapped (a background workspace) is started by `spawn_headless`
        // instead; both go through `ensure_spawned`, which spawns once.
        *imp.pending_spawn.borrow_mut() = Some(imp::SpawnConfig {
            app,
            working_directory: working_directory.map(|s| s.to_string()),
            command: command.map(|s| s.to_string()),
            env: env_vars
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        });
        // Weak ref: the widget owns this signal handler, so a strong `self`
        // clone here would be a permanent reference cycle (see setup_event_controllers).
        self.connect_resize(glib::clone!(
            #[weak(rename_to = widget)]
            self,
            move |_w, width, height| {
                let imp = widget.imp();
                tracing::debug!(width, height, spawn = ?imp.spawn.get(), "GLArea resize");
                if width <= 0 || height <= 0 || imp.first_allocation.get() {
                    return;
                }
                imp.first_allocation.set(true);
                // No-op when the child already spawned headlessly; the
                // `resize` vfunc delivers this real size to ghostty either way.
                widget.ensure_spawned();
                widget.grab_focus();
            }
        ));

        // Spawn watchdog: the shell only spawns on the first resize, which
        // requires the widget to be allocated (mapped + visible). If we're
        // mapped but the surface never appeared, allocation was missed — nudge
        // GTK and say so loudly instead of leaving a silently blank terminal
        // (the failure mode that forced the GtkStack revert, 8bb1ab3).
        let weak = self.downgrade();
        glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
            let Some(widget) = weak.upgrade() else { return };
            // A surface parked for headless spawn is deliberately unmapped and
            // unallocated; only complain when GTK owes us an allocation.
            if widget.imp().surface.get().is_null() && widget.is_mapped() {
                tracing::warn!(
                    mapped = widget.is_mapped(),
                    realized = widget.is_realized(),
                    w = widget.width(),
                    h = widget.height(),
                    parent = widget
                        .parent()
                        .map(|p| format!("{}({})", p.type_(), p.is_mapped()))
                        .unwrap_or_default(),
                    "ghostty surface not created 3s after init — forcing an \
                     allocation pass"
                );
                widget.queue_resize();
            }
        });
    }

    #[allow(clippy::needless_return)] // guard clauses before cfg-gated body
    fn create_surface(
        &self,
        app: ghostty_app_t,
        _working_directory: Option<&str>,
        _command: Option<&str>,
        _env_vars: &[(String, String)],
    ) {
        if app.is_null() {
            return;
        }

        if !self.imp().surface.get().is_null() {
            return;
        }
        tracing::debug!("create_surface: creating ghostty surface");

        #[cfg(feature = "link-ghostty")]
        {
            // Ensure GL context is current before creating the surface —
            // ghostty's surfaceInit loads GLAD from the current context.
            self.make_current();

            // Zero-initialize the entire config to avoid garbage in fields
            // we don't explicitly set. In release builds, the optimizer reuses
            // stack slots aggressively and uninitialized fields (env_vars,
            // env_var_count, initial_input, etc.) can contain garbage → SIGSEGV.
            let mut config: ghostty_surface_config_s = unsafe { std::mem::zeroed() };
            config.scale_factor = self.scale_factor() as f64;
            let callback_userdata = Box::new(crate::callbacks::SurfaceUserdata::new(self));

            config.platform_tag = ghostty_platform_e::GHOSTTY_PLATFORM_LINUX;
            config.platform = ghostty_platform_u {
                linux: ghostty_platform_linux_s {
                    gl_area: self.as_ptr() as *mut c_void,
                },
            };
            config.scale_factor = self.scale_factor() as f64;
            config.context = ghostty_surface_context_e::GHOSTTY_SURFACE_CONTEXT_SPLIT;
            // Explicit even though zeroed: EXEC is what opens the pty and
            // spawns the child. (MANUAL would make the embedder pump bytes and
            // ghostty would call `io_write_cb` — see the config struct notes.)
            config.io_mode = ghostty_surface_io_mode_e::GHOSTTY_SURFACE_IO_EXEC;
            config.userdata =
                (&*callback_userdata as *const crate::callbacks::SurfaceUserdata) as *mut c_void;

            let wd_cstr = _working_directory.and_then(|wd| std::ffi::CString::new(wd).ok());
            config.working_directory = wd_cstr.as_ref().map_or(ptr::null(), |c| c.as_ptr());

            if let Some(cmd) = _command {
                tracing::info!(command = %cmd, len = cmd.len(), "Creating ghostty surface with command");
            }
            let cmd_cstr = _command.and_then(|cmd| std::ffi::CString::new(cmd).ok());
            config.command = cmd_cstr.as_ref().map_or(ptr::null(), |c| c.as_ptr());

            // Set environment variables (e.g., scrollback restore file)
            let env_cstrs: Vec<(std::ffi::CString, std::ffi::CString)> = _env_vars
                .iter()
                .filter_map(|(k, v)| {
                    Some((
                        std::ffi::CString::new(k.as_str()).ok()?,
                        std::ffi::CString::new(v.as_str()).ok()?,
                    ))
                })
                .collect();
            let mut env_vars_c: Vec<ghostty_env_var_s> = env_cstrs
                .iter()
                .map(|(k, v)| ghostty_env_var_s {
                    key: k.as_ptr(),
                    value: v.as_ptr(),
                })
                .collect();
            if !env_vars_c.is_empty() {
                config.env_vars = env_vars_c.as_mut_ptr();
                config.env_var_count = env_vars_c.len();
            }

            // The initial pty size is NOT part of the surface config (libghostty
            // has no such field — it starts at 800×600 and is resized). When
            // this runs from the first-allocation path, the `resize` vfunc fires
            // right after us with the real pixel size; the headless path calls
            // `ghostty_surface_set_size` itself.

            // Keep the config C strings alive for the surface's lifetime.
            // Ghostty spawns the command lazily (after this call returns), so
            // these must not be dropped at the end of create_surface or the
            // command/working-directory pointers dangle → garbage exec
            // ("/bin/sh: $'\x85\x01': command not found").
            {
                let mut store = self.imp().config_cstrings.borrow_mut();
                if let Some(c) = wd_cstr {
                    store.push(c);
                }
                if let Some(c) = cmd_cstr {
                    store.push(c);
                }
                for (k, v) in env_cstrs {
                    store.push(k);
                    store.push(v);
                }
            }
            // `config.env_vars` points into `env_vars_c`; the Vec's buffer is
            // stable across this move, so the pointer stays valid.
            *self.imp().config_env_array.borrow_mut() = env_vars_c;

            let surface = unsafe { ghostty_surface_new(app, &config) };
            if surface.is_null() {
                tracing::error!(
                    "ghostty_surface_new returned null — ghostty failed to create the surface"
                );
                return;
            }

            *self.imp().callback_userdata.borrow_mut() = Some(callback_userdata);
            self.imp().surface.set(surface);

            // Brief grace period: the render callback paints the terminal
            // background color instead of calling ghostty_surface_draw,
            // giving the shell time to initialize before we show content.
            self.imp().created_at.set(Some(std::time::Instant::now()));
            self.set_auto_render(false);

            let widget = self.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(200), move || {
                widget.imp().created_at.set(None);
                widget.set_auto_render(true);
                widget.queue_draw();
            });

            self.flush_pending_text();
        }
    }

    fn setup_event_controllers(&self) {
        // All controller closures below hold a *weak* ref to the surface widget.
        // The widget owns each controller, the controller owns its closure — a
        // strong `self` clone here would close a reference cycle that keeps the
        // widget (and its up-to-10 MB ghostty scrollback grid) alive forever,
        // so `dispose()` would never run and the surface would never be freed.
        // See the module-level teardown notes; this was a multi-GB leak.

        // Keyboard events
        let key_controller = gtk4::EventControllerKey::new();
        key_controller.connect_key_pressed(glib::clone!(
            #[weak(rename_to = surface_widget)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |controller, keyval, keycode, state| {
                tracing::trace!(
                    keyval = keyval.into_glib(),
                    keycode,
                    ?state,
                    "key_pressed"
                );
                surface_widget.on_key_event(
                    controller,
                    keyval.into_glib(),
                    keycode,
                    state,
                    ghostty_input_action_e::GHOSTTY_ACTION_PRESS,
                )
            }
        ));
        key_controller.connect_key_released(glib::clone!(
            #[weak(rename_to = surface_widget)]
            self,
            move |controller, keyval, keycode, state| {
                tracing::trace!(
                    keyval = keyval.into_glib(),
                    keycode,
                    ?state,
                    "key_released"
                );
                surface_widget.on_key_event(
                    controller,
                    keyval.into_glib(),
                    keycode,
                    state,
                    ghostty_input_action_e::GHOSTTY_ACTION_RELEASE,
                );
            }
        ));
        self.add_controller(key_controller);

        // Right-click context menu (Copy / Paste)
        let context_menu = self.build_context_menu();
        context_menu.set_parent(self);

        // Mouse click events
        let click = gtk4::GestureClick::new();
        click.set_button(0); // All buttons
        click.connect_pressed(glib::clone!(
            #[weak(rename_to = surface_widget)]
            self,
            move |gesture, _n_press, x, y| {
                // Grab focus on click so key events go to this widget
                surface_widget.grab_focus();
                let button = gesture.current_button();
                surface_widget.on_mouse_button(
                    button,
                    x,
                    y,
                    ghostty_input_mouse_state_e::GHOSTTY_MOUSE_PRESS,
                );
            }
        ));
        // `context_menu` is parented to the widget, so the widget already keeps
        // it alive; a strong capture here doesn't reference `self` back, so it's
        // cycle-free.
        click.connect_released(glib::clone!(
            #[weak(rename_to = surface_widget)]
            self,
            #[strong]
            context_menu,
            move |gesture, _n_press, x, y| {
                let button = gesture.current_button();
                // Right-click: show context menu instead of forwarding to ghostty
                if button == 3 {
                    let rect = gdk4::Rectangle::new(x as i32, y as i32, 1, 1);
                    context_menu.set_pointing_to(Some(&rect));
                    context_menu.popup();
                    return;
                }
                surface_widget.on_mouse_button(
                    button,
                    x,
                    y,
                    ghostty_input_mouse_state_e::GHOSTTY_MOUSE_RELEASE,
                );
            }
        ));
        // If the gesture is cancelled (e.g. widget reparented during a click),
        // send a synthetic mouse release so Ghostty doesn't get stuck in
        // selection mode.
        click.connect_cancel(glib::clone!(
            #[weak(rename_to = surface_widget)]
            self,
            move |_gesture, _sequence| {
                surface_widget.on_mouse_button(
                    1,
                    0.0,
                    0.0,
                    ghostty_input_mouse_state_e::GHOSTTY_MOUSE_RELEASE,
                );
            }
        ));
        self.add_controller(click);

        // Mouse motion events
        let motion = gtk4::EventControllerMotion::new();
        motion.connect_motion(glib::clone!(
            #[weak(rename_to = surface_widget)]
            self,
            move |_controller, x, y| {
                surface_widget.on_mouse_motion(x, y);
            }
        ));
        self.add_controller(motion);

        // Scroll events
        let scroll = gtk4::EventControllerScroll::new(
            gtk4::EventControllerScrollFlags::BOTH_AXES
                | gtk4::EventControllerScrollFlags::DISCRETE,
        );
        scroll.connect_scroll(glib::clone!(
            #[weak(rename_to = surface_widget)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_controller, dx, dy| {
                surface_widget.on_scroll(dx, dy);
                glib::Propagation::Stop
            }
        ));
        self.add_controller(scroll);

        // Focus events
        let focus = gtk4::EventControllerFocus::new();
        focus.connect_enter(glib::clone!(
            #[weak(rename_to = surface_widget)]
            self,
            move |_| {
                surface_widget.on_focus_change(true);
            }
        ));
        focus.connect_leave(glib::clone!(
            #[weak(rename_to = surface_widget)]
            self,
            move |_| {
                surface_widget.on_focus_change(false);
            }
        ));
        self.add_controller(focus);

        // File drag-and-drop — paste shell-escaped paths into the terminal
        let drop_target =
            gtk4::DropTarget::new(gdk4::FileList::static_type(), gdk4::DragAction::COPY);
        drop_target.connect_drop(glib::clone!(
            #[weak(rename_to = surface_widget)]
            self,
            #[upgrade_or]
            false,
            move |_target, value, _x, _y| {
                let Ok(file_list) = value.get::<gdk4::FileList>() else {
                    return false;
                };
                let files = file_list.files();
                if files.is_empty() {
                    return false;
                }
                let paths: Vec<String> = files
                    .iter()
                    .filter_map(|f| f.path())
                    .map(|p| shell_escape(&p.to_string_lossy()))
                    .collect();
                if !paths.is_empty() {
                    let text = paths.join(" ");
                    surface_widget.send_text(&text);
                }
                true
            }
        ));
        self.add_controller(drop_target);
    }

    fn on_key_event(
        &self,
        controller: &gtk4::EventControllerKey,
        keyval: u32,
        keycode: u32,
        state: gdk4::ModifierType,
        action: ghostty_input_action_e,
    ) -> glib::Propagation {
        let surface = self.imp().surface.get();
        if surface.is_null() {
            return glib::Propagation::Proceed;
        }

        let was_composing = self.imp().im_composing.get();
        if action == ghostty_input_action_e::GHOSTTY_ACTION_PRESS {
            if let Some(im_context) = self.imp().im_context.borrow().as_ref() {
                if let Some(event) = controller.current_event() {
                    self.update_ime_cursor_location();
                    self.imp().in_keyevent.set(if was_composing {
                        ImeKeyEventState::Composing
                    } else {
                        ImeKeyEventState::NotComposing
                    });
                    let ime_handled = im_context.filter_keypress(&event);
                    self.imp().in_keyevent.set(ImeKeyEventState::Idle);

                    if ime_handled {
                        let is_composing = self.imp().im_composing.get();
                        let has_committed_text = !self.imp().im_commit_text.borrow().is_empty();
                        if is_composing || was_composing || !has_committed_text {
                            return glib::Propagation::Stop;
                        }
                    }
                }
            }
        }

        let mods = keys::gdk_mods_to_ghostty(state);

        // Convert keyval to a GDK Key for unicode conversion
        let key: gdk4::Key = unsafe { glib::translate::from_glib(keyval) };

        let committed_text = {
            let mut text = self.imp().im_commit_text.borrow_mut();
            std::mem::take(&mut *text)
        };

        let mut text_buf = [0u8; 8];
        let text_cstr;
        let committed_text_cstr;
        let text_ptr = if !committed_text.is_empty() {
            match std::ffi::CString::new(committed_text) {
                Ok(cstr) => {
                    committed_text_cstr = cstr;
                    committed_text_cstr.as_ptr()
                }
                Err(_) => {
                    tracing::warn!("Ignoring IME commit containing interior NUL");
                    ptr::null()
                }
            }
        } else if action == ghostty_input_action_e::GHOSTTY_ACTION_PRESS {
            if let Some(ch) = key.to_unicode() {
                if ch >= '\x20' {
                    let len = ch.encode_utf8(&mut text_buf).len();
                    text_buf[len] = 0;
                    text_cstr = &text_buf[..=len];
                    text_cstr.as_ptr() as *const c_char
                } else {
                    ptr::null()
                }
            } else {
                ptr::null()
            }
        } else {
            ptr::null()
        };

        // Unshifted codepoint: the unicode value of the key without Shift.
        // Translate the hardware keycode with no modifiers but preserving the
        // keyboard group (layout) from the current event.
        let unshifted_codepoint = {
            let display = self.display();
            let group = controller
                .current_event()
                .and_then(|ev| {
                    ev.downcast_ref::<gdk4::KeyEvent>()
                        .map(|ke| ke.layout() as i32)
                })
                .unwrap_or(0);
            if let Some((unshifted_key, _, _, _)) =
                display.translate_key(keycode, gdk4::ModifierType::empty(), group)
            {
                unshifted_key.to_unicode().map(|c| c as u32).unwrap_or(0)
            } else {
                key.to_lower().to_unicode().map(|c| c as u32).unwrap_or(0)
            }
        };

        // Compute consumed modifiers from the GDK event — modifiers that
        // were used by the keyboard layout to produce the character (e.g.,
        // Shift is consumed when producing '?' from '/'). Without this,
        // ghostty sees Shift+? instead of just ? in raw/alternate mode.
        let consumed_mods = controller
            .current_event()
            .and_then(|ev| ev.downcast_ref::<gdk4::KeyEvent>().cloned())
            .map(|ke| keys::gdk_mods_to_ghostty(ke.consumed_modifiers()))
            .unwrap_or(0);

        let key_event = ghostty_input_key_s {
            action,
            mods,
            consumed_mods,
            keycode,
            text: text_ptr,
            unshifted_codepoint,
            composing: self.imp().im_composing.get(),
        };

        #[cfg(feature = "link-ghostty")]
        {
            let handled = unsafe { ghostty_surface_key(surface, key_event) };
            if handled {
                return glib::Propagation::Stop;
            }
        }
        let _ = key_event;

        glib::Propagation::Proceed
    }

    fn on_mouse_button(&self, button: u32, _x: f64, _y: f64, state: ghostty_input_mouse_state_e) {
        let surface = self.imp().surface.get();
        if surface.is_null() {
            return;
        }

        let ghostty_button = keys::gdk_button_to_ghostty(button);

        #[cfg(feature = "link-ghostty")]
        unsafe {
            ghostty_surface_mouse_button(surface, state, ghostty_button, 0);
        }
        let _ = (state, ghostty_button);
    }

    fn on_mouse_motion(&self, x: f64, y: f64) {
        let surface = self.imp().surface.get();
        if surface.is_null() {
            return;
        }

        #[cfg(feature = "link-ghostty")]
        unsafe {
            ghostty_surface_mouse_pos(surface, x, y, 0);
        }
        let _ = (x, y);
    }

    fn on_scroll(&self, dx: f64, dy: f64) {
        let surface = self.imp().surface.get();
        if surface.is_null() {
            return;
        }

        #[cfg(feature = "link-ghostty")]
        unsafe {
            // Ghostty expects positive deltas for up/right and negative for
            // down/left. GTK delivers the inverse "natural scrolling" sign.
            ghostty_surface_mouse_scroll(surface, -dx, -dy, 0);
        }
        let _ = (dx, dy);
    }

    #[allow(clippy::needless_return)] // guard clause before cfg-gated closure body
    fn on_focus_change(&self, focused: bool) {
        tracing::debug!(focused, "surface focus_change");
        self.imp().focused.set(focused);
        let surface = self.imp().surface.get();
        if let Some(im_context) = self.imp().im_context.borrow().as_ref() {
            if focused {
                im_context.focus_in();
                self.update_ime_cursor_location();
            } else {
                self.imp().im_composing.set(false);
                self.imp().im_commit_text.borrow_mut().clear();
                im_context.focus_out();
                im_context.reset();
                self.update_preedit("");
            }
        }

        if focused {
            self.cancel_focus_disarm();
            self.imp().focus_restore_armed.set(true);
        } else {
            self.schedule_focus_disarm();
        }

        if surface.is_null() || self.imp().focus_idle_queued.replace(true) {
            return;
        }

        let surface_widget = self.clone();
        glib::idle_add_local_once(move || {
            let imp = surface_widget.imp();
            imp.focus_idle_queued.set(false);

            let surface = imp.surface.get();
            if surface.is_null() {
                return;
            }

            #[cfg(feature = "link-ghostty")]
            unsafe {
                ghostty_surface_set_focus(surface, imp.focused.get());
            }
        });
    }

    fn schedule_focus_disarm(&self) {
        self.cancel_focus_disarm();

        let surface_widget = self.clone();
        let source =
            glib::timeout_add_local_once(std::time::Duration::from_millis(250), move || {
                surface_widget.imp().focus_disarm_source.borrow_mut().take();
                if !surface_widget.imp().focused.get() {
                    surface_widget.imp().focus_restore_armed.set(false);
                }
            });
        *self.imp().focus_disarm_source.borrow_mut() = Some(source);
    }

    fn cancel_focus_disarm(&self) {
        if let Some(source) = self.imp().focus_disarm_source.borrow_mut().take() {
            source.remove();
        }
    }

    fn schedule_resize_focus_restore(&self) {
        if !self.imp().focus_restore_armed.get() {
            return;
        }

        self.cancel_focus_disarm();

        if let Some(source) = self.imp().resize_focus_restore_source.borrow_mut().take() {
            source.remove();
        }

        let surface_widget = self.clone();
        let source =
            glib::timeout_add_local_once(std::time::Duration::from_millis(150), move || {
                surface_widget
                    .imp()
                    .resize_focus_restore_source
                    .borrow_mut()
                    .take();

                if !surface_widget.imp().focused.get() {
                    let _ = surface_widget.grab_focus();
                }
            });
        *self.imp().resize_focus_restore_source.borrow_mut() = Some(source);
    }

    /// Get the raw ghostty surface pointer.
    pub fn raw_surface(&self) -> ghostty_surface_t {
        self.imp().surface.get()
    }

    /// Request the surface to refresh its rendering.
    pub fn refresh(&self) {
        let surface = self.imp().surface.get();
        if !surface.is_null() {
            #[cfg(feature = "link-ghostty")]
            unsafe {
                ghostty_surface_refresh(surface);
            }
        }
        self.queue_render();
    }

    fn write_text(&self, surface: ghostty_surface_t, text: &str) -> bool {
        #[cfg(feature = "link-ghostty")]
        {
            let Some(cstr) = cstring_input(text, "terminal text input") else {
                return false;
            };
            unsafe {
                ghostty_surface_text(surface, cstr.as_ptr(), text.len());
            }
        }
        let _ = (surface, text);
        true
    }

    #[allow(dead_code)] // called from init_ghostty
    fn flush_pending_text(&self) {
        let surface = self.imp().surface.get();
        if surface.is_null() {
            return;
        }

        let pending = std::mem::take(&mut *self.imp().pending_text.borrow_mut());
        if !pending.is_empty() {
            tracing::debug!(count = pending.len(), "flush_pending_text");
        }
        for text in pending {
            tracing::debug!(text_len = text.len(), text_preview = %&text[..text.len().min(40)], "flush_pending_text: sending");
            let _ = self.write_text(surface, &text);
        }
    }

    /// Execute a ghostty binding action by name (e.g., "open_search", "close_search").
    pub fn binding_action(&self, action: &str) -> bool {
        let surface = self.imp().surface.get();
        if surface.is_null() {
            return false;
        }

        #[cfg(feature = "link-ghostty")]
        unsafe {
            ghostty_surface_binding_action(
                surface,
                action.as_ptr() as *const std::os::raw::c_char,
                action.len(),
            )
        }
        #[cfg(not(feature = "link-ghostty"))]
        {
            let _ = (surface, action);
            false
        }
    }

    /// Send a synthetic key event to the terminal surface.
    ///
    /// `keyval` is a GDK keyval (e.g. from `gdk4::Key::from_name`).
    /// `mods` is a ghostty modifier bitmask.
    pub fn send_key(&self, keyval: u32, keycode: u32, mods: u32) -> bool {
        let surface = self.imp().surface.get();
        if surface.is_null() {
            return false;
        }

        // Build the text pointer for printable keys
        let key: gdk4::Key = unsafe { glib::translate::from_glib(keyval) };
        let mut text_buf = [0u8; 8];
        let text_ptr =
            if mods == 0 || mods == ghostty_sys::ghostty_input_mods_e::GHOSTTY_MODS_SHIFT as u32 {
                if let Some(ch) = key.to_unicode() {
                    if ch >= '\x20' {
                        let len = ch.encode_utf8(&mut text_buf).len();
                        text_buf[len] = 0;
                        text_buf.as_ptr() as *const c_char
                    } else {
                        ptr::null()
                    }
                } else {
                    ptr::null()
                }
            } else {
                ptr::null()
            };

        let unshifted_codepoint = key.to_lower().to_unicode().map(|c| c as u32).unwrap_or(0);

        let key_event = ghostty_input_key_s {
            action: ghostty_input_action_e::GHOSTTY_ACTION_PRESS,
            mods,
            consumed_mods: 0,
            keycode,
            text: text_ptr,
            unshifted_codepoint,
            composing: false,
        };

        #[cfg(feature = "link-ghostty")]
        unsafe {
            ghostty_surface_key(surface, key_event)
        }

        #[cfg(not(feature = "link-ghostty"))]
        {
            let _ = key_event;
            false
        }
    }

    /// Read the visible screen text from the terminal.
    ///
    /// Returns the full viewport text, or `None` if the surface is not ready.
    pub fn read_screen_text(&self) -> Option<String> {
        let surface = self.imp().surface.get();
        if surface.is_null() {
            return None;
        }

        #[cfg(feature = "link-ghostty")]
        unsafe {
            use ghostty_sys::*;

            let size = ghostty_surface_size(surface);
            if size.columns == 0 || size.rows == 0 {
                return Some(String::new());
            }

            let selection = ghostty_selection_s {
                top_left: ghostty_point_s {
                    tag: ghostty_point_tag_e::GHOSTTY_POINT_VIEWPORT,
                    coord: ghostty_point_coord_e::GHOSTTY_POINT_COORD_TOP_LEFT,
                    x: 0,
                    y: 0,
                },
                bottom_right: ghostty_point_s {
                    tag: ghostty_point_tag_e::GHOSTTY_POINT_VIEWPORT,
                    coord: ghostty_point_coord_e::GHOSTTY_POINT_COORD_BOTTOM_RIGHT,
                    x: size.columns as u32 - 1,
                    y: size.rows as u32 - 1,
                },
                rectangle: false,
            };

            let mut text_out = std::mem::zeroed::<ghostty_text_s>();
            if !ghostty_surface_read_text(surface, selection, &mut text_out) {
                return Some(String::new());
            }

            let result = if text_out.text.is_null() || text_out.text_len == 0 {
                String::new()
            } else {
                let slice =
                    std::slice::from_raw_parts(text_out.text as *const u8, text_out.text_len);
                String::from_utf8_lossy(slice).into_owned()
            };

            ghostty_surface_free_text(surface, &mut text_out);
            Some(result)
        }

        #[cfg(not(feature = "link-ghostty"))]
        {
            Some(String::new())
        }
    }

    /// Read the full screen buffer including scrollback history.
    ///
    /// Uses `GHOSTTY_POINT_SCREEN` which covers the entire scrollback
    /// buffer plus the visible viewport. Returns `None` if the surface
    /// is not ready.
    pub fn read_scrollback_text(&self) -> Option<String> {
        let surface = self.imp().surface.get();
        if surface.is_null() {
            return None;
        }

        #[cfg(feature = "link-ghostty")]
        unsafe {
            use ghostty_sys::*;

            let selection = ghostty_selection_s {
                top_left: ghostty_point_s {
                    tag: ghostty_point_tag_e::GHOSTTY_POINT_SCREEN,
                    coord: ghostty_point_coord_e::GHOSTTY_POINT_COORD_TOP_LEFT,
                    x: 0,
                    y: 0,
                },
                bottom_right: ghostty_point_s {
                    tag: ghostty_point_tag_e::GHOSTTY_POINT_SCREEN,
                    coord: ghostty_point_coord_e::GHOSTTY_POINT_COORD_BOTTOM_RIGHT,
                    x: 0,
                    y: 0,
                },
                rectangle: false,
            };

            let mut text_out = std::mem::zeroed::<ghostty_text_s>();
            if !ghostty_surface_read_text(surface, selection, &mut text_out) {
                return Some(String::new());
            }

            let result = if text_out.text.is_null() || text_out.text_len == 0 {
                String::new()
            } else {
                let slice =
                    std::slice::from_raw_parts(text_out.text as *const u8, text_out.text_len);
                String::from_utf8_lossy(slice).into_owned()
            };

            ghostty_surface_free_text(surface, &mut text_out);
            Some(result)
        }

        #[cfg(not(feature = "link-ghostty"))]
        {
            Some(String::new())
        }
    }

    /// Send text input to the terminal (e.g., from IME commit).
    pub fn send_text(&self, text: &str) -> bool {
        let surface = self.imp().surface.get();
        if surface.is_null() {
            self.imp().pending_text.borrow_mut().push(text.to_string());
            return true;
        }

        self.write_text(surface, text)
    }

    pub fn read_clipboard_request(&self, clipboard: ghostty_clipboard_e, context: *mut c_void) {
        let clipboard = self.clipboard_for_kind(clipboard);
        let surface = self.clone();
        let context = SendPtr(context);
        clipboard.read_text_async(None::<&gtk4::gio::Cancellable>, move |result| {
            let text = match result {
                Ok(Some(text)) => text.to_string(),
                Ok(None) => String::new(),
                Err(err) => {
                    tracing::warn!("Failed to read clipboard text: {}", err);
                    String::new()
                }
            };

            if !text.is_empty() {
                surface.complete_clipboard_request(&text, context.0, false);
                return;
            }

            // No text — try reading image from clipboard and paste as temp file path
            let cb = surface.clipboard();
            let surface2 = surface.clone();
            cb.read_texture_async(None::<&gtk4::gio::Cancellable>, move |result| {
                let path = match result {
                    Ok(Some(texture)) => save_clipboard_image(&texture),
                    _ => None,
                };
                let text = path.unwrap_or_default();
                surface2.complete_clipboard_request(&text, context.0, false);
            });
        });
    }

    pub fn confirm_clipboard_read(
        &self,
        content: &str,
        context: *mut c_void,
        request: ghostty_clipboard_request_e,
    ) {
        tracing::warn!(
            ?request,
            "Auto-confirming Ghostty clipboard request in embedded host"
        );
        self.complete_clipboard_request(content, context, true);
    }

    pub fn write_clipboard(
        &self,
        clipboard: ghostty_clipboard_e,
        content: &[ClipboardContent],
        _confirm: bool,
    ) {
        let clipboard = self.clipboard_for_kind(clipboard);
        if let Some(text) = content
            .iter()
            .find_map(
                |entry| match (entry.mime.as_deref(), entry.data.as_deref()) {
                    (Some("text/plain"), Some(text)) => Some(text),
                    _ => None,
                },
            )
            .or_else(|| content.iter().find_map(|entry| entry.data.as_deref()))
        {
            clipboard.set_text(text);
        }
    }

    pub fn set_close_handler<F>(&self, handler: F)
    where
        F: Fn(bool) + 'static,
    {
        *self.imp().close_handler.borrow_mut() = Some(Rc::new(handler));
    }

    pub fn close_requested(&self, process_alive: bool) {
        tracing::debug!(process_alive, "ghostty requested surface close");
        let handler = self.imp().close_handler.borrow().clone();
        if let Some(handler) = handler {
            handler(process_alive);
        }
    }

    fn setup_ime(&self) {
        let Some(im_context) = self.imp().im_context.borrow().as_ref().cloned() else {
            return;
        };

        im_context.set_client_widget(Some(self));
        im_context.set_use_preedit(true);

        // Weak refs: `im_context` is stored on the widget's imp, so strong `self`
        // clones in these handlers would be permanent reference cycles that keep
        // the surface (and its scrollback grid) alive forever.
        im_context.connect_preedit_start(glib::clone!(
            #[weak(rename_to = surface_widget)]
            self,
            move |_context| {
                surface_widget.im_preedit_start();
            }
        ));

        im_context.connect_commit(glib::clone!(
            #[weak(rename_to = surface_widget)]
            self,
            move |_context, text| {
                surface_widget.im_commit(text);
            }
        ));

        im_context.connect_preedit_changed(glib::clone!(
            #[weak(rename_to = surface_widget)]
            self,
            move |context| {
                surface_widget.im_preedit_changed(context);
            }
        ));

        im_context.connect_preedit_end(glib::clone!(
            #[weak(rename_to = surface_widget)]
            self,
            move |_context| {
                surface_widget.im_preedit_end();
            }
        ));
    }

    fn im_preedit_start(&self) {
        self.imp().im_composing.set(true);
        self.imp().im_commit_text.borrow_mut().clear();
    }

    fn im_preedit_changed(&self, context: &gtk4::IMMulticontext) {
        self.imp().im_composing.set(true);
        let (text, _attrs, _cursor_pos) = context.preedit_string();
        self.update_preedit(text.as_str());
        self.update_ime_cursor_location();
    }

    fn im_preedit_end(&self) {
        self.imp().im_composing.set(false);
        self.update_preedit("");
    }

    fn im_commit(&self, text: &str) {
        match self.imp().in_keyevent.get() {
            ImeKeyEventState::NotComposing => {
                let mut committed = self.imp().im_commit_text.borrow_mut();
                committed.clear();
                committed.extend_from_slice(text.as_bytes());
            }
            ImeKeyEventState::Composing | ImeKeyEventState::Idle => {
                self.imp().im_composing.set(false);
                self.update_preedit("");
                self.send_text_as_key(text);
            }
        }
    }

    fn send_text_as_key(&self, text: &str) {
        let surface = self.imp().surface.get();
        if surface.is_null() {
            return;
        }

        #[cfg(not(feature = "link-ghostty"))]
        let _ = text;

        let Some(cstr) = cstring_input(text, "IME commit") else {
            return;
        };

        #[cfg(feature = "link-ghostty")]
        unsafe {
            let event = ghostty_input_key_s {
                action: ghostty_input_action_e::GHOSTTY_ACTION_PRESS,
                mods: 0,
                consumed_mods: 0,
                keycode: 0,
                text: cstr.as_ptr(),
                unshifted_codepoint: 0,
                composing: false,
            };
            let _ = ghostty_surface_key(surface, event);
        }

        #[cfg(not(feature = "link-ghostty"))]
        let _ = cstr;
    }

    fn update_preedit(&self, text: &str) {
        let surface = self.imp().surface.get();
        if surface.is_null() {
            return;
        }

        #[cfg(feature = "link-ghostty")]
        {
            let Some(cstr) = cstring_input(text, "IME preedit") else {
                return;
            };

            unsafe {
                ghostty_surface_preedit(surface, cstr.as_ptr(), text.len());
            }
        }
        let _ = text;
    }

    #[allow(clippy::needless_return)] // guard clause before cfg-gated body
    fn update_ime_cursor_location(&self) {
        let surface = self.imp().surface.get();
        if surface.is_null() {
            return;
        }

        #[cfg(feature = "link-ghostty")]
        unsafe {
            let Some(im_context) = self.imp().im_context.borrow().as_ref().cloned() else {
                return;
            };

            let mut x = 0.0;
            let mut y = 0.0;
            let mut w = 0.0;
            let mut h = 0.0;
            ghostty_surface_ime_point(surface, &mut x, &mut y, &mut w, &mut h);
            let rect = gdk4::Rectangle::new(
                x.round() as i32,
                y.round() as i32,
                w.max(1.0).round() as i32,
                h.max(1.0).round() as i32,
            );
            im_context.set_cursor_location(&rect);
        }
    }

    /// Set the current title (called from action callback).
    pub fn set_title(&self, title: &str) {
        *self.imp().title.borrow_mut() = title.to_string();
    }

    /// Get the current title.
    pub fn title(&self) -> String {
        self.imp().title.borrow().clone()
    }

    /// Request the surface to close.
    pub fn request_close(&self) {
        let surface = self.imp().surface.get();
        if !surface.is_null() {
            #[cfg(feature = "link-ghostty")]
            unsafe {
                ghostty_surface_request_close(surface);
            }
        }
    }

    /// Check if the process has exited.
    pub fn process_exited(&self) -> bool {
        let surface = self.imp().surface.get();
        if surface.is_null() {
            return true;
        }
        #[cfg(feature = "link-ghostty")]
        {
            unsafe { ghostty_surface_process_exited(surface) }
        }
        #[cfg(not(feature = "link-ghostty"))]
        false
    }

    /// Get the surface size info.
    pub fn surface_size(&self) -> Option<ghostty_surface_size_s> {
        let surface = self.imp().surface.get();
        if surface.is_null() {
            return None;
        }
        #[cfg(feature = "link-ghostty")]
        {
            Some(unsafe { ghostty_surface_size(surface) })
        }
        #[cfg(not(feature = "link-ghostty"))]
        None
    }

    /// Build a right-click context menu with Copy and Paste actions.
    fn build_context_menu(&self) -> gtk4::PopoverMenu {
        let menu = gtk4::gio::Menu::new();
        menu.append(Some("Copy"), Some("surface.copy"));
        menu.append(Some("Paste"), Some("surface.paste"));

        let action_group = gtk4::gio::SimpleActionGroup::new();

        // Copy: read the primary selection (highlighted text) into the system clipboard
        let copy_action = gtk4::gio::SimpleAction::new("copy", None);
        // Weak ref: the action group is inserted into the widget (below), so a
        // strong `self` clone here would be a permanent reference cycle.
        copy_action.connect_activate(glib::clone!(
            #[weak(rename_to = widget_for_copy)]
            self,
            move |_, _| {
                let primary = widget_for_copy.primary_clipboard();
                let system = widget_for_copy.clipboard();
                primary.read_text_async(
                    None::<&gtk4::gio::Cancellable>,
                    glib::clone!(
                        #[weak]
                        system,
                        move |result| {
                            if let Ok(Some(text)) = result {
                                if !text.is_empty() {
                                    system.set_text(&text);
                                }
                            }
                        }
                    ),
                );
            }
        ));
        action_group.add_action(&copy_action);

        // Paste: read system clipboard and send to the terminal
        let paste_action = gtk4::gio::SimpleAction::new("paste", None);
        paste_action.connect_activate(glib::clone!(
            #[weak(rename_to = widget_for_paste)]
            self,
            move |_, _| {
                let clipboard = widget_for_paste.clipboard();
                let surface = widget_for_paste.clone();
                clipboard.read_text_async(None::<&gtk4::gio::Cancellable>, move |result| {
                    if let Ok(Some(text)) = result {
                        if !text.is_empty() {
                            surface.send_text(&text);
                        }
                    }
                });
            }
        ));
        action_group.add_action(&paste_action);

        self.insert_action_group("surface", Some(&action_group));

        let popover = gtk4::PopoverMenu::from_model(Some(&menu));
        popover.set_has_arrow(false);
        popover
    }

    fn clipboard_for_kind(&self, clipboard: ghostty_clipboard_e) -> gdk4::Clipboard {
        match clipboard {
            ghostty_clipboard_e::GHOSTTY_CLIPBOARD_SELECTION => self.primary_clipboard(),
            _ => self.clipboard(),
        }
    }

    fn complete_clipboard_request(&self, text: &str, context: *mut c_void, confirmed: bool) {
        let surface = self.imp().surface.get();
        if surface.is_null() {
            return;
        }

        #[cfg(feature = "link-ghostty")]
        {
            let Some(cstr) = cstring_input(text, "clipboard request") else {
                return;
            };

            unsafe {
                ghostty_surface_complete_clipboard_request(
                    surface,
                    cstr.as_ptr(),
                    context,
                    confirmed,
                );
            }
        }
        #[cfg(not(feature = "link-ghostty"))]
        let _ = (text, context, confirmed);
    }
}

/// Save a clipboard image texture to a temp PNG file.
/// Returns the shell-safe file path on success.
fn save_clipboard_image(texture: &gdk4::Texture) -> Option<String> {
    let dir = std::env::temp_dir().join("jmux-clipboard");
    std::fs::create_dir_all(&dir).ok()?;
    let filename = format!(
        "clipboard-{}.png",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );
    let path = dir.join(&filename);
    let path_str = path.to_str()?;
    texture.save_to_png(path_str).ok()?;
    // Return shell-escaped path
    Some(shell_escape(path_str))
}

fn shell_escape(s: &str) -> String {
    if s.contains(|c: char| c.is_whitespace() || "\"'\\$`!#&|;(){}[]<>?*~".contains(c)) {
        format!("'{}'", s.replace('\'', "'\\''"))
    } else {
        s.to_string()
    }
}

#[derive(Clone, Copy)]
struct SendPtr(*mut c_void);

// SAFETY: SendPtr wraps an opaque ghostty pointer that is sent via channel
// to the GTK main thread. The pointer is only dereferenced on the main thread.
unsafe impl Send for SendPtr {}

/// Parse a CSS hex color (#RGB or #RRGGBB) into (r, g, b) floats in [0, 1].
fn parse_hex_color(hex: &str) -> Option<(f32, f32, f32)> {
    let hex = hex.strip_prefix('#')?;
    match hex.len() {
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some((r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0))
        }
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
            Some((r as f32 / 15.0, g as f32 / 15.0, b as f32 / 15.0))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::cstring_input;
    use super::SpawnState;

    #[test]
    fn cstring_input_accepts_valid_text() {
        assert!(cstring_input("hello", "test").is_some());
    }

    #[test]
    fn cstring_input_rejects_interior_nul() {
        assert!(cstring_input("hel\0lo", "test").is_none());
    }

    // ── spawn-once guard ────────────────────────────────────────────────
    // The two spawn paths (first visible allocation / `spawn_headless`) share
    // this guard; double-spawning would fork a second pty + agent process.

    #[test]
    fn a_fresh_surface_may_spawn() {
        assert!(SpawnState::default().may_start());
    }

    #[test]
    fn an_in_flight_spawn_blocks_a_second_one() {
        assert!(!SpawnState::Spawning.may_start());
    }

    #[test]
    fn a_spawned_surface_never_spawns_again() {
        // The headless path spawned the agent; the widget is mapped later and
        // its first allocation must resize only.
        let after_headless = SpawnState::settled(true);
        assert_eq!(after_headless, SpawnState::Spawned);
        assert!(!after_headless.may_start());
    }

    #[test]
    fn a_failed_spawn_is_retryable() {
        // No GL context (no window yet): the allocation path must still be able
        // to start the child when the workspace is eventually shown.
        let after_failure = SpawnState::settled(false);
        assert_eq!(after_failure, SpawnState::Idle);
        assert!(after_failure.may_start());
    }
}

impl Default for GhosttyGlSurface {
    fn default() -> Self {
        Self::new()
    }
}
