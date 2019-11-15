# Copyright (C) 2019  Braiins Systems s.r.o.
#
# This file is part of Braiins Open-Source Initiative (BOSI).
#
# BOSI is free software: you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with this program.  If not, see <https://www.gnu.org/licenses/>.
#
# Please, keep in mind that we may also license BOSI or any part thereof
# under a proprietary license. For more information on the terms and conditions
# of such proprietary license or if you have any other questions, please
# contact us at opensource@braiins.com.

FROM debian:buster AS base-system

# required packages:
#   build-essential - rust needs gcc linker
#   pkg-config      - for rust packages linking C libraries
#   libusb-1.0      - required by erupter

RUN apt-get update && apt-get install -y \
        build-essential \
        pkg-config \
        libusb-1.0-0-dev \
        python3-toml \
        curl \
        openssh-client \
    && rm -rf /var/lib/apt/* /var/cache/apt/*

FROM base-system AS rust-system

# put projecter script into $PATH
COPY open/scripts/projecter /usr/local/bin/

# --- C crosscompiler part --- --- ---
# pull in openwrt sdk for s9
RUN curl --output - 'https://downloads.openwrt.org/releases/17.01.6/targets/zynq/generic/lede-sdk-17.01.6-zynq_gcc-5.4.0_musl-1.1.16_eabi.Linux-x86_64.tar.xz' | tar Jxvf - && \
    ln -s lede-sdk-17.01.6-zynq_gcc-5.4.0_musl-1.1.16_eabi.Linux-x86_64 sdk

# add crosscompile gcc to path
ENV PATH="$PATH:/sdk/staging_dir/toolchain-arm_cortex-a9+neon_gcc-5.4.0_musl-1.1.16_eabi/bin"
# needed by gcc
ENV STAGING_DIR="/sdk/staging_dir/toolchain-arm_cortex-a9+neon_gcc-5.4.0_musl-1.1.16_eabi"
# override rust default cross compile prefix for gcc toolchain
ENV CROSS_COMPILE="arm-openwrt-linux"

# --- rust toolchain part --- --- ---
# poor man's packaging to not let rustup mess around with PATH
# building under different uid will need to redefine CARGO_HOME somewhere to $HOME
ENV CARGO_HOME="/usr/local"
ENV RUSTUP_HOME="/usr/local/rustup"

RUN mkdir -p $RUSTUP_HOME
# pull in rust the 'official'  way. do not attempt to add self to $PATH, since cargo's idea of bin/ already is
RUN bash -c "curl --proto '=https' --tlsv1.2 -sSf 'https://sh.rustup.rs' | sh /dev/stdin -y --no-modify-path"

RUN rustup toolchain install beta-2019-11-06 && \
    rustup component add rustfmt && \
    rustup target add arm-unknown-linux-musleabi --toolchain beta-2019-11-06-x86_64-unknown-linux-gnu

# --- prefetching cargo packages --- --- ---
# bOS source tree. note that this step is just disposable interstage
FROM rust-system AS rust-with-sources

# assuming we are run from vcs root, grab whole project
COPY . src
# let all cargo projects pull cache their dependencies
RUN ( cd src/open/ && projecter cargo fetch )

# final image
FROM rust-system AS rust

# copy cached dependencies
COPY --from=rust-with-sources /usr/local/registry /usr/local/registry
