FROM ghcr.io/astral-sh/uv:0.11.6-python3.13-alpine
ENV PYTHONPATH=/app:$PYTHONPATH
ENV PYTHONUNBUFFERED=1
RUN apk add build-base git
WORKDIR /app
COPY pyproject.toml /app/pyproject.toml
COPY src /app/src
RUN uv sync
