# Overview

# Build

## Prerequisities

- python 3
- rust toolchain installed via [rustup](https://rustup.rs/)
- arm target
- svd2rust
- rustfmt-preview

### Install Prerequisites

Install python modules:

Setup virtual env if don't want to install packages into ~/.local or to the
system.
```shell
virtualenv --python=/usr/bin/python3 .venv
```

```shell

python3 -m pip3 install -r scripts/requirements.txt
```

Install cargo utilities:

```shell
cargo install svd2rust
cargo install form
rustup component add rustfmt-preview
```

Install target toolchain for the project:

```shell
cd to/bosminer
rustup target add arm-unknown-linux-musleabi
```

## Build Process

- setup toolchain path - note this assumes you have our lede meta environment build tool (```bb.py```) in path. Also, the selected config (if not left to default configuration) has to for the **musl** libc toolchain as stated above.

```
cd to/braiins-os/
eval $(./bb.py toolchain 2>/dev/null)
```

- build:

```shell
cd to/bosminer
cargo build --target <TARGET> --features <BACKEND>
```

The resulting binary is in: ```target/<TARGET>/debug/bosminer```. Currently, all musl targets are being statically linked - see here for details: https://github.com/japaric/rust-cross

### Backend and Target Selection

The correct backend and target platform must be selected for building miner. The following backends are supported:

- _erupter_: Block Erupter
- _antminer_s9_: Antminer S9 (_arm-unknown-linux-musleabi_)

```shell
# build for Bitmain's Antminer S9
cargo build --target "arm-unknown-linux-musleabi" --features "antminer_s9"

# build for Block Erupter USB miner compatible with host target
cargo build --features "erupter"
```

#### Override Target Default Configuration

Cargo can also be configured through environment variables and it is possible to set target and then use only `cargo build` without additional parameters.

```shell
export CARGO_BUILD_TARGET=arm-unknown-linux-musleabi

cargo build --features "antminer_s9"
```

# Implementation Notes

## Register field bit mapping
We use the [packed_struct](https://github.com/hashmismatch/packed_struct.rs) crate. The use of bit fields in case of registers longer than 1 byte is counter intuitive. This issue provides details https://github.com/hashmismatch/packed_struct.rs/issues/35. The counter-intuitive part is when using LSB byte mapping of the register with *LSB0* bit mapping. The crate starts the bit index at the highest byte which is not intuitive.

- Example of a 4 byte register mapped as least significant byte first (LSB) with LSB0 bit mapping:

| Description | byte | byte | byte | byte |
|--- | --- | --- | --- | --- |
| byte index | 3 | 2 | 1 | 0 |
|packed_struct bit index | bits 7:0 | bits 15:8 | bits 23:16 | bits 31:24 |
|actual bit index | bits 31:24 | bits 23:16 | bits 15:8 | bits 7:0 |

- Example of a 4 byte register mapped as most significant byte first (MSB) with LSB0 bit mapping:

| Description | byte | byte | byte | byte |
|--- | --- | --- | --- | --- |
| byte index | 3 | 2 | 1 | 0 |
|packed_struct bit index | bits 31:24 | bits 23:16 | bits 15:8 | bits 7:0 |
|actual bit index | bits 31:24 | bits 23:16 | bits 15:8 | bits 7:0 |

The implementation uses the MSB + LSB0 variant for registers longer than 1 byte that require individual bit mappings. It ensures the resulting array of bytes after packing is interpreted correctly e.g. using [u32::from_be_bytes()](https://doc.rust-lang.org/stable/std/primitive.u32.html#method.from_be_bytes).



# Testing

Authentication method "none" (no password) DOES NOT WORK.

For authentication, you MUST use either "publickey" authentication or "password" (although beware, this is not confirmed to be working from all sources).

NOTE: for the time being, the key MUST NOT have a passphrase. Therefore, only
temporary development key should be used.

```shell
cargo test --target <TARGET> --features <BACKEND> -- --hostname <HOSTNAME>
```

This runs all tests on remote machine specified by argument *--hostname*. It is possible to omit this additional parameter
by providing a configuration file *Test.toml* stored in crate root directory:

```toml
[remote]
hostname = "<HOSTNAME>"
```

With this settings it is possible to call following command:

```shell
cargo test
```

## Integration tests

The sources for integration tests can be found in ```tests/``` subdirectory. The command ```cargo test``` also results in building all integration tests. Since each test is a separate crate, there are separate binaries for each test. The resulting test binaries can be found in ```target/arm-unknown-linux-musleabi/debug/```, too. The binary file starts with the prefix that corresponds with the integration test source name. E.g:

```tests/s9_test.rs``` -> ```s9_test-c86bb9af61985799``` The hash would be different for each build for the current state of the project sources.

## Running Miner

The miner can be run on host target or on remote one depending on backend and supported targets.

```shell
# run miner on host target (without runner)
cargo run --target <TARGET> --features <BACKEND> -- --pool <POOLV2PROXY> --user <POOLUSER> [--disable-asic-boost]

# run miner on remote target (using runner written in python)
cargo run --target <TARGET> --features <BACKEND> -- [--hostname <HOSTNAME>] -- --pool <POOLV2PROXY> --user <POOLUSER> [--disable-asic-boost]
```

The `--disable-asic-boost` option disables ASIC boost on S9 targets (ASIC boost is enabled by default on S9) - this is achieved by changing the number of midstates sent to chips from 4 to 1. This option does nothing on eruptor target.


## Logging

To enable more verbose logging/tracing, set `RUST_LOG` environment variable:

- enable all tracing: `RUST_LOG=trace ./s9_stratum_test ...`
- enable hardware tracing: `RUST_LOG=bosminer::hal::s9=trace ./s9_stratum_test ...`
- quiet mode, print just errors: `RUST_LOG=error ./s9...`
- enable hardware tracing and workhub tracing: `RUST_LOG=bosminer::hal::s9=trace,bosminer::work::hub=trace ./s9_stratum_test ...`

More details about `RUST_LOG` syntax can be found in [envlogger reference](https://docs.rs/slog-envlogger/2.1.0/slog_envlogger/).


# TODO
- logging infrastructure
- get rid of thread specific components - e.g. sleeps, time delay calculations
- implement custom error type(s) and get rid of misusing std::io::Error
- cpu simulation (for diff 1 testing)
- extend ip core to indicate the number of items in both RX FIFO's
