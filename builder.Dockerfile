FROM ubuntu:20.04

RUN \
    apt-get update && \
    DEBIAN_FRONTEND=noninteractive apt-get install -yy curl gcc pkg-config git make musl-tools libssl-dev && \
    rm -rf /var/lib/apt/lists/*
RUN curl -Lo rustup.sh https://sh.rustup.rs && \
    sh rustup.sh --default-toolchain nightly --no-modify-path -y && \
    rm rustup.sh

ENV PATH=$PATH:/bin:/root/.cargo/bin
WORKDIR /src

RUN rustup target add x86_64-unknown-linux-musl
ENV PKG_CONFIG_ALLOW_CROSS=1
