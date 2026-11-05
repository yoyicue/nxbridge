#!/usr/bin/env python3
"""Exercise the actual Java sender's Switch mapping over a local TCP socket."""
import pathlib
import socket
import struct
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]

class InputSenderTest(unittest.TestCase):
    def test_mapping_and_ordered_release(self):
        with tempfile.TemporaryDirectory() as tmp, socket.socket() as listener:
            listener.bind(('127.0.0.1', 0)); listener.listen(1); listener.settimeout(5)
            port = listener.getsockname()[1]
            source = (ROOT / 'adapters/input/NxbridgeSender.java').read_text().replace('@TOKEN@', 'ab' * 32).replace('@PORT@', str(port))
            directory = pathlib.Path(tmp) / 'com/limelight/binding/input/virtual_controller'
            directory.mkdir(parents=True)
            (directory / 'NxbridgeSender.java').write_text(source)
            (directory / 'VirtualController.java').write_text('''package com.limelight.binding.input.virtual_controller;
public class VirtualController { public static class ControllerInputContext {
public short inputMap, leftStickX, leftStickY, rightStickX, rightStickY;
public byte leftTrigger, rightTrigger; } }
''')
            (directory / 'Probe.java').write_text('''package com.limelight.binding.input.virtual_controller;
public class Probe { public static void main(String[] args) throws Exception {
NxbridgeSender s = new NxbridgeSender(); Thread.sleep(300);
VirtualController.ControllerInputContext c = new VirtualController.ControllerInputContext();
c.inputMap=(short)0xf3ff; c.leftTrigger=(byte)255; c.rightTrigger=(byte)128;
c.leftStickX=1234; c.leftStickY=-2345; c.rightStickX=32767; c.rightStickY=-32767;
s.send(c); c.inputMap=0; c.leftTrigger=0; c.rightTrigger=0;
c.leftStickX=0; c.leftStickY=0; c.rightStickX=0; c.rightStickY=0; s.send(c);
Thread.sleep(300); s.close(); } }
''')
            subprocess.run(['javac', '-d', tmp, *(str(p) for p in directory.glob('*.java'))], check=True, stdout=subprocess.DEVNULL)
            proc = subprocess.Popen(['java', '-cp', tmp, 'com.limelight.binding.input.virtual_controller.Probe'])
            with listener.accept()[0] as client:
                client.settimeout(3)
                data = b''
                while True:
                    chunk = client.recv(4096)
                    if not chunk: break
                    data += chunk
            self.assertEqual(proc.wait(timeout=3), 0)
            self.assertEqual(data[:32], bytes.fromhex('ab' * 32))
            payload = data[32:]
            self.assertEqual(len(payload) % 22, 0)
            frames = [struct.unpack('!4sQHhhhh', payload[i:i+22]) for i in range(0, len(payload), 22)]
            self.assertTrue(all(f[0] == b'NXI1' for f in frames))
            self.assertEqual([f[1] for f in frames], list(range(1, len(frames)+1)))
            states = [f[2:] for f in frames]
            self.assertIn((0xffff,1234,-2345,32767,-32767), states)
            self.assertEqual(states[-1], (0,0,0,0,0))

if __name__ == '__main__': unittest.main()
