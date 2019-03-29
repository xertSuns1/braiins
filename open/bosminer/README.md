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

The remote target that is used for running the tests should be accessible
via ssh key authenticated channel.
NOTE: for the time being, the key MUST NOT have a passphrase. Therefore, only
temporary development key should be used.

```shell
cargo test -- --hostname <HOSTNAME>
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

# TODO
- logging infrastructure
- get rid of thread specific components - e.g. sleeps, time delay calculations
- implement custom error type(s) and get rid of misusing std::io::Error
- cpu simulation (for diff 1 testing)
- extend ip core to indicate the number of items in both RX FIFO's
