# Overview

This project is the Braiins Open-Source Initiative. It currently provides the
software components listed below.


## [`braiins-os`](braiins-os/README.md)

Meta project that is able to assemble full (mining) firmware images from various
components.


## [`open/protocols/stratum`](open/protocols/stratum/README.md)

Stratum protocol implementation including V1 and V2 in Rust programming
language. Additionally, there is a stratum protocol simulator written in Python
available [`sim`](open/protocols/stratum/sim/README.md) subdirectory.


## [`open/stratum-proxy`](open/stratum-proxy/README.md)

Stratum proxy written in Rust that allows translation between protocol versions.


## [`open/bosminer`](open/bosminer/README.md)

The bOSminer suite is a Bitcoin mining software written in Rust programming language.


# How to build individual components

Eventhough, each component has its own self-contained README. This section
provides common instructions to setup all environments.


## Rust applications/libraries

These software components require the toolchain installed e.g. via [rustup](https://rustup.rs/)


### Building with Debugging symbols

`cargo build`


### Building release version

`cargo build --release`


## Python applications

The easiest way to run any python application is to use  `virtualenvwrapper`.


### The `virtualenvwrapper`

```
apt install virtualenvwrapper
source /usr/share/virtualenvwrapper/virtualenvwrapper.sh
```


# Contributing

All contributions should follow [code of conduct](code-of-conduct.md).
