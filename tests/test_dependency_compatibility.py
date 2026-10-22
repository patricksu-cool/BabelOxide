"""Interfaces required by BabelDOC and the bundled translator services."""

from importlib.metadata import version

from packaging.version import Version


def test_patched_babeldoc_and_compatible_pymupdf_are_installed():
    assert Version(version("babeldoc")) >= Version("0.6.3")
    assert Version(version("pymupdf")) >= Version("1.26.7")


def test_babeldoc_bitstring_interface_is_available():
    from bitstring import BitStream

    stream = BitStream(bin="1010")
    assert stream.bin == "1010"
