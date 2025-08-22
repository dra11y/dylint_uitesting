//! This crate provides convenient access to the [`compiletest_rs`] package for testing [Dylint]
//! libraries.
//!
//! **Note: If your test has dependencies, you must use `ui_test_example` or `ui_test_examples`.**
//! See the [`question_mark_in_expression`] example in this repository.
//!
//! This crate provides the following three functions:
//!
//! - [`ui_test`] - test a library on all source files in a directory
//! - [`ui_test_example`] - test a library on one example target
//! - [`ui_test_examples`] - test a library on all example targets
//!
//! For most situations, you can add the following to your library's `lib.rs` file:
//!
//! ```rust,ignore
//! #[test]
//! fn ui() {
//!     dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
//! }
//! ```
//!
//! And include one or more `.rs` and `.stderr` files in a `ui` directory alongside your library's
//! `src` directory. See the [examples] in this repository.
//!
//! # Test builder
//!
//! In addition to the above three functions, [`ui::Test`] is a test "builder." Currently, the main
//! advantage of using `Test` over the above functions is that `Test` allows flags to be passed to
//! `rustc`. For an example of its use, see [`non_thread_safe_call_in_test`] in this repository.
//!
//! `Test` has three constructors, which correspond to the above three functions as follows:
//!
//! - [`ui::Test::src_base`] <-> [`ui_test`]
//! - [`ui::Test::example`] <-> [`ui_test_example`]
//! - [`ui::Test::examples`] <-> [`ui_test_examples`]
//!
//! In each case, the constructor's arguments are exactly those of the corresponding function.
//!
//! A `Test` instance has the following methods:
//!
//! - `dylint_toml` - set the `dylint.toml` file's contents (for testing [configurable libraries])
//! - `rustc_flags` - pass flags to the compiler when running the test
//! - `run` - run the test
//!
//! # Blessing expected files
//!
//! - Default run: `cargo test` uses annotations only. It will not create or overwrite `.stderr/.stdout` files.
//! - Bless: `BLESS=1 cargo test` will first verify that annotations pass, then write or update `.stderr/.stdout` for you.
//!
//! This keeps diffs in `target/ui` during normal runs and only touches fixtures when you explicitly bless.
//!
//! [Dylint]: https://github.com/trailofbits/dylint/tree/master
//! [`ui_test`]: https://crates.io/crates/ui_test
//! [`non_thread_safe_call_in_test`]: https://github.com/trailofbits/dylint/tree/master/examples/general/non_thread_safe_call_in_test/src/lib.rs
//! [`question_mark_in_expression`]: https://github.com/trailofbits/dylint/tree/master/examples/restriction/question_mark_in_expression/Cargo.toml
//! [`ui::Test::example`]: https://docs.rs/dylint_testing/latest/dylint_testing/ui/struct.Test.html#method.example
//! [`ui::Test::examples`]: https://docs.rs/dylint_testing/latest/dylint_testing/ui/struct.Test.html#method.examples
//! [`ui::Test::src_base`]: https://docs.rs/dylint_testing/latest/dylint_testing/ui/struct.Test.html#method.src_base
//! [`ui::Test`]: https://docs.rs/dylint_testing/latest/dylint_testing/ui/struct.Test.html
//! [`ui_test_example`]: https://docs.rs/dylint_testing/latest/dylint_testing/fn.ui_test_example.html
//! [`ui_test_examples`]: https://docs.rs/dylint_testing/latest/dylint_testing/fn.ui_test_examples.html
//! [`ui_test`]: https://docs.rs/dylint_testing/latest/dylint_testing/fn.ui_test.html
//! [configurable libraries]: https://github.com/trailofbits/dylint/tree/master#configurable-libraries
//! [docs.rs documentation]: https://docs.rs/dylint_testing/latest/dylint_testing/
//! [examples]: https://github.com/trailofbits/dylint/tree/master/examples
//! [its repository]: https://github.com/Manishearth/compiletest-rs

use anyhow::{Context, Result, anyhow, ensure};
use cargo_metadata::{Metadata, Package, Target, TargetKind};
use dylint_internal::{CommandExt, env, library_filename, rustup::is_rustc};
use regex::Regex;
use std::sync::OnceLock;
use std::{
    env::{consts, remove_var, set_var, var_os},
    ffi::{OsStr, OsString},
    fs::{copy, read_dir, remove_file},
    io::BufRead,
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex},
};
use ui_test::{self};

pub mod ui;

static DRIVER: OnceLock<PathBuf> = OnceLock::new();
static LINKING_FLAGS: OnceLock<Vec<String>> = OnceLock::new();

macro_rules! declare_env_var {
    ($var: ident) => {
        pub const $var: &str = stringify!($var);
    };
}

declare_env_var!(BLESS);
declare_env_var!(RUST_BACKTRACE);
declare_env_var!(RUST_LOG);

pub fn is_env_truthy(var: &str) -> bool {
    ["true", "1"].contains(
        &std::env::var_os(var)
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default()
            .as_str(),
    )
}

/// Test a library on all source files in a directory.
///
/// - `name` is the name of a Dylint library to be tested. (Often, this is the same as the package
///   name.)
/// - `src_base` is a directory containing:
///   - source files on which to test the library (`.rs` files), and
///   - the output those files should produce (`.stderr` files).
pub fn ui_test(name: &str, src_base: impl AsRef<Path>) {
    ui::Test::src_base(name, src_base).run();
}

/// Test a library on one example target.
///
/// - `name` is the name of a Dylint library to be tested.
/// - `example` is an example target on which to test the library.
pub fn ui_test_example(name: &str, example: &str) {
    ui::Test::example(name, example).run();
}

/// Test a library on all example targets.
///
/// - `name` is the name of a Dylint library to be tested.
pub fn ui_test_examples(name: &str) {
    ui::Test::examples(name).run();
}

fn initialize(name: &str) -> Result<&Path> {
    if let Some(path) = DRIVER.get() {
        return Ok(path.as_path());
    }

    let _ = env_logger::try_init();

    // Try to order failures by informativeness: build lib, then find lib, then build/find driver.
    dylint_internal::cargo::build(&format!("library `{name}`"))
        .build()
        .success()?;

    // `DYLINT_LIBRARY_PATH` must be set before `dylint_libs` is called.
    let metadata = dylint_internal::cargo::current_metadata().unwrap();
    let dylint_library_path = metadata.target_directory.join("debug");
    unsafe {
        set_var(env::DYLINT_LIBRARY_PATH, dylint_library_path);
    }

    let dylint_libs = dylint_libs(name)?;
    let driver =
        dylint::driver_builder::get(&dylint::opts::Dylint::default(), env!("RUSTUP_TOOLCHAIN"))?;

    unsafe {
        set_var(env::CLIPPY_DISABLE_DOCS_LINKS, "true");
        set_var(env::DYLINT_LIBS, dylint_libs);
    }

    // Store driver path for future calls
    let _ = DRIVER.set(driver);
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

fn example_target(package: &Package, example: &str) -> Result<Target> {
    package
        .targets
        .iter()
        .find(|target| target.kind == [TargetKind::Example] && target.name == example)
        .cloned()
        .ok_or_else(|| anyhow!("Could not find example `{}`", example))
}

#[allow(clippy::unnecessary_wraps)]
fn example_targets(package: &Package) -> Result<Vec<Target>> {
    Ok(package
        .targets
        .iter()
        .filter(|target| target.kind == [TargetKind::Example])
        .cloned()
        .collect())
}

fn run_example_test(
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

    run_tests(driver, src_base, &config);

    Ok(())
}

fn linking_flags(
    metadata: &Metadata,
    package: &Package,
    target: &Target,
) -> Result<&'static [String]> {
    if let Some(existing) = LINKING_FLAGS.get() {
        return Ok(existing.as_slice());
    }

    let rustc_flags = rustc_flags(metadata, package, target)?;

    let mut linking_flags = Vec::new();
    let mut iter = rustc_flags.into_iter();
    while let Some(flag) = iter.next() {
        if flag.starts_with("--edition=") {
            linking_flags.push(flag);
        } else if flag == "--extern" || flag == "-L" {
            let arg = next(&flag, &mut iter)?;
            linking_flags.extend([flag, arg.trim_matches('\'').to_owned()]);
        }
    }

    let _ = LINKING_FLAGS.set(linking_flags);
    Ok(LINKING_FLAGS.get().unwrap().as_slice())
}

// smoelius: We need to recover the `rustc` flags used to build a target. I can see four options:
//
// * Use `cargo build --build-plan`
//   - Pros: Easily parsable JSON output
//   - Cons: Unstable and likely to be removed: https://github.com/rust-lang/cargo/issues/7614
// * Parse the output of `cargo build --verbose`
//   - Pros: ?
//   - Cons: Not as easily parsable, requires synchronization (see below)
// * Use a custom executor like Siderophile does: https://github.com/trailofbits/siderophile/blob/26c067306f6c2f66d9530dacef6b17dbf59cdf8c/src/trawl_source/mod.rs#L399
//   - Pros: Ground truth
//   - Cons: Seems a bit of a heavy lift (Note: I think Siderophile's approach was inspired by
//     `cargo-geiger`.)
// * Set `RUSTC_WORKSPACE_WRAPPER` to something that logs `rustc` invocations
//   - Pros: Ground truth
//   - Cons: Requires a separate executable/script, portability could be an issue
//
// I am going with the second option for now, because it seems to be the least of all evils. This
// decision may need to be revisited.

static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*Running\s*`(.*)`$").unwrap());

fn rustc_flags(metadata: &Metadata, package: &Package, target: &Target) -> Result<Vec<String>> {
    // smoelius: The following comments are old and retained for posterity. The linking flags are
    // now initialized using a `OnceCell`, which makes the mutex unnecessary.
    //   smoelius: Force rebuilding of the example by removing it. This is kind of messy. The
    //   example is a shared resource that may be needed by multiple tests. For now, I lock a mutex
    //   while the example is removed and put back.
    //   smoelius: Should we use a temporary target directory here?
    let output = {
        remove_example(metadata, package, target)?;

        // smoelius: Because of lazy initialization, `cargo build` is run only once. Seeing
        // "Building example `target`" for one example but not for others is confusing. So instead
        // say "Building `package` examples".
        dylint_internal::cargo::build(&format!("`{}` examples", package.name))
            .build()
            .env_remove(env::CARGO_TERM_COLOR)
            .args([
                "--manifest-path",
                package.manifest_path.as_ref(),
                "--example",
                &target.name,
                "--verbose",
            ])
            .logged_output(true)?
    };

    let matches = output
        .stderr
        .lines()
        .map(|line| {
            let line =
                line.with_context(|| format!("Could not read from `{}`", package.manifest_path))?;
            Ok((*RE).captures(&line).and_then(|captures| {
                let args = captures[1]
                    .split(' ')
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>();
                if args.first().is_some_and(is_rustc)
                    && args
                        .as_slice()
                        .windows(2)
                        .any(|window| window == ["--crate-name", &snake_case(&target.name)])
                {
                    Some(args)
                } else {
                    None
                }
            }))
        })
        .collect::<Result<Vec<Option<Vec<_>>>>>()?;

    let mut matches = matches.into_iter().flatten().collect::<Vec<Vec<_>>>();
    ensure!(
        matches.len() <= 1,
        "Found multiple `rustc` invocations for `{}`",
        target.name
    );
    matches
        .pop()
        .ok_or_else(|| anyhow!("Found no `rustc` invocations for `{}`", target.name))
}

fn remove_example(metadata: &Metadata, _package: &Package, target: &Target) -> Result<()> {
    let examples = metadata.target_directory.join("debug/examples");
    for entry in
        read_dir(&examples).with_context(|| format!("`read_dir` failed for `{examples}`"))?
    {
        let entry = entry.with_context(|| format!("`read_dir` failed for `{examples}`"))?;
        let path = entry.path();

        if let Some(file_name) = path.file_name() {
            let s = file_name.to_string_lossy();
            let target_name = snake_case(&target.name);
            if s == target_name.clone() + consts::EXE_SUFFIX
                || s.starts_with(&(target_name.clone() + "-"))
            {
                remove_file(&path).with_context(|| {
                    format!("`remove_file` failed for `{}`", path.to_string_lossy())
                })?;
            }
        }
    }

    Ok(())
}

fn next<I, T>(flag: &str, iter: &mut I) -> Result<T>
where
    I: Iterator<Item = T>,
{
    iter.next()
        .ok_or_else(|| anyhow!("Missing argument for `{}`", flag))
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

static MUTEX: Mutex<()> = Mutex::new(());

fn run_tests(driver: &Path, src_base: &Path, config: &ui::Config) {
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
        RUST_BACKTRACE,
        RUST_LOG,
    ] {
        let val = std::env::var_os(key);
        cfg.program
            .envs
            .push((OsString::from(key), val.map(Into::into)));
    }

    let bless = is_env_truthy(BLESS);
    cfg.output_conflict_handling = if bless {
        ui_test::bless_output_files
    } else {
        cfg.bless_command = Some(format!("{BLESS}=1 cargo test"));
        ui_test::error_on_output_conflict
    };

    match ui_test::run_tests(cfg) {
        Ok(()) => {}
        Err(report) => {
            let msg = report.to_string();
            // if !msg.contains("tests failed") {
            panic!("TEST PANIC: {msg}");
            // }
        }
    }
}

// smoelius: `VarGuard` was copied from:
// https://github.com/rust-lang/rust-clippy/blob/9cc8da222b3893bc13bc13c8827e93f8ea246854/tests/compile-test.rs
// smoelius: Clippy dropped `VarGuard` when it switched to `ui_test`:
// https://github.com/rust-lang/rust-clippy/commit/77d10ac63dae6ef0a691d9acd63d65de9b9bf88e

/// Restores an env var on drop
#[must_use]
struct VarGuard {
    key: &'static str,
    value: Option<OsString>,
}

impl VarGuard {
    fn set(key: &'static str, val: impl AsRef<OsStr>) -> Self {
        let value = var_os(key);
        unsafe {
            set_var(key, val);
        }
        Self { key, value }
    }
}

impl Drop for VarGuard {
    fn drop(&mut self) {
        match self.value.as_deref() {
            None => unsafe { remove_var(self.key) },
            Some(value) => unsafe { set_var(self.key, value) },
        }
    }
}

fn snake_case(name: &str) -> String {
    name.replace('-', "_")
}

#[cfg(test)]
mod gating_tests {
    use super::*;
    use std::panic;

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
        let result = panic::catch_unwind(|| run_tests(Path::new("rustc"), tmp.path(), &config));
        assert!(result.is_err(), "verify should fail without annotations");

        // Ensure .stderr was NOT created despite BLESS being set
        assert!(
            !file.with_extension("stderr").exists(),
            ".stderr must not be created when annotations are missing"
        );
    }
}
