# SPDX-License-Identifier: GPL-3.0-or-later
"""Real C/Wayland sync with a synthetic Unix peer, never the user's display."""
import ctypes
import os
from pathlib import Path
import shlex
import socket
import struct
import subprocess
import tempfile
import threading
import time
import unittest


class NativeWaylandDeadlineTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        root = Path(__file__).resolve().parents[1]
        output = str(Path(cls.tmp.name) / 'wait.so')
        flags = shlex.split(subprocess.check_output(
            ['pkg-config', '--cflags', '--libs', 'wayland-client'], text=True))
        subprocess.run(['cc', '-std=c11', '-Wall', '-Wextra', '-Werror', '-shared', '-fPIC',
                        str(root / 'tests/native_wayland_wait_probe.c'),
                        str(root / 'native/wayland_wait.c'), '-o', output, *flags], check=True)
        cls.lib = ctypes.CDLL(output)
        cls.lib.probe_wait.argtypes = (ctypes.c_int, ctypes.c_uint, ctypes.c_uint)
        cls.lib.probe_wait.restype = ctypes.c_int

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def exchange(self, timeout, delays, fragment=False, disconnect=False):
        client, peer = socket.socketpair()
        errors = []

        def server():
            try:
                peer.settimeout(2)
                for delay in delays:
                    request = b''
                    while len(request) < 12:
                        data = peer.recv(12 - len(request))
                        if not data:
                            return
                        request += data
                    obj, header, callback = struct.unpack('=III', request)
                    if (obj, header) != (1, 12 << 16):
                        raise AssertionError('Expected core wl_display.sync only')
                    if disconnect:
                        return
                    time.sleep(delay)
                    response = struct.pack('=III', callback, 12 << 16, 7)
                    if fragment:
                        peer.sendall(response[:5])
                        time.sleep(0.005)
                        peer.sendall(response[5:])
                    else:
                        peer.sendall(response)
            except (BrokenPipeError, ConnectionResetError):
                pass  # Expected when the timed-out client destroys its display.
            except BaseException as error:
                errors.append(error)
            finally:
                peer.close()

        thread = threading.Thread(target=server, daemon=True)
        thread.start()
        try:
            started = time.monotonic()
            result = self.lib.probe_wait(os.dup(client.fileno()), timeout, len(delays))
            elapsed = time.monotonic() - started
        finally:
            client.close()
            thread.join(timeout=3)
        self.assertFalse(thread.is_alive(), 'Synthetic peer must be collected')
        if errors:
            raise errors[0]
        return result, elapsed

    def test_sync_ack_is_confirmed_without_opening_a_display(self):
        self.assertEqual(self.exchange(500, [0])[0], 0)

    def test_fragmented_ack_is_not_lost(self):
        self.assertEqual(self.exchange(500, [0], fragment=True)[0], 0)

    def test_silent_or_late_peer_times_out_instead_of_blocking_forever(self):
        result, elapsed = self.exchange(20, [0.08])
        self.assertEqual(result, -1)
        self.assertLess(elapsed, 1)

    def test_all_rounds_share_one_absolute_budget(self):
        self.assertEqual(self.exchange(50, [0.03, 0.06])[0], -1)

    def test_disconnect_is_not_reported_as_a_success(self):
        self.assertEqual(self.exchange(500, [0], disconnect=True)[0], -1)

    def test_expired_budget_never_sends_a_sync_request(self):
        self.assertEqual(self.exchange(0, [0])[0], -1)
