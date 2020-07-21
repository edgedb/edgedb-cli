EdgeDB Command-line Tools
=========================

This repository contains `edgedb` command-line tool rewritten in rust.

Use cargo for building it:
```
cargo build
cargo run -- --admin -d tutorial
cargo test
```

It's easiest to use Vagga to set up the build, execution, and testing
environments.  You can run the builds and tests directly as well,
provided that:

* you have the latest rustc (use rustup),
* you have an EdgeDB server installation (you can use
  `edgedb server install`),
* and set up the following environment variables **before** building
  the CLI or tests (use `cargo clean` to rebuild when changing env
  variables):

  * export EDGEDB_MAJOR_VERSION=1-alpha4
  * export PSQL_DEFAULT_PATH=/Library/Frameworks/EdgeDB.framework/Versions/1-alpha4/lib/edgedb-server-1-alpha4/bin
  * export PYTHONWARNINGS=


License
=======


Licensed under either of

* Apache License, Version 2.0,
  (./LICENSE-APACHE or http://www.apache.org/licenses/LICENSE-2.0)
* MIT license (./LICENSE-MIT or http://opensource.org/licenses/MIT)

at your option.
