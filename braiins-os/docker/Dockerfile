# docker build --build-arg LOC_UID=$(id -u) --build-arg LOC_GID=$(id -g) -t bos-builder docker/
# docker run --rm -v $SSH_AUTH_SOCK:/ssh-agent --env SSH_AUTH_SOCK=/ssh-agent -ti bos-builder

FROM debian:9.6 AS build-system

RUN apt-get update && apt-get install -y \
    build-essential \
    sudo \
    python3 \
    python3-pip \
    virtualenv \
    git \
    gawk \
    zlib1g-dev \
    libncurses5-dev \
    unzip \
    wget \
    python2.7 \
    libssl1.0-dev \
    texinfo \
    device-tree-compiler \
    python3-gdbm \
    curl

FROM build-system

ENV USER build

ARG LOC_UID
ARG LOC_GID
RUN addgroup --gid=$LOC_GID $USER && \
    adduser --system --uid=$LOC_UID --gid=$LOC_GID $USER

WORKDIR /home/$USER
USER $USER

COPY --chown=build:build ./requirements.txt .
RUN pip3 install -r requirements.txt

COPY --chown=build:build ./bashrc ./.bashrc
COPY --chown=build:build ./requirements.md5 .

ENV PATH="/home/$USER/.cargo/bin:${PATH}"
RUN bash -c "curl --proto '=https' --tlsv1.2 -sSf 'https://sh.rustup.rs' | sh /dev/stdin -y" && \
    rustup component add rustfmt

RUN cargo install svd2rust --version 0.16.1 && \
    cargo install form --version 0.7.0 && \
    rustup target add arm-unknown-linux-musleabi

RUN rustup toolchain install nightly-2019-05-01 && \
    rustup target add --toolchain nightly-2019-05-01 arm-unknown-linux-musleabi && \
    rustup toolchain install nightly-2019-05-10 && \
    rustup target add --toolchain nightly-2019-05-10 arm-unknown-linux-musleabi
