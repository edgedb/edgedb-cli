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


License
=======


Licensed under either of

* Apache License, Version 2.0,
  (./LICENSE-APACHE or http://www.apache.org/licenses/LICENSE-2.0)
* MIT license (./LICENSE-MIT or http://opensource.org/licenses/MIT)

at your option.
