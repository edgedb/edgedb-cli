EdgeDB Command-line Tools
=========================

This repository contains `edgedb` command-line tool rewritten in rust.


Install
=======

Install the latest stable build with:

```
curl --proto '=https' --tlsv1.2 -sSf https://sh.edgedb.com | sh
```

Nightly builds can be installed with:

```
curl --proto '=https' --tlsv1.2 -sSf https://sh.edgedb.com | sh -s -- --nightly
```


Development
===========

Use cargo for building it:

```
cargo build
cargo run -- --admin -d tutorial
cargo test
```

Tests
=====

There are a few categories of tests in this repo:

- unit tests within `src/`
  - run with: `cargo test --bins`,
  - no additional requirements,

- `tests/func/`
  - invokes the cli binary,
  - run with: `cargo test --test=func`,
  - requires `edgedb-server` binary in PATH,
  - will use [test-utils](https://github.com/edgedb/test-utils/) to start the server,

- `tests/shared-client-tests/`
  - generates tests from [shared-client-testcases](https://github.com/edgedb/shared-client-testcases/),
  - invokes the cli binary,
  - run with: `cargo test --package=shared-client-tests`,
  - will write into `/home/edgedb`,

- `tests/portable_*.rs/`
  - tests installation of the portable EdgeDB server,
  - will download large packages,
  - run with: `cargo test --features=portable_tests --test=portable_X`,
  - assumes you don't have any portables installed before running it,

- `tests/docker_test_wrapper.rs`
  - runs other tests in a docker container,
  - run with: `cargo test --features=docker_test_wrapper --test=docker_test_wrapper`,
  - requires Docker,
  - requires that binaries compiled on host machine are runnable in "ubuntu:mantic",

- Github Actions & Nightly tests


Code Quality Assurance
======================

This project uses rustfmt and clippy to provide a unified code style.
When opening pull requests, it is advised to run the following commands
before doing so:

```bash
$ cargo clippy --all-features --workspace --all-targets -- \
-A clippy::collapsible_if \
-A clippy::derive_partial_eq_without_eq \
-A clippy::zero_ptr \
-A clippy::manual_strip \
-A clippy::new_ret_no_self \
-A clippy::type_complexity \
-A clippy::vec_init_then_push \
-A clippy::while_let_on_iterator \
-A clippy::useless_format \
-A clippy::too_many_arguments \
-A clippy::clone_on_copy

$ cargo fmt
```


License
=======


Licensed under either of

* Apache License, Version 2.0,
  (./LICENSE-APACHE or http://www.apache.org/licenses/LICENSE-2.0)
* MIT license (./LICENSE-MIT or http://opensource.org/licenses/MIT)

at your option.
