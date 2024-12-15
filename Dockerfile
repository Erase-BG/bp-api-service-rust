FROM rust:1.79 AS builder

# Sets the working directory for the build
WORKDIR /app

# Copy files
COPY . .

# Builds the Rust binary in release mode
RUN cargo build --release

# Runtime stage
FROM alpine:latest

WORKDIR /app

# Copies compiled binary from the builder stage
COPY --from=builder /app/target/release/bp-api-service .

# Makes binary executable
RUN chmod +x bp-api-service

# Runs the compiled binary
CMD ["./bp-api-service"]
