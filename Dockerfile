FROM ghcr.io/astral-sh/uv:python3.13-bookworm-slim

WORKDIR /app
EXPOSE 7860
ENV PYTHONUNBUFFERED=1

RUN apt-get update && \
    apt-get install --no-install-recommends -y libgl1 libglib2.0-0 libxext6 libsm6 libxrender1 && \
    rm -rf /var/lib/apt/lists/*

COPY . .
RUN uv sync --locked --no-dev --no-editable && \
    uv run --no-sync babeldoc --version && \
    uv run --no-sync babeldoc --warmup && \
    uv run --no-sync pdf2zh --version

# Explicitly pass --server-name 0.0.0.0 to publish the container port.
# Bind the host side to loopback, or use authentication on a trusted LAN.
CMD ["uv", "run", "--no-sync", "pdf2zh", "--gui"]
