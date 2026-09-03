"""Entry point. Imports both other modules.

FileID: 0f0f0f0f-8888-8888-8888-888888888888
"""

from app.helpers import shout
from app.models import Message


def run(text):
    """ID: 0f0f0f0f-9999-9999-9999-999999999999"""
    message = Message(shout(text))
    return message.render()
