use crate::{
    cargo_integration::linking_flags,
    env::{self, VarGuard, is_env_truthy},
    ui,
};
use anyhow::{Context, Result, anyhow};
use cargo_metadata::{Metadata, Package, Target};
use log::debug;
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

    // Ensure our temporary test files are not filtered out by ui_test's CLI filters.
    // ui_test will call `with_args(Args::test())` internally and append filter strings
    // derived from `cargo test` CLI; those often do not match our temp file paths.
    // Adding the `src_base` directory as a filter guarantees every file beneath it
    // matches `default_any_file_filter` (substring match when `filter_exact` is false).
    cfg.filter_files.push(src_base.display().to_string());

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

    // Align expected exit status with the selected program.
    // rustc normally exits 1 on error; dylint-driver defaults to 101 (configurable).
    let is_dylint_driver = driver
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.contains("dylint-driver"))
        .unwrap_or(false);
    let expected_exit: i32 = if is_dylint_driver {
        config.expected_exit_status
    } else {
        1
    };
    cfg.comment_defaults.base().exit_status =
        ui_test::spanned::Spanned::<i32>::dummy(expected_exit).into();
    debug!(
        "run_tests: BLESS environment variable = {}",
        std::env::var("BLESS").unwrap_or_else(|_| "unset".to_string())
    );
    debug!("run_tests: is_env_truthy(BLESS) = {}", bless);
    debug!("run_tests: src_base = {}", src_base.display());
    debug!("run_tests: driver = {}", driver.display());

    // Normalize noisy driver debug lines on stderr for stable diffs.
    // Example: "[2025-..Z DEBUG dylint_driver] [\"rustc\", ...]"
    cfg.stderr_filter(r"(?m)^\[[^\]]+\s+DEBUG\s+dylint_driver\].*\n", b"");

    if bless {
        debug!("run_tests: Running two-pass blessing approach");
        // Two-pass approach for blessing as documented:
        // 1. First verify semantics (build, diagnostics, custom flags) but DO NOT fail on .stderr mismatches
        // 2. Only then write/update .stderr/.stdout files

        // Pass 1: Verify without creating/updating expected files
        debug!("run_tests: Pass 1 - Verification (ignore_output_conflict)");
        cfg.output_conflict_handling = ui_test::ignore_output_conflict;
        cfg.bless_command = Some(format!("{}=1 cargo test", env::BLESS));

        let verify_result = ui_test::run_tests(cfg.clone());
        debug!("run_tests: Pass 1 result = {:?}", verify_result);

        match &verify_result {
            Ok(_) => debug!("run_tests: Pass 1 SUCCEEDED - continuing to blessing"),
            Err(e) => debug!("run_tests: Pass 1 FAILED - {}", e),
        }

        // Do not bless if verification failed. This prevents blessing with incorrect/missing annotations.
        verify_result.map_err(|err| anyhow!("verification failed: {err}"))?;

        // Pass 2: Bless files (only reached if verification passed)
        debug!("run_tests: Pass 2 - Blessing (bless_output_files)");
        cfg.output_conflict_handling = ui_test::bless_output_files;
        let bless_result = ui_test::run_tests(cfg);
        debug!("run_tests: Pass 2 result = {:?}", bless_result);
        bless_result.map_err(|err| anyhow!("blessing failed: {err}"))
    } else {
        debug!("run_tests: Running non-blessing mode (error_on_output_conflict)");
        // Non-blessing mode: verify annotations and error on conflicts
        cfg.bless_command = Some(format!("{}=1 cargo test", env::BLESS));
        cfg.output_conflict_handling = ui_test::error_on_output_conflict;
        let result = ui_test::run_tests(cfg);
        debug!("run_tests: Non-blessing result = {:?}", result);
        result.map_err(|err| anyhow!("run tests failed: {err}"))
    }
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
    use std::path::PathBuf;

    fn write_file(dir: &Path, name: &str, contents: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn no_annotations_does_not_bless_even_with_env() {
        debug!("🧪 Starting no_annotations_does_not_bless_even_with_env");

        let tmp = tempfile::tempdir().unwrap();
        debug!("🧪 Created temp dir: {}", tmp.path().display());

        let file = write_file(
            tmp.path(),
            "no_annot.rs",
            "//@edition: 2021\nfn main(){ let _ = undefined; }\n",
        );
        debug!("🧪 Created test file: {}", file.display());
        debug!(
            "🧪 File contents:\n{}",
            std::fs::read_to_string(&file).unwrap()
        );

        // Build minimal config
        let config = ui::Config::default();

        // Set BLESS=1 to simulate user blessing
        unsafe { std::env::set_var("BLESS", "1") };
        debug!("🧪 Set BLESS=1");

        // Test the run_tests function directly with plain rustc - no dylint driver needed!
        debug!("🧪 About to call run_tests with rustc...");
        let rustc_path = std::path::Path::new("rustc");
        let result = run_tests(rustc_path, tmp.path(), &config);
        debug!("🧪 run_tests returned: {:?}", result);

        let stderr_path = file.with_extension("stderr");
        let stderr_exists = stderr_path.exists();
        debug!(
            "🧪 .stderr file exists? {} (path: {})",
            stderr_exists,
            stderr_path.display()
        );

        if stderr_exists {
            debug!(
                "🧪 .stderr file contents:\n{}",
                std::fs::read_to_string(&stderr_path)
                    .unwrap_or_else(|_| "Could not read stderr file".to_string())
            );
        }

        // The test should fail because there are no annotations
        assert!(result.is_err(), "verify should fail without annotations");

        // Ensure .stderr was NOT created despite BLESS being set
        assert!(
            !file.with_extension("stderr").exists(),
            ".stderr must not be created when annotations are missing"
        );
        debug!("🧪 All assertions passed!");
    }
}
