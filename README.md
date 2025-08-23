# dylint_testing

[docs.rs documentation]

<!-- cargo-rdme start -->

This crate provides convenient access to the [`compiletest_rs`] package for testing [Dylint]
libraries.

**Note: If your test has dependencies, you must use `ui_test_example` or `ui_test_examples`.**
See the [`question_mark_in_expression`] example in this repository.

This crate provides the following three functions:

- [`ui_test`] - test a library on all source files in a directory
- [`ui_test_example`] - test a library on one example target
- [`ui_test_examples`] - test a library on all example targets

For most situations, you can add the following to your library's `lib.rs` file:

```rust
#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
```

And include one or more `.rs` and `.stderr` files in a `ui` directory alongside your library's
`src` directory. See the [examples] in this repository.

## Test builder

In addition to the above three functions, [`ui::Test`] is a test "builder." Currently, the main
advantage of using `Test` over the above functions is that `Test` allows flags to be passed to
`rustc`. For an example of its use, see [`non_thread_safe_call_in_test`] in this repository.

`Test` has three constructors, which correspond to the above three functions as follows:

- [`ui::Test::src_base`] <-> [`ui_test`]
- [`ui::Test::example`] <-> [`ui_test_example`]
- [`ui::Test::examples`] <-> [`ui_test_examples`]

In each case, the constructor's arguments are exactly those of the corresponding function.

A `Test` instance has the following methods:

- `dylint_toml` - set the `dylint.toml` file's contents (for testing [configurable libraries])
- `rustc_flags` - pass flags to the compiler when running the test
- `expected_exit_status` - set the expected driver exit status (default 101 for dylint_driver)
- `run` - run the test

## Blessing expected files

- Default run: `cargo test` verifies annotations and diffs against `.stderr/.stdout`. It never writes fixtures.
- Bless: `BLESS=1 cargo test` performs a two-pass run:
  1) Verify (no writes). 2) If verification succeeds, update `.stderr/.stdout`.

Exit status: when using the Dylint driver, we accept either exit code `101` (current behavior)
or `1` (future-compatible), so your tests remain stable if upstream changes. Diagnostics are
parsed from stderr; debug driver prefixes are filtered for stable diffs.

This keeps diffs in `target/ui` during normal runs and only touches fixtures when you explicitly bless.

[Dylint]: https://github.com/trailofbits/dylint/tree/master
[`ui_test`]: https://crates.io/crates/ui_test
[`non_thread_safe_call_in_test`]: https://github.com/trailofbits/dylint/tree/master/examples/general/non_thread_safe_call_in_test/src/lib.rs
[`question_mark_in_expression`]: https://github.com/trailofbits/dylint/tree/master/examples/restriction/question_mark_in_expression/Cargo.toml
[`ui::Test::example`]: https://docs.rs/dylint_testing/latest/dylint_testing/ui/struct.Test.html#method.example
[`ui::Test::examples`]: https://docs.rs/dylint_testing/latest/dylint_testing/ui/struct.Test.html#method.examples
[`ui::Test::src_base`]: https://docs.rs/dylint_testing/latest/dylint_testing/ui/struct.Test.html#method.src_base
[`ui::Test`]: https://docs.rs/dylint_testing/latest/dylint_testing/ui/struct.Test.html
[`ui_test_example`]: https://docs.rs/dylint_testing/latest/dylint_testing/fn.ui_test_example.html
[`ui_test_examples`]: https://docs.rs/dylint_testing/latest/dylint_testing/fn.ui_test_examples.html
[`ui_test`]: https://docs.rs/dylint_testing/latest/dylint_testing/fn.ui_test.html
[configurable libraries]: https://github.com/trailofbits/dylint/tree/master#configurable-libraries
[docs.rs documentation]: https://docs.rs/dylint_testing/latest/dylint_testing/
[examples]: https://github.com/trailofbits/dylint/tree/master/examples
[its repository]: https://github.com/Manishearth/compiletest-rs

<!-- cargo-rdme end -->
