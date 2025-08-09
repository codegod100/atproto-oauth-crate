# Use the official Rust image as base
FROM rust:1.82-slim-bookworm AS builder

# Install system dependencies needed for compilation
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /app

# Copy Cargo files first for better layer caching
COPY Cargo.toml Cargo.lock ./

# Create a dummy main to build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release --bins
RUN rm -rf src

# Copy the actual source code
COPY src ./src
COPY examples ./examples

# Build the project and examples
RUN cargo build --release --examples

# Runtime stage
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create a non-root user
RUN useradd -r -s /bin/false -m -d /app appuser

# Set working directory
WORKDIR /app

# Copy the built binary from builder stage
COPY --from=builder /app/target/release/examples/basic_usage /app/basic_usage
COPY --from=builder /app/target/release/examples/ /app/examples/

# Change ownership to appuser
RUN chown -R appuser:appuser /app

# Switch to non-root user
USER appuser

# Expose the port the app runs on
EXPOSE 3000

# Default command runs the basic usage example
CMD ["./basic_usage"]

# To run a different example, override the CMD:
# docker run -p 3000:3000 your-image ./examples/other_example