# Overview

This is the Antminer S9 backend that uses [fully opensource FPGA bitstream](../../hw/zynq-io-am1-s9/README.md).

# Build

We assume you have setup the generic part of the build environment as specified in the [bOSminer documentation](../README.md)


## Rust Prerequisites

S9's use an Zynq based control board and require special target tool chain for cross compilation:

```
rustup target add arm-unknown-linux-musleabi
```

Building bOSminer for S9 requires a cross tool chain (linker available). There are two ways where to get it:

- have an existing Braiins OS build available
- use downloaded/prebuilt toolchain

### Cross toolchain from Braiins OS Build 

In order to complete a docker build of Braiins OS, please follow [bOS build procedure](../../../braiins-os/README.md), specifically the section "Building Latest Firmware Images in Docker". That way you can reuse the toolchain for the cross linking phase. Also, the selected configuration (if not left to default configuration) has to be set for the **musl** libc toolchain. See [here](https://github.com/japaric/rust-cross) for details.


Once the Braiins OS build is complete, you can setup the cross toolchain path:

```
(workon bos; cd ../../../braiins-os/; eval $(./bb.py toolchain 2>/dev/null))
```

### Using Downloaded/prebuilt Toolchain

Download prebuilt toolchain from OpenWrt (https://downloads.openwrt.org/releases/17.01.6/targets/zynq/generic/lede-sdk-17.01.6-zynq_gcc-5.4.0_musl-1.1.16_eabi.Linux-x86_64.tar.xz) and set up environment:

```shell
export PATH="$PATH:/_sdk_path_/staging_dir/toolchain-arm_cortex-a9+neon_gcc-5.4.0_musl-1.1.16_eabi/bin"
export STAGING_DIR="/_sdk_path_/staging_dir/toolchain-arm_cortex-a9+neon_gcc-5.4.0_musl-1.1.16_eabi"
export CROSS_COMPILE="arm-openwrt-linux"
```

This avoids lots of time and space to build complete Braiins OS, but you can only build bOSminer binary.


## Build

```shell
cargo build
```
The resulting binary is in: ```target/<TARGET>/debug/bosminer-am1-s9```.


# Running

## Using Cargo

Note, that the command below assumes that the miner hostname or IP have been specified via *Test.toml* as described in [toplevel documentation](../README.md).

```shell
cargo run --release -- -- --pool v2.stratum.slushpool.com:3336 --user YOURUSERNAME.WORKERNAME --frequency 600 --voltage 9.0
````

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
