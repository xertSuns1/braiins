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

- python toml parser (https://github.com/uiri/toml)

This is used by deployment helper script, you can skip it if you plan to use different deployment method.


Install cargo utilities:

```shell
rustup component add rustfmt
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

- alternatively, you can download prebuilt toolchain from openwrt (https://downloads.openwrt.org/releases/17.01.6/targets/zynq/generic/lede-sdk-17.01.6-zynq_gcc-5.4.0_musl-1.1.16_eabi.Linux-x86_64.tar.xz) and set up environment:

```shell
export PATH="$PATH:/_sdk_path_/staging_dir/toolchain-arm_cortex-a9+neon_gcc-5.4.0_musl-1.1.16_eabi/bin"
export STAGING_DIR="/_sdk_path_/staging_dir/toolchain-arm_cortex-a9+neon_gcc-5.4.0_musl-1.1.16_eabi"
export CROSS_COMPILE="arm-openwrt-linux"
```
  this avoids lots of time and space to build complete BrainsOS, but you can only build bosminer binary.

- build:

```shell
# build of Antminer S9 (the target is automaticaly set to 'arm-unknown-linux-musleabi')
cd to/bosminer-am1-s9
cargo build

# build of Block Erupter
cd to/bosminer-erupter
cargo build
```

The resulting binary is in: ```target/<TARGET>/debug/bosminer```. Currently, all musl targets are being statically linked - see here for details: https://github.com/japaric/rust-cross

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

## Running Miner

The miner can be run on host target or on remote one depending on backend and supported targets.

```shell
cd bosminer-<TARGET>
# run miner on host target (without runner)
cargo run -- --pool <POOLV2PROXY> --user <POOLUSER> [--disable-asic-boost]

# run miner on remote target (using runner written in python)
cargo run -- [--hostname <HOSTNAME>] -- --pool <POOLV2PROXY> --user <POOLUSER> [--disable-asic-boost]
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
