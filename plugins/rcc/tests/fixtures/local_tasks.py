"""Dummy upstream SDK fixtures; fail if a task attempts Python networking."""

import socket
from contextlib import contextmanager
from unittest.mock import patch

from robocorp.tasks import task


def deny_network(*args, **kwargs):
    raise AssertionError("Network access is forbidden in local compatibility fixtures")


@contextmanager
def local_only():
    with patch.object(socket.socket, "connect", deny_network), \
         patch.object(socket.socket, "connect_ex", deny_network), \
         patch.object(socket, "create_connection", deny_network), \
         patch.object(socket, "getaddrinfo", deny_network):
        yield


@task
def producer():
    with local_only():
        from robocorp import vault, workitems

        assert vault.get_secret("fixture")["token"] == "test-only"
        for seed in workitems.inputs:
            with seed:
                for row in seed.payload["rows"]:
                    workitems.outputs.create({"id": row["id"], "status": "new"})


@task
def consumer():
    with local_only():
        from robocorp import workitems

        for item in workitems.inputs:
            with item:
                workitems.outputs.create({"id": item.payload["id"], "processed": True})
