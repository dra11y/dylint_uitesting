use crate::env;
use anyhow::{Context, Result, anyhow, ensure};
use cargo_metadata::{Metadata, Package, Target, TargetKind};
use dylint_internal::{CommandExt, rustup::is_rustc};
use regex::Regex;
use std::{
    env::consts,
    fs::{read_dir, remove_file},
    io::BufRead,
    sync::{LazyLock, OnceLock},
};

static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*Running\s*`(.*)`$").unwrap());
static LINKING_FLAGS: OnceLock<Vec<String>> = OnceLock::new();

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

fn snake_case(name: &str) -> String {
    name.replace('-', "_")
}

pub fn example_target(package: &Package, example: &str) -> Result<Target> {
    package
        .targets
        .iter()
        .find(|target| target.kind == [TargetKind::Example] && target.name == example)
        .cloned()
        .ok_or_else(|| anyhow!("Could not find example `{}`", example))
}

#[allow(clippy::unnecessary_wraps)]
pub fn example_targets(package: &Package) -> Result<Vec<Target>> {
    Ok(package
        .targets
        .iter()
        .filter(|target| target.kind == [TargetKind::Example])
        .cloned()
        .collect())
}

pub fn rustc_flags(metadata: &Metadata, package: &Package, target: &Target) -> Result<Vec<String>> {
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

pub fn linking_flags(
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
            let arg = next_arg_for_flag(&flag, &mut iter)?;
            linking_flags.extend([flag, arg.trim_matches('\'').to_owned()]);
        }
    }

    let _ = LINKING_FLAGS.set(linking_flags);
    Ok(LINKING_FLAGS.get().unwrap().as_slice())
}

fn next_arg_for_flag<I, T>(flag: &str, iter: &mut I) -> Result<T>
where
    I: Iterator<Item = T>,
{
    iter.next()
        .ok_or_else(|| anyhow!("Missing argument for `{}`", flag))
}

pub fn remove_example(metadata: &Metadata, _package: &Package, target: &Target) -> Result<()> {
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
