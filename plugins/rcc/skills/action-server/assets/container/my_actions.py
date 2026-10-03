from actions import action


@action(is_consequential=False)
def container_echo(message: str) -> str:
    """Return a message to verify the container action process.

    Args:
        message: Text to echo through the action process.
    """
    return f"container:{message}"
