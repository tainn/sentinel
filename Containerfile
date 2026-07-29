FROM ghcr.io/astral-sh/uv:0.12.0-python3.14-alpine3.23
ENV PYTHONPATH=/app:$PYTHONPATH
ENV PYTHONUNBUFFERED=1
RUN apk add build-base git
WORKDIR /app
COPY pyproject.toml /app/pyproject.toml
COPY src /app/src
RUN uv sync
