mod agent_monitor;
mod ai;
mod app;
mod browser_history;
#[cfg(feature = "webkit")]
mod browser_import;
#[cfg(feature = "webkit")]
mod browser_profiles;
mod ghostty_config;
mod goal;
mod hibernate;
mod model;
mod notifications;
mod port_scanner;
mod remote;
mod session;
mod settings;
mod socket;
mod ui;

use tracing_subscriber::EnvFilter;

fn main() {
    prefer_desktop_opengl();

    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!("jmux starting");

    // Run the GTK application
    let exit_code = app::run();
    std::process::exit(exit_code);
}

fn prefer_desktop_opengl() {
    // Ghostty's embedded renderer uses desktop OpenGL via GLAD, which
    // requires GDK to use GL (not Vulkan) for compositing. When GDK
    // picks Vulkan, GtkGLArea can't get a compatible desktop GL context,
    // causing realize/unrealize instability.
    append_env_flag("GDK_DEBUG", "gl-prefer-gl");
    append_env_flag("GDK_DISABLE", "vulkan");
    // Note: we intentionally do NOT disable gles-api — WebKitGTK's GPU
    // compositor needs GLES/EGL, and gl-prefer-gl is sufficient to ensure
    // ghostty's GtkGLArea gets a desktop GL context.
}

fn append_env_flag(var: &str, flag: &str) {
    match std::env::var(var) {
        Ok(existing) if existing.split(',').any(|f| f.trim() == flag) => {}
        Ok(existing) if existing.trim().is_empty() => std::env::set_var(var, flag),
        Ok(existing) => std::env::set_var(var, format!("{existing},{flag}")),
        Err(_) => std::env::set_var(var, flag),
    }
}
