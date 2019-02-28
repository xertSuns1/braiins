# Overview

# Build

## Prerequisities

- rust toolchain installed via [rustup](https://rustup.rs/)
- arm target
- svd2rust
- rustfmt-preview

### Install Prerequisites

```shell
cargo install svd2rust
cargo install form
rustup component add rustfmt-preview
```
Install target toolchain for the project.

```shell
cd to/rurminer
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
cd to/rurminer
cargo build
```

The resulting binary is in: ```target/arm-unknown-linux-musleabi/debug/rminer```. Currently, all musl targets are being statically linked - see here for details: https://github.com/japaric/rust-cross

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
```shell
cargo test
```

This fails as cargo attempts to run the test case on the build host. However, it is possible to deploy the compiled test binary to the target. See below for dinghy and issues.

## Integration tests
The sources for integration tests can be found in ```tests/``` subdirectory.
The command ```cargo test``` also results in building all integration tests
(since each is a separate crate, there are separate binaries for each test). Due
 to the same issue as above, the test can be found in ```target/arm-unknown-linux-musleabi/debug/```, too. The binary file starts
with the prefix that corresponds with the integration test source name. E.g:

```tests/s9_test.rs``` -> ```s9_test-c86bb9af61985799``` The hash would be
different for each build for the current state of the project sources.

# Dinghy Integration (tool for deploying)

- this is currently incomplete as dinghy requires toolchain path and sysroot. The latter is a problem as we currently have no sysroot. The issue is being discussed here: https://github.com/snipsco/dinghy/issues/71

# TODO
- logging infrastructure
- get rid of thread specific components - e.g. sleeps, time delay calculations
- implement custom error type(s) and get rid of misusing std::io::Error
- cpu simulation (for diff 1 testing)
- extend ip core to indicate the number of items in both RX FIFO's
