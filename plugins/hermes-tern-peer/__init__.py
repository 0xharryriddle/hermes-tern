"""Hermes registration only; Rust owns mailbox authorization and persistence."""
from .adapter import register

__all__ = ["register"]
