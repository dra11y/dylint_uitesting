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

use std::path::Path;

mod cargo_integration;
mod env;
mod runtime;
mod test_runner;
pub mod ui;

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
