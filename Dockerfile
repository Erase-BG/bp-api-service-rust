FROM rust:1.79 AS builder

# Sets the working directory for the build
WORKDIR /app

# Copy files
COPY . .

# Builds the Rust binary in release mode
RUN cargo build --release

# Makes binary executable
RUN chmod +x target/release/bp-api-service

# Runtime stage
FROM gcr.io/distroless/cc

WORKDIR /app

# Copies compiled binary from the builder stage
COPY --from=builder /app/target/release/bp-api-service .

# Runs the compiled binary
CMD ["./bp-api-service"]
