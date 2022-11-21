###############
# BUILD STAGE #
###############
FROM rust:slim AS build

COPY ./ /build
WORKDIR /build
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo update
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    apt update && \
    apt install pkg-config libssl-dev -y
    
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release


################
# RUNTIME PREP #
################
FROM debian:bullseye-slim

## Runtime deps
RUN apt update && \
    apt install wget libatomic1 libssl1.1 -y

# TsDuck MUST be newer than https://github.com/tsduck/tsduck/commit/df3c927ddd3489cab1d86dbdc36d4f81370aff2f
RUN wget https://github.com/tsduck/tsduck/releases/download/v3.30-2710/tsduck-dev_3.30-2710.debian11_amd64.deb \
         https://github.com/tsduck/tsduck/releases/download/v3.30-2710/tsduck_3.30-2710.debian11_amd64.deb && \
    apt install ./tsduck*.deb -y && \
    rm ./tsduck*.deb
COPY --from=build /build/target/release /app

ENTRYPOINT ["/app/arib-recorder"]

