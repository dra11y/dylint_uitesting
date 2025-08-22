use crate::{
    cargo_integration::linking_flags,
    env::{self, VarGuard, is_env_truthy},
    ui,
};
use anyhow::{Context, Result, anyhow};
use cargo_metadata::{Metadata, Package, Target};
use std::{ffi::OsString, fs::copy, path::Path, sync::Mutex};

static MUTEX: Mutex<()> = Mutex::new(());

pub(crate) fn run_tests(driver: &Path, src_base: &Path, config: &ui::Config) -> Result<()> {
    let _lock = MUTEX.lock().unwrap();

    // Temporarily set DYLINT_TOML if provided
    let _var = config
        .dylint_toml
        .as_ref()
        .map(|value| VarGuard::set(env::DYLINT_TOML, value));

    // Build ui_test config starting from rustc defaults
    let mut cfg = ui_test::Config::rustc(src_base);

    // Program: overwrite only the binary path to the dylint driver and extend args
    cfg.program.program = driver.to_path_buf();
    // Required flags for diagnostics
    for arg in ["-Dwarnings", "--emit=metadata"] {
        cfg.program.args.push(OsString::from(arg));
    }
    // User-provided rustc flags (and example linking flags already merged upstream)
    for arg in &config.rustc_flags {
        cfg.program.args.push(OsString::from(arg));
    }

    // Propagate relevant env vars to the driver
    for key in [
        env::DYLINT_LIBS,
        env::CLIPPY_DISABLE_DOCS_LINKS,
        env::DYLINT_TOML,
        // Forward debugging aids so compiler ICEs/errors are actionable under the harness
        env::RUST_BACKTRACE,
        env::RUST_LOG,
    ] {
        let val = std::env::var_os(key);
        cfg.program
            .envs
            .push((OsString::from(key), val.map(Into::into)));
    }

    let bless = is_env_truthy(env::BLESS);
    cfg.output_conflict_handling = if bless {
        ui_test::bless_output_files
    } else {
        cfg.bless_command = Some(format!("{}=1 cargo test", env::BLESS));
        ui_test::error_on_output_conflict
    };

    ui_test::run_tests(cfg).map_err(|err| anyhow!("run tests failed: {err}"))
}

pub fn run_example_test(
    driver: &Path,
    metadata: &Metadata,
    package: &Package,
    target: &Target,
    config: &ui::Config,
) -> Result<()> {
    let linking_flags = linking_flags(metadata, package, target)?;
    let file_name = target
        .src_path
        .file_name()
        .ok_or_else(|| anyhow!("Could not get file name"))?;

    let tempdir = tempfile::tempdir().with_context(|| "`tempdir` failed")?;
    let src_base = tempdir.path();
    let to = src_base.join(file_name);

    copy(&target.src_path, &to).with_context(|| {
        format!(
            "Could not copy `{}` to `{}`",
            target.src_path,
            to.to_string_lossy()
        )
    })?;
    for extension in ["fixed", "stderr", "stdout"] {
        copy_with_extension(&target.src_path, &to, extension)
            .map(|_| ())
            .unwrap_or_default();
    }

    let mut config = config.clone();
    config.rustc_flags.extend(linking_flags.iter().cloned());

    run_tests(driver, src_base, &config)
}

fn copy_with_extension<P: AsRef<Path>, Q: AsRef<Path>>(
    from: P,
    to: Q,
    extension: &str,
) -> Result<u64> {
    let from = from.as_ref().with_extension(extension);
    let to = to.as_ref().with_extension(extension);
    copy(from, to).map_err(Into::into)
}

#[cfg(test)]
mod gating_tests {
    use super::*;
    use std::{env::set_var, path::PathBuf};

    fn write_file(dir: &Path, name: &str, contents: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn no_annotations_does_not_bless_even_with_env() {
        let tmp = tempfile::tempdir().unwrap();
        let file = write_file(
            tmp.path(),
            "no_annot.rs",
            "//@edition: 2021\nfn main(){ let _ = undefined; }\n",
        );

        // Build minimal config
        let config = ui::Config::default();

        // Set BLESS=1 to simulate user blessing
        unsafe { set_var("BLESS", "1") }

        // Expect run_tests to fail due to missing //~ annotations
        let result = run_tests(Path::new("rustc"), tmp.path(), &config);
        assert!(result.is_err(), "verify should fail without annotations");

        // Ensure .stderr was NOT created despite BLESS being set
        assert!(
            !file.with_extension("stderr").exists(),
            ".stderr must not be created when annotations are missing"
        );
    }
}
