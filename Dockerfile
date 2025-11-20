# Build stage
FROM rust:1.75-alpine AS builder

# Install build dependencies
RUN apk add --no-cache \
    musl-dev \
    pkgconfig \
    openssl-dev \
    ffmpeg-dev \
    clang \
    llvm \
    gcc \
    g++

# Create app directory
WORKDIR /app

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY server/Cargo.toml server/
COPY core/Cargo.toml core/

# Create dummy files to cache dependencies
RUN mkdir -p server/src core/src && \
    echo "fn main() {}" > server/src/main.rs && \
    echo "//dummy" > core/src/lib.rs

# Build dependencies
RUN cargo build --release --package ferrex-server

# Copy actual source code
COPY server/src server/src
COPY core/src core/src
COPY server/migrations server/migrations

# Rebuild with actual source
RUN touch server/src/main.rs && \
    cargo build --release --package ferrex-server

# Runtime stage
FROM alpine:3.19

# Install runtime dependencies
RUN apk add --no-cache \
    ca-certificates \
    ffmpeg \
    postgresql-client \
    tini

# Create non-root user
RUN addgroup -g 1000 ferrex && \
    adduser -u 1000 -G ferrex -D ferrex

# Create necessary directories
RUN mkdir -p /app/data /app/migrations && \
    chown -R ferrex:ferrex /app

WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/target/release/ferrex-server /app/ferrex-server

# Copy migrations
COPY --from=builder /app/server/migrations /app/migrations

# Switch to non-root user
USER ferrex

# Expose port
EXPOSE 3000

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD wget --no-verbose --tries=1 --spider http://localhost:3000/health || exit 1

# Use tini as init system
ENTRYPOINT ["/sbin/tini", "--"]

# Run the server
CMD ["/app/ferrex-server"]