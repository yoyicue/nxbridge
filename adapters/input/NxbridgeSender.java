package com.limelight.binding.input.virtual_controller;

import java.io.DataOutputStream;
import java.net.InetSocketAddress;
import java.net.Socket;
import java.util.concurrent.ArrayBlockingQueue;
import java.util.concurrent.TimeUnit;

/* One sender per displayed overlay. No input is sent to global host devices. */
final class NxbridgeSender {
    private static final String TOKEN = "@TOKEN@";
    private static final int PORT = @PORT@;
    private final ArrayBlockingQueue<byte[]> queue = new ArrayBlockingQueue<>(1024);
    private volatile boolean running = true;
    private volatile boolean connected;
    private volatile Socket socket;
    private byte[] current = new byte[10];

    NxbridgeSender() { new Thread(this::run, "nxbridge-input").start(); }

    synchronized void send(VirtualController.ControllerInputContext c) {
        int m = c.inputMap & 0xffff, b = 0;
        if ((m & 0x1000) != 0) b |= 1 << 0;  // A
        if ((m & 0x2000) != 0) b |= 1 << 1;  // B
        if ((m & 0x4000) != 0) b |= 1 << 2;  // X
        if ((m & 0x8000) != 0) b |= 1 << 3;  // Y
        if ((m & 0x0040) != 0) b |= 1 << 4;
        if ((m & 0x0080) != 0) b |= 1 << 5;
        if ((m & 0x0100) != 0) b |= 1 << 6;
        if ((m & 0x0200) != 0) b |= 1 << 7;
        if ((c.leftTrigger & 255) >= 128) b |= 1 << 8;
        if ((c.rightTrigger & 255) >= 128) b |= 1 << 9;
        if ((m & 0x0010) != 0) b |= 1 << 10;
        if ((m & 0x0020) != 0) b |= 1 << 11;
        if ((m & 0x0004) != 0) b |= 1 << 12;
        if ((m & 0x0001) != 0) b |= 1 << 13;
        if ((m & 0x0008) != 0) b |= 1 << 14;
        if ((m & 0x0002) != 0) b |= 1 << 15;
        byte[] state = new byte[10]; put16(state, 0, b);
        put16(state, 2, c.leftStickX); put16(state, 4, c.leftStickY);
        put16(state, 6, c.rightStickX); put16(state, 8, c.rightStickY);
        if (!connected) return;
        current = state;
        // Never discard a press or release while continuing the connection.
        if (running && !queue.offer(state)) close();
    }
    private static void put16(byte[] b, int i, int v) { b[i] = (byte)(v >> 8); b[i+1] = (byte)v; }
    private synchronized byte[] current() { return current; }
    void close() {
        running = false;
        Socket s = socket;
        if (s != null) try { s.close(); } catch (Exception ignored) { }
    }
    private void run() {
        try (Socket s = new Socket()) {
            socket = s;
            if (!running) return;
            s.connect(new InetSocketAddress("127.0.0.1", PORT), 1000);
            s.setTcpNoDelay(true);
            DataOutputStream out = new DataOutputStream(s.getOutputStream());
            for (int i = 0; i < 64; i += 2) out.writeByte(Integer.parseInt(TOKEN.substring(i, i+2), 16));
            out.flush();
            connected = true;
            long seq = 0;
            while (running) {
                byte[] state = queue.poll(200, TimeUnit.MILLISECONDS);
                if (state == null) state = current();
                out.writeInt(0x4e584931); out.writeLong(++seq); out.write(state); out.flush();
            }
        } catch (Exception ignored) {
            // Receiver releases all input on disconnect or heartbeat timeout.
        } finally { running = false; socket = null; }
    }
}
