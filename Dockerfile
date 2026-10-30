FROM debian:bullseye-slim

WORKDIR /
ARG DEBIAN_FRONTEND=noninteractive
ARG CMAKE_VERSION=3.30.6
ARG CMAKE_SHA256=a7aa25cdd8545156fe0fec95ebbd53cb2b5173a8717e227f6e8a755185c168cf
ARG SCITER_COMMIT=f33df075d9eb2f8d252cb88f1b2c8096e56197ed
ARG SCITER_SHA256=a1682fbf55e004f1862d6ace31b5220121d20906bdbf308d0a9237b451e4db86
ARG RUSTUP_VERSION=1.28.2
ARG RUSTUP_SHA256=20a06e644b0d9bd2fbdbfd52d42540bdde820ea7df86e92e533c073da0cdd43c
ENV VCPKG_FORCE_SYSTEM_BINARIES=1
RUN apt update -y && \
    apt install --yes --no-install-recommends \
        g++ \
        gcc \
        git \
        curl \
        nasm \
        yasm \
        libgtk-3-dev \
        clang \
        libxcb-randr0-dev \
        libxdo-dev \
        libxfixes-dev \
        libxcb-shape0-dev \
        libxcb-xfixes0-dev \
        libasound2-dev \
        libpulse-dev \
        make \
        wget \
        libssl-dev \
        unzip \
        zip \
        sudo \
        libgstreamer1.0-dev \
        libgstreamer-plugins-base1.0-dev \
        ca-certificates \
        ninja-build && \
        rm -rf /var/lib/apt/lists/*

RUN curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 \
        -o cmake-${CMAKE_VERSION}.tar.gz \
        https://github.com/Kitware/CMake/releases/download/v${CMAKE_VERSION}/cmake-${CMAKE_VERSION}.tar.gz && \
    echo "${CMAKE_SHA256}  cmake-${CMAKE_VERSION}.tar.gz" | sha256sum -c - && \
    tar xzf cmake-${CMAKE_VERSION}.tar.gz && \
    cd cmake-${CMAKE_VERSION} && \
    ./configure  --prefix=/usr/local && \
    make && \
    make install && \
    cd / && rm -rf cmake-${CMAKE_VERSION} cmake-${CMAKE_VERSION}.tar.gz

# The dated vcpkg tag is immutable and keeps the native dependency set reproducible.
RUN git clone --branch 2023.04.15 --depth=1 https://github.com/microsoft/vcpkg && \
    /vcpkg/bootstrap-vcpkg.sh -disableMetrics && \
    /vcpkg/vcpkg --disable-metrics install libvpx libyuv opus aom

RUN groupadd -r user && \
    useradd -r -g user user --home /home/user && \
    mkdir -p /home/user/rustdesk && \
    chown -R user: /home/user

WORKDIR /home/user
RUN curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 \
        -o /home/user/libsciter-gtk.so \
        https://raw.githubusercontent.com/c-smile/sciter-sdk/${SCITER_COMMIT}/bin.lnx/x64/libsciter-gtk.so && \
    echo "${SCITER_SHA256}  /home/user/libsciter-gtk.so" | sha256sum -c -

USER user
RUN curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 \
        -o rustup-init \
        https://static.rust-lang.org/rustup/archive/${RUSTUP_VERSION}/x86_64-unknown-linux-gnu/rustup-init && \
    echo "${RUSTUP_SHA256}  rustup-init" | sha256sum -c - && \
    chmod +x rustup-init && \
    ./rustup-init -y && \
    rm -f rustup-init

ENV HOME=/home/user
COPY ./entrypoint.sh /
USER user
ENTRYPOINT ["/entrypoint.sh"]
