"""The documented package exports must remain importable."""

import pdf2zh_next


def test_declared_public_exports_are_available():
    namespace = {}
    exec("from pdf2zh_next import *", namespace)  # noqa: S102 - fixed public import contract
    assert set(pdf2zh_next.__all__) <= namespace.keys()
    assert namespace["ClaudeCodeSettings"] is pdf2zh_next.config.ClaudeCodeSettings
