"""Interfaces required by BabelDOC and the bundled translator services."""


def test_babeldoc_bitstring_interface_is_available():
    from bitstring import BitStream

    stream = BitStream(bin="1010")
    assert stream.bin == "1010"
