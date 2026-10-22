"""Redact configured credentials before they reach application log handlers."""

from __future__ import annotations

import logging
import re
import shlex
import threading
import traceback
from collections.abc import Mapping
from typing import Any

from pydantic import BaseModel

REDACTED = "[REDACTED]"

_lock = threading.RLock()
_known_secrets: set[str] = set()
_sensitive_suffixes = (
    "apikey",
    "authkey",
    "secretkey",
    "secretid",
    "clientsecret",
    "password",
    "passwd",
    "accesstoken",
    "refreshtoken",
    "credential",
    "credentials",
)
_command_fields = {"clitranslatorcommand", "clitranslatorpostprocesscommand"}
_sensitive_exact = {
    "authorization",
    "proxyauthorization",
    "secret",
    "token",
    *_command_fields,
}
_bearer = re.compile(r"(?i)\b(Bearer|Basic)\s+([A-Za-z0-9._~+/-]+)")
_url_credentials = re.compile(r"(https?://[^/\s:@]+:)[^@\s/]+@", re.IGNORECASE)
_key_value = re.compile(
    r"(?i)(\b(?:[a-z0-9_.-]*(?:api[_-]?key|auth[_-]?key|secret(?:[_-]?(?:id|key))?|password|passwd|access[_-]?token|refresh[_-]?token|client[_-]?secret)|token|authorization|proxy_authorization|credentials?)\b['\"]?\s*[:=]\s*['\"]?)([^'\"\s,;}\]]+)"
)


def is_sensitive_field(name: object) -> bool:
    """Recognize credential fields without masking unrelated token counts."""
    normalized = re.sub(r"[^a-z0-9]", "", str(name).lower())
    return normalized in _sensitive_exact or normalized.endswith(_sensitive_suffixes)


def _as_mapping(value: Any) -> Any:
    return value.model_dump() if isinstance(value, BaseModel) else value


def _register_command_values(command: str) -> None:
    try:
        parts = shlex.split(command)
    except ValueError:
        return
    for index, part in enumerate(parts):
        flag, separator, inline_value = part.partition("=")
        if not is_sensitive_field(flag.lstrip("-")):
            continue
        candidate = (
            inline_value
            if separator
            else parts[index + 1]
            if index + 1 < len(parts)
            else ""
        )
        if len(candidate) >= 6:
            with _lock:
                _known_secrets.add(candidate)


def register_secret_values(value: Any) -> None:
    """Remember configured secrets so an unlabeled exception cannot echo them."""
    value = _as_mapping(value)
    if isinstance(value, Mapping):
        for key, item in value.items():
            if is_sensitive_field(key):
                if isinstance(item, str) and len(item) >= 6:
                    with _lock:
                        _known_secrets.add(item)
                    if re.sub(r"[^a-z0-9]", "", str(key).lower()) in _command_fields:
                        _register_command_values(item)
            else:
                register_secret_values(item)
    elif isinstance(value, (list, tuple, set)):
        for item in value:
            register_secret_values(item)


def redact_text(value: object) -> str:
    """Mask known credentials and common header, URL and key/value forms."""
    result = str(value)
    with _lock:
        secrets = sorted(_known_secrets, key=len, reverse=True)
    for secret in secrets:
        result = result.replace(secret, REDACTED)
    result = _bearer.sub(lambda match: f"{match[1]} {REDACTED}", result)
    result = _url_credentials.sub(lambda match: f"{match[1]}{REDACTED}@", result)
    result = _key_value.sub(lambda match: f"{match[1]}{REDACTED}", result)
    return result


def redact_for_log(value: Any) -> Any:
    """Return a log-only copy of a model or nested collection."""
    value = _as_mapping(value)
    if isinstance(value, Mapping):
        redacted = {}
        for key, item in value.items():
            if is_sensitive_field(key):
                register_secret_values({key: item})
                redacted[key] = REDACTED if item is not None else None
            else:
                redacted[key] = redact_for_log(item)
        return redacted
    if isinstance(value, (list, tuple, set)):
        return [redact_for_log(item) for item in value]
    if isinstance(value, str):
        return redact_text(value)
    return value


class RedactingLogFilter(logging.Filter):
    """Sanitize a record, including its traceback, before handler formatting."""

    def filter(self, record: logging.LogRecord) -> bool:
        try:
            message = record.getMessage()
        except Exception:
            message = "[unformattable log message]"
        if record.exc_info:
            exc_type, exc, tb = record.exc_info
            if exc is not None:
                message = message.replace(str(exc), f"<{type(exc).__name__}>")
            if tb is not None:
                message += "\n" + "".join(traceback.format_tb(tb))
            if exc_type is not None:
                message += f"{exc_type.__name__} (message omitted from logs)"
            record.exc_info = None
            record.exc_text = None
        record.msg = redact_text(message)
        record.args = ()
        if record.stack_info:
            record.stack_info = redact_text(record.stack_info)
        return True


def install_redacting_filters() -> None:
    """Protect each currently configured root handler, including queue handlers."""
    for handler in logging.getLogger().handlers:
        if not any(isinstance(item, RedactingLogFilter) for item in handler.filters):
            handler.addFilter(RedactingLogFilter())
