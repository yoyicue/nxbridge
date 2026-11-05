#!/usr/bin/env python3
"""Exercise Sunshine's local session adapter and wire protocol with real sockets."""
import os
import pathlib
import socket
import struct
import subprocess
import tempfile
import time
import unittest

ROOT=pathlib.Path(__file__).resolve().parents[1]
FRAME=struct.Struct('!4sQHhhhh')
TOKEN=bytes(range(32))


def readn(s,n):
    b=b''
    while len(b)<n:
        chunk=s.recv(n-len(b))
        if not chunk: raise EOFError()
        b+=chunk
    return b


class BridgeTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp=tempfile.TemporaryDirectory(); p=pathlib.Path(cls.tmp.name)
        (p/'probe.cpp').write_text(r'''#include "sunshine_bridge.hpp"
#include <iostream>
int main() {
 std::cout << nxbridge::allowed("paired\n") << " " << nxbridge::allowed("other\n") << std::endl;
 auto all=nxbridge::convert(0xf3ff,255,128,1234,-2345,32767,-32767);
 std::cout << all.buttons << " " << all.axes[0] << " " << all.axes[1] << " " << all.axes[2] << " " << all.axes[3] << std::endl;
 nxbridge::sender bridge; bridge.start(); std::string command;
 while (std::getline(std::cin,command)) {
   if(command=="press") bridge.update(nxbridge::convert(0x1000,0,0,0,0,0,0));
   if(command=="release") bridge.update({});
   if(command=="combo") bridge.update(nxbridge::convert(0x2000,255,0,100,200,-300,400));
   if(command=="stop") bridge.stop();
   if(command=="start") bridge.start();
   if(command=="exit") break;
   std::cout << "ok" << std::endl;
 }
}''')
        subprocess.run(['clang++','-std=c++20','-pthread','-I',str(ROOT/'adapters/input'),str(p/'probe.cpp'),'-o',str(p/'probe')],check=True)
        (p/'client.pem').write_text('paired\n');cls.path=p

    @classmethod
    def tearDownClass(cls):cls.tmp.cleanup()

    def setUp(self):
        self.listener=socket.socket();self.listener.bind(('127.0.0.1',0));self.listener.listen(5);self.listener.settimeout(3)
        env=dict(os.environ,NXBRIDGE_INPUT_PORT=str(self.listener.getsockname()[1]),NXBRIDGE_INPUT_TOKEN=TOKEN.hex(),NXBRIDGE_CLIENT_CERT=str(self.path/'client.pem'))
        self.proc=subprocess.Popen([str(self.path/'probe')],env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True)
        self.assertEqual(self.proc.stdout.readline().strip(),'1 0')
        self.assertEqual(self.proc.stdout.readline().strip(),'65535 1234 -2345 32767 -32767')
        self.peer,_=self.listener.accept();self.peer.settimeout(3)
        self.assertEqual(readn(self.peer,32),TOKEN)
        self.assertEqual(FRAME.unpack(readn(self.peer,22))[2:],(0,0,0,0,0))

    def tearDown(self):
        self.proc.stdin.write('exit\n');self.proc.stdin.flush();self.proc.wait(timeout=4)
        self.proc.stdin.close();self.proc.stdout.close();self.peer.close();self.listener.close()

    def command(self,cmd):
        self.proc.stdin.write(cmd+'\n');self.proc.stdin.flush();self.assertEqual(self.proc.stdout.readline().strip(),'ok')

    def next_state(self):return FRAME.unpack(readn(self.peer,22))[2:]

    def wait_state(self,state):
        for _ in range(15):
            if self.next_state()==state:return
        self.fail(f'state {state} not received')

    def test_press_release_combo_and_stop(self):
        self.command('press');self.command('release')
        self.wait_state((1,0,0,0,0));self.wait_state((0,0,0,0,0))
        self.command('combo');self.wait_state((258,100,200,-300,400))
        self.command('stop')
        while self.peer.recv(22):pass

    def test_reconnect_requires_neutral_before_new_press(self):
        self.command('press');self.wait_state((1,0,0,0,0));self.peer.close()
        self.peer,_=self.listener.accept();self.peer.settimeout(3);self.assertEqual(readn(self.peer,32),TOKEN)
        self.assertEqual(self.next_state(),(0,0,0,0,0))
        self.command('press');self.assertEqual(self.next_state(),(0,0,0,0,0))
        self.command('release');self.command('press');self.wait_state((1,0,0,0,0))

if __name__=='__main__':unittest.main()
