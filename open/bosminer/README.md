# Overview

This is the root folder of bOSminer software. The project is intended as a
replacement for the existing *cgminer* software. However, it has been designed
from scratch and written in a more modern Rust programming language.

bOSminer natively supports **Stratum V2** and can be used in combination with
*V2->V1* [mining proxy](../stratum-proxy/README.md).

Currently, the project is early preview stage. Its Antminer S9 provides the following features:

- AsicBoost
- clock speed is fixed @ 650 MHz which results in ~ 4.6 Th/s per hashboard
- only single hashboard is currently supported
- fans run @ 100% speed (can be lowered by `set_fans.sh` tool)


## Further Work

Below is a list of domains that are to be implemented in the MVP phase of the project:

- cgminer compatible API
- fan control
- clock and voltage configuration



## Directory Layout

The project is structured as a set of crates:

- [bosminer](bosminer/README.md) - generic part of the software, you should not need to build this crate separately unless you are a developer
- [bosminer-erupter](bosminer-erupter/README.md) - Block Erupter support is
  provided for development purpose - it serves as a test bed for bosminer code base
- [bosminer-am1-s9](bosminer-am1-s9/README.md) - Antminer S9 application

Below are generic guidelines how to setup your build environment, after that,
you can follow specific details of each backend.

# Build

We assume you have setup the generic part of the build environment as specified in the [generic documentation](../../README.md)
Follow the steps below and proceed to the subdirectory of the selected backend (e.g. [bosminer-am1-s9](bosminer-am1-s9/README.md))

## Prerequisities

- python prerequisites
- svd2rust
- rustfmt-preview

### Python

```
mkvirtualenv --python=/usr/bin/python3 bosminer
python3 -m pip3 install -r scripts/requirements.txt
```

### Cargo/Rust tools


```shell
cargo install svd2rust
cargo install form
rustup component add rustfmt
```


# Remote Targets

The actual mining devices are considered as *remote targets*. That that means is that you can direct cargo to run the mining application or its tests remotely on a device that is already running an image of Braiins OS.

In order to perform the steps below you have to descent to a specific target folder (am1-s9 is the only remote target for the time being)

```shell
cd bosminer-am1-s9
```

## Authentication Notes
Authentication method "none" (no password) DOES NOT WORK.

For authentication, you MUST use either "publickey" authentication or "password" (although beware, this is not confirmed to be working from all sources).

NOTE: for the time being, the key MUST NOT have a passphrase. Therefore, only
temporary development key should be used.

## Running the Test suite Remotely
```shell
cargo test --target <TARGET> --features <BACKEND> -- --hostname <HOSTNAME>
```

This runs all tests on remote machine specified by argument *--hostname*. It is possible to omit this additional parameter
by providing a configuration file *Test.toml* stored in crate root directory:

```toml
[remote]
hostname = "<HOSTNAME>"
```

It is possible to call following command With these settings

```shell
cargo test
```

## Running the bosminer

The miner can be run on host target or on remote one depending on backend and supported targets.

```shell
# run miner on host target (without runner)
cargo run -- --pool <POOLV2PROXY> --user <POOLUSER> [--disable-asic-boost]

# run miner on remote target (using runner written in python)
cargo run -- [--hostname <HOSTNAME>] -- --pool <POOLV2PROXY> --user <POOLUSER> [--disable-asic-boost]
```

The `--disable-asic-boost` option disables ASIC boost on S9 targets (ASIC boost is enabled by default on S9) - this is achieved by changing the number of midstates sent to chips from 4 to 1. This option does nothing on eruptor target.


## Logging

To enable more verbose logging/tracing, set `RUST_LOG` environment variable:

- enable all tracing: `RUST_LOG=trace cargo run ...`
- quiet mode, print just errors: `RUST_LOG=error cargo run ...`
