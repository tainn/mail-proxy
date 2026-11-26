FROM docker.io/rust:1.100.0 AS build
WORKDIR /build
COPY Cargo.toml /build/Cargo.toml
COPY src /build/src
RUN cargo build --release

FROM quay.io/fedora/fedora-minimal:45
WORKDIR /app
COPY --from=build /build/target/release/mail-proxy /app/mail-proxy
ENTRYPOINT [ "/app/mail-proxy" ]
