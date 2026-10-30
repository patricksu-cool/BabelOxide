These fixed HTTP response bodies are synthetic test data. They were encoded
independently with CPython gzip (mtime=0), zlib, Brotli, and pyzstd; Rust tests
do not generate data with the decoder being tested.

The JSON and HTML payloads test Unicode/parsing and markup preservation. The
small `oversize.gzip` expands to 4 MiB + 1 byte, proving that the response limit
applies after decompression. Malformed and stalled bodies are produced by the
loopback server in `compressed_http.rs`. These fixtures do not establish raw
nonstandard deflate, stacked encodings, or every remote provider's behavior.
