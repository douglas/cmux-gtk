mod app;
mod browser_history;
#[cfg(feature = "webkit")]
mod browser_profiles;
mod ghostty_config;
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

    tracing::info!("cmux starting");

    // Run the GTK application
    let exit_code = app::run();
    std::process::exit(exit_code);
}

fn prefer_desktop_opengl() {
    // Prefer desktop GL over GLES — ghostty's embedded renderer uses desktop
    // OpenGL via GLAD, which is incompatible with GLES and Vulkan backends.
    append_env_flag("GDK_DEBUG", "gl-prefer-gl");
    // Disable GLES and Vulkan backends entirely to prevent GDK from selecting
    // them on hardware where they're available but cause rendering issues.
    for flag in ["gles-api", "vulkan"] {
        append_env_flag("GDK_DISABLE", flag);
    }
}

fn append_env_flag(var: &str, flag: &str) {
    match std::env::var(var) {
        Ok(existing) if existing.split(',').any(|f| f.trim() == flag) => {}
        Ok(existing) if existing.trim().is_empty() => std::env::set_var(var, flag),
        Ok(existing) => std::env::set_var(var, format!("{existing},{flag}")),
        Err(_) => std::env::set_var(var, flag),
    }
}
