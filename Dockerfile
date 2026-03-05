# ══════════════════════════════════════════════════════════
# IRONCLAW — Bio DynamX Voice Agent Factory
# Multi-stage Docker build optimized for minimal image size
# ══════════════════════════════════════════════════════════

# ── Stage 1: Build ────────────────────────────────────────
FROM rust:1.93-alpine AS builder

# Install build dependencies
RUN apk add --no-cache musl-dev openssl-dev openssl-libs-static pkgconfig

WORKDIR /app

# Cache dependencies by copying manifests first
COPY Cargo.toml Cargo.lock* ./

# Create a dummy src to build deps
RUN mkdir src && \
    echo 'fn main() { println!("placeholder"); }' > src/main.rs && \
    cargo build --release && \
    rm -rf src

# Copy actual source and rebuild
COPY src/ src/

# Touch main.rs to force rebuild of our code (not deps)
RUN touch src/main.rs && \
    cargo build --release && \
    strip target/release/ironclaw

# ── Stage 2: Runtime ─────────────────────────────────────
FROM alpine:3.21 AS runtime

# Install minimal runtime dependencies
RUN apk add --no-cache ca-certificates tini

# Create non-root user
RUN addgroup -S ironclaw && adduser -S ironclaw -G ironclaw

WORKDIR /app

# Copy the binary
COPY --from=builder /app/target/release/ironclaw /app/ironclaw

# Create the profiles directory (will be mounted as a volume)
RUN mkdir -p /app/profiles && chown -R ironclaw:ironclaw /app

# Switch to non-root
USER ironclaw

# Expose the health/control port
EXPOSE 8080

# Health check
HEALTHCHECK --interval=15s --timeout=5s --start-period=10s --retries=3 \
    CMD wget --no-verbose --tries=1 --spider http://localhost:8080/healthz || exit 1

# Use tini as PID 1 for proper signal handling
ENTRYPOINT ["tini", "--"]

# Run Ironclaw with JSON logging for Cloud Logging
CMD ["/app/ironclaw", \
     "--profiles-dir", "/app/profiles", \
     "--host", "0.0.0.0", \
     "--port", "8080", \
     "--json-logs"]
