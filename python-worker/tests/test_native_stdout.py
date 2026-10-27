"""Check the actual OS pipes, including native Windows stdout writes."""

import json
import subprocess
import sys
from pathlib import Path


def test_python_crt_and_win32_diagnostics_cannot_corrupt_protocol():
    path = Path(__file__).resolve().parents[1] / "worker.py"
    code = f"""
import ctypes
import importlib.util
import os
spec = importlib.util.spec_from_file_location('boundary', {str(path)!r})
boundary = importlib.util.module_from_spec(spec)
spec.loader.exec_module(boundary)
protocol = boundary.isolate_stdout()
assert not os.get_inheritable(protocol.fileno())
print('python diagnostic', flush=True)
os.write(1, b'CRT diagnostic\\n')
if os.name == 'nt':
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    kernel.GetStdHandle.argtypes = [ctypes.c_ulong]
    kernel.GetStdHandle.restype = ctypes.c_void_p
    kernel.WriteFile.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_ulong, ctypes.POINTER(ctypes.c_ulong), ctypes.c_void_p]
    kernel.WriteFile.restype = ctypes.c_int
    data = ctypes.create_string_buffer(b'Win32 diagnostic\\n')
    count = ctypes.c_ulong()
    assert kernel.WriteFile(kernel.GetStdHandle(-11 & 0xffffffff), data, len(data.value), ctypes.byref(count), None)
protocol.write(b'{{"v":1,"type":"native_test"}}\\n')
protocol.close()
"""
    result = subprocess.run(
        [sys.executable, "-I", "-u", "-c", code],
        capture_output=True,
        timeout=15,
        check=True,
    )
    assert json.loads(result.stdout) == {"v": 1, "type": "native_test"}
    assert b"python diagnostic" in result.stderr
    assert b"CRT diagnostic" in result.stderr
    if sys.platform == "win32":
        assert b"Win32 diagnostic" in result.stderr


def test_protocol_stdin_is_private_from_python_and_native_stdin():
    path = Path(__file__).resolve().parents[1] / "worker.py"
    code = f"""
import ctypes
import importlib.util
import json
import os
import sys
spec = importlib.util.spec_from_file_location('boundary', {str(path)!r})
boundary = importlib.util.module_from_spec(spec)
spec.loader.exec_module(boundary)
protocol = boundary.isolate_stdin()
assert not os.get_inheritable(protocol.fileno())
assert sys.stdin.buffer.read(1) == b''
assert os.read(0, 1) == b''
if os.name == 'nt':
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    kernel.GetStdHandle.argtypes = [ctypes.c_ulong]
    kernel.GetStdHandle.restype = ctypes.c_void_p
    kernel.ReadFile.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_ulong, ctypes.POINTER(ctypes.c_ulong), ctypes.c_void_p]
    kernel.ReadFile.restype = ctypes.c_int
    data = ctypes.create_string_buffer(1)
    count = ctypes.c_ulong()
    assert kernel.ReadFile(kernel.GetStdHandle(-10 & 0xffffffff), data, 1, ctypes.byref(count), None)
    assert count.value == 0
assert boundary.read_frame(protocol)['text'] == 'private message'
print('private input verified', flush=True)
protocol.close()
"""
    result = subprocess.run(
        [sys.executable, "-I", "-u", "-c", code],
        input=b'{"v":1,"text":"private message"}\n',
        capture_output=True,
        timeout=15,
        check=True,
    )
    assert result.stdout.strip() == b"private input verified"
