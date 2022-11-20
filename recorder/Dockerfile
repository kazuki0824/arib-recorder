###############
# BUILD STAGE #
###############
FROM rust:latest AS build

COPY ./ /build
WORKDIR /build
RUN cargo build --release


################
# RUNTIME PREP #
################
FROM ubuntu:kinetic

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    apt update && \
    apt install  wget -y    
RUN wget https://github.com/tsduck/tsduck/releases/download/v3.32-2983/tsduck-dev_3.32-2983.ubuntu22_amd64.deb \
         https://github.com/tsduck/tsduck/releases/download/v3.32-2983/tsduck_3.32-2983.ubuntu22_amd64.deb && \
    apt install ./tsduck*.deb -y && \
    rm ./tsduck*.deb && \
    apt  --fix-missing install libatomic1
COPY --from=build /build/release /app

ENTRYPOINT ["/app/recorder"]

