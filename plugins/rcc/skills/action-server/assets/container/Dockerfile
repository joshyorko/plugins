# syntax=docker/dockerfile:1
FROM python:3.12.11-slim-bookworm@sha256:519591d6871b7bc437060736b9f7456b8731f1499a57e22e6c285135ae657bf7

# Latest PyPI releases inspected on 2026-10-03. Refresh and test pins together.
ARG ACTIONS_RUNTIME_VERSION=1.0.2
ARG ACTIONS_CORE_VERSION=1.0.1
RUN test "$(dpkg --print-architecture)" = amd64 \
    && apt-get update \
    && apt-get install --no-install-recommends -y ca-certificates curl procps \
    && rm -rf /var/lib/apt/lists/* \
    && python -m pip install --no-cache-dir \
       "actions-runtime==${ACTIONS_RUNTIME_VERSION}" "actions-core==${ACTIONS_CORE_VERSION}"

RUN groupadd --gid 10001 action-user \
    && useradd --uid 10001 --gid 10001 --create-home action-user \
    && mkdir -p /opt/actions /var/lib/action-server /home/action-user/.actions \
    && chown -R 10001:10001 /opt/actions /var/lib/action-server /home/action-user
ENV ACTIONS_HOME=/home/action-user/.actions \
    SEMA4AI_OPTIMIZE_FOR_CONTAINER=1
COPY --chmod=0755 entrypoint.sh /usr/local/bin/start-actions
WORKDIR /opt/actions
COPY --chown=10001:10001 . .
USER 10001:10001

# Keep resolved environments in the image, but initialize the real database
# only after the persistent volume is mounted at container start.
RUN action-server import --dir /opt/actions --datadir /tmp/build-datadir \
    && rm -rf /tmp/build-datadir
EXPOSE 8080
ENTRYPOINT ["/bin/sh", "/usr/local/bin/start-actions"]
