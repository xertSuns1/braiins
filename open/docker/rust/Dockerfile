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

FROM debian:9.6 AS base-system

# required packages:
#   build-essential - rust needs gcc linker
#   pkg-config      - for rust packages linking C libraries
#   libusb-1.0      - required by erupter

RUN apt-get update && apt-get install -y \
        build-essential \
        pkg-config \
        libusb-1.0-0-dev \
        curl \
    && rm -rf /var/lib/apt/* /var/cache/apt/*

FROM base-system AS rust-system

COPY run4all /usr/local/bin/

# poor man's packaging to not let rustup mess around with PATH
# building under different uid will need to redefine CARGO_HOME somewhere to $HOME
ENV CARGO_HOME="/usr/local"
ENV RUSTUP_HOME="/usr/local/rustup"

RUN mkdir -p $RUSTUP_HOME
RUN bash -c "curl --proto '=https' --tlsv1.2 -sSf 'https://sh.rustup.rs' | sh /dev/stdin -y"
RUN rustup toolchain install nightly-2019-10-17-x86_64-unknown-linux-gnu && \
    rustup component add rustfmt && \
    rustup component add rustfmt --toolchain nightly-2019-10-17-x86_64-unknown-linux-gnu && \
    cargo install svd2rust --version 0.16.1 && \
    cargo install form --version 0.7.0
# update cargo index
RUN cargo search --limit 0
