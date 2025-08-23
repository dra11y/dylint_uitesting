use anyhow::Result;
use dylint_internal::{CommandExt, library_filename};
use log::debug;
use std::{
    env::set_var,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use crate::env;

pub static DRIVER: OnceLock<PathBuf> = OnceLock::new();

pub fn initialize(name: &str) -> Result<&Path> {
    debug!("initialize: initialize() called with name: '{}'", name);

    if let Some(path) = DRIVER.get() {
        debug!(
            "initialize: Driver already initialized, returning: {}",
            path.display()
        );
        return Ok(path.as_path());
    }

    debug!("initialize: First time initialization, building library and driver...");
    let _ = env_logger::try_init();

    // Try to order failures by informativeness: build lib, then find lib, then build/find driver.
    debug!("initialize: Building library '{}'...", name);
    dylint_internal::cargo::build(&format!("library `{name}`"))
        .build()
        .success()?;
    debug!("initialize: Library build completed successfully");

    // `DYLINT_LIBRARY_PATH` must be set before `dylint_libs` is called.
    let metadata = dylint_internal::cargo::current_metadata().unwrap();
    let dylint_library_path = metadata.target_directory.join("debug");
    debug!(
        "initialize: Setting DYLINT_LIBRARY_PATH to: {}",
        dylint_library_path
    );
    unsafe {
        set_var(env::DYLINT_LIBRARY_PATH, dylint_library_path);
    }

    debug!("initialize: Getting dylint_libs...");
    let dylint_libs = dylint_libs(name)?;
    debug!("initialize: dylint_libs result: {}", dylint_libs);

    debug!("initialize: Getting dylint driver...");
    let driver =
        dylint::driver_builder::get(&dylint::opts::Dylint::default(), env!("RUSTUP_TOOLCHAIN"))?;
    debug!("initialize: Got driver: {}", driver.display());

    unsafe {
        set_var(env::CLIPPY_DISABLE_DOCS_LINKS, "true");
        set_var(env::DYLINT_LIBS, dylint_libs);
    }
    debug!("initialize: Environment variables set");

    // Store driver path for future calls
    let _ = DRIVER.set(driver);
    debug!("initialize: Driver stored in static, initialization complete");
    Ok(DRIVER.get().unwrap().as_path())
}

#[doc(hidden)]
pub fn dylint_libs(name: &str) -> Result<String> {
    let metadata = dylint_internal::cargo::current_metadata().unwrap();
    let rustup_toolchain = env::var(env::RUSTUP_TOOLCHAIN)?;
    let filename = library_filename(name, &rustup_toolchain);
    let path = metadata.target_directory.join("debug").join(filename);
    let paths = vec![path];
    serde_json::to_string(&paths).map_err(Into::into)
}
