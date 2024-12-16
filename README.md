# BP API Service

The microservice for handling background removal request at [https://api.erasebg.org](https://api.erasebg.org).

## Environment variables

```markdown
MEDIA_ROOT=
MEDIA_URL=
MEDIA_SERVE_HOST=
BP_SERVER_HOST=
BP_SERVER_AUTH_TOKEN=
PROCESS_HARD=
POSTGRES_URL=
```

### Run

```shell
cargo run --release
```

## Docker commands

### Building image
```bash
sudo docker build -t sagasab/bp-api-service-rust:v0.1 .
```

### Running image
```bash
sudo docker pull ghcr.io/sagasab/bp-api-service-rust:v0.1
sudo docker run sagasab/bp-api-service-rust:v0.1
```

