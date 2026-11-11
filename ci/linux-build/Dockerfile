# Refresh deliberately, then pin the published multi-arch digest in workflows.
# Ubuntu 22.04 keeps the Linux release glibc baseline at 2.35.
FROM ubuntu:22.04@sha256:076e7bf82319619f28dd0aa9c02ab34b055ad37100395b4f49a2ffba04fb5e11

LABEL org.opencontainers.image.source="https://github.com/arpagon/bzz" \
      org.opencontainers.image.description="bzz Linux build, test, relay and virtual-audio prerequisites (amd64/arm64)"

# apt is used ONLY when maintaining this image, never in release/CI jobs.
# Keep libdbus headers for the unmodified v0.12.1 upgrade fixture; current bzz
# vendors DBus. Audio stays dynamically linked to the system's libasound.so.2.
RUN apt-get -o Acquire::Retries=3 -o APT::Update::Error-Mode=any update \
    && DEBIAN_FRONTEND=noninteractive apt-get -o Acquire::Retries=3 install --yes --no-install-recommends \
      bash build-essential ca-certificates cmake curl git jq libasound2-dev \
      libasound2-plugins libdbus-1-dev libssl-dev lsof openssl perl pkg-config \
      postgresql-client pulseaudio pulseaudio-utils python3 sudo tmux \
      docker.io docker-compose-v2 unzip zip zstd \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 1001 --shell /bin/bash builder \
    && printf 'builder ALL=(ALL) NOPASSWD:ALL\n' > /etc/sudoers.d/builder \
    && chmod 0440 /etc/sudoers.d/builder \
    && dpkg-query -W > /usr/local/share/bzz-build-packages.txt

# GitHub Actions supplies the Rust toolchain. Local smoke tests may mount a
# fetched cargo/rustup cache; no private credentials are part of this image.
CMD ["bash"]
