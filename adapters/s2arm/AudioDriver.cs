// SPDX-License-Identifier: MIT
// NXBridge: a process-private, clocked stereo sink for S2Arm's Apple host.
using Ryujinx.Audio.Backends.Common;
using Ryujinx.Audio.Common;
using Ryujinx.Audio.Integration;
using Ryujinx.Memory;
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Threading;
using static Ryujinx.Audio.Integration.IHardwareDeviceDriver;

namespace S2A.Apple.Audio
{
    public delegate void NXBridgePcmSink(ReadOnlySpan<short> samples);

    public sealed class NXBridgeAudioDriver : IHardwareDeviceDriver
    {
        internal const int Rate = 48000, Channels = 2, Frames = 240;
        internal const int MaxSessions = 8, MaxBuffers = 64, MaxBytes = Rate * Channels * 2 * 2;
        private readonly object _lock = new();
        private readonly List<Session> _sessions = new(MaxSessions);
        private readonly ManualResetEvent _update = new(false), _pause = new(true);
        private readonly ManualResetEventSlim _stop = new(false);
        private readonly float[] _mix = new float[Frames * Channels];
        private readonly short[] _pcm = new short[Frames * Channels];
        private readonly NXBridgePcmSink _push;
        private readonly Thread _worker;
        private bool _disposed;
        public static bool IsSupported => OperatingSystem.IsMacOS();
        public float Volume { get; set; } = 1;

        [DllImport("s2a_native", EntryPoint = "nxb_pcm_probe_start")]
        private static extern void NativeStart();
        [DllImport("s2a_native", EntryPoint = "nxb_pcm_probe_push")]
        private static extern unsafe void NativePush(short* samples, uint frames);

        private static NXBridgePcmSink NativeSink()
        {
            NativeStart();
            return PushNative;
        }
        private static unsafe void PushNative(ReadOnlySpan<short> samples)
        {
            fixed (short* data = samples) NativePush(data, (uint)(samples.Length / Channels));
        }

        public NXBridgeAudioDriver() : this(NativeSink(), true) { }
        // Explicit injection lets device-free tests exercise the real clock and
        // completion rules without loading a game or calling the Mach transport.
        internal NXBridgeAudioDriver(NXBridgePcmSink push, bool startThread)
        {
            _push = push;
            if (startThread)
            {
                _worker = new Thread(Run) { IsBackground = true, Name = "NXBridge.Audio" };
                _worker.Start();
            }
        }

        private void Run()
        {
            long period = Stopwatch.Frequency / 200;
            long deadline = Stopwatch.GetTimestamp() + period;
            while (!_stop.IsSet)
            {
                long remaining = deadline - Stopwatch.GetTimestamp();
                if (remaining > 0)
                {
                    if (_stop.Wait(TimeSpan.FromSeconds((double)remaining / Stopwatch.Frequency))) break;
                    continue;
                }
                Tick();
                // A stalled consumer resumes at the current clock. It never
                // bursts through old guest buffers to catch up missed periods.
                deadline += period;
                if (deadline <= Stopwatch.GetTimestamp()) deadline = Stopwatch.GetTimestamp() + period;
            }
        }

        internal void Tick()
        {
            lock (_lock)
            {
                if (_disposed) return;
                Array.Clear(_mix);
                bool active = false;
                foreach (Session session in _sessions) active |= session.Mix(_mix);
                if (!active) return;
                float gain = float.IsFinite(Volume) ? Math.Clamp(Volume, 0, 1) : 0;
                for (int i = 0; i < _pcm.Length; i++)
                    _pcm[i] = (short)Math.Clamp(_mix[i] * gain, short.MinValue, short.MaxValue);
                _push(_pcm);
            }
        }

        public IHardwareDeviceSession OpenDeviceSession(Direction direction, IVirtualMemoryManager memory,
            SampleFormat format, uint rate, uint channels)
        {
            if (rate == 0) rate = Rate;
            if (channels == 0) channels = Channels;
            if (!SupportsDirection(direction) || !SupportsSampleRate(rate) ||
                !SupportsSampleFormat(format) || !SupportsChannelCount(channels))
                throw new NotSupportedException("NXBridge sink requires stereo 48 kHz signed PCM16 output");
            lock (_lock)
            {
                ObjectDisposedException.ThrowIf(_disposed, this);
                if (_sessions.Count == MaxSessions) throw new InvalidOperationException("NXBridge audio session limit");
                var session = new Session(this, memory, format, rate, channels);
                _sessions.Add(session);
                return session;
            }
        }
        public bool SupportsDirection(Direction direction) => direction == Direction.Output;
        public bool SupportsSampleRate(uint rate) => rate == Rate;
        public bool SupportsSampleFormat(SampleFormat format) => format == SampleFormat.PcmInt16;
        public bool SupportsChannelCount(uint channels) => channels == Channels;
        public ManualResetEvent GetUpdateRequiredEvent() => _update;
        public ManualResetEvent GetPauseEvent() => _pause;
        public void Dispose()
        {
            _stop.Set();
            _worker?.Join();
            lock (_lock)
            {
                if (_disposed) return;
                _disposed = true;
                foreach (Session session in _sessions) session.Close();
                _sessions.Clear();
                // Ryujinx owns disposal of the update-required event.
                _pause.Dispose();
            }
            // Retain _stop for idempotent Dispose calls.
        }

        private sealed class Session : HardwareDeviceSessionOutputBase
        {
            private readonly NXBridgeAudioDriver _driver;
            private readonly AudioSampleQueue _samples = new(MaxBytes);
            private readonly Queue<(AudioBuffer Buffer, ulong End, bool Copied)> _pending = new(MaxBuffers);
            private readonly byte[] _scratch = new byte[Frames * Channels * 2];
            private ulong _submitted, _played;
            private ulong _backpressureWaits;
            private float _volume = 1;
            private bool _started, _closed;

            public Session(NXBridgeAudioDriver driver, IVirtualMemoryManager memory,
                SampleFormat format, uint rate, uint channels) : base(memory, format, rate, channels)
                => _driver = driver;

            public override void QueueBuffer(AudioBuffer buffer)
            {
                lock (_driver._lock)
                {
                    ObjectDisposedException.ThrowIf(_closed, this);
                    ulong bytes = buffer.Data != null ? (ulong)buffer.Data.Length : buffer.DataSize;
                    if (bytes % (Channels * 2) != 0 || bytes > MaxBytes)
                        throw new InvalidOperationException("NXBridge audio buffer alignment or size limit");
                    foreach (var entry in _pending)
                        if (ReferenceEquals(entry.Buffer, buffer) && (buffer.Data == null || !entry.Copied))
                            throw new InvalidOperationException("NXBridge borrowed audio buffer queued twice before consumption");
                    if (bytes == 0) return;
                    if (bytes > (ulong)(MaxBytes - _samples.Length) || _pending.Count == MaxBuffers)
                    {
                        if (!_started)
                            throw new InvalidOperationException("NXBridge inactive audio queue is full");
                        if (++_backpressureWaits == 1)
                            Console.Error.WriteLine("[nxb] audio producer waiting for bounded queue capacity");
                        // Renderer output has no upstream completion check. A
                        // delayed consumer must slow that producer instead of
                        // aborting the game, dropping PCM or growing the queue.
                        // Wait releases the driver lock so Tick / Close can run.
                        while (bytes > (ulong)(MaxBytes - _samples.Length) || _pending.Count == MaxBuffers)
                        {
                            Monitor.Wait(_driver._lock);
                            ObjectDisposedException.ThrowIf(_closed, this);
                        }
                    }
                    // HardwareDeviceImpl rotates four descriptors and one PCM
                    // array without waiting for playback. AudioSampleQueue owns
                    // a separate copy for each Data submission. Registered guest
                    // buffers remain borrowed and cannot be reused this way.
                    _samples.Enqueue(buffer);
                    _submitted += bytes / (Channels * 2);
                    _pending.Enqueue((buffer, _submitted, buffer.Data != null));
                }
            }

            internal bool Mix(float[] mix)
            {
                if (!_started || _closed) return false;
                int bytes = _samples.Read(_scratch);
                ReadOnlySpan<short> pcm = MemoryMarshal.Cast<byte, short>(_scratch.AsSpan(0, bytes));
                float gain = float.IsFinite(_volume) ? Math.Clamp(_volume, 0, 1) : 0;
                for (int i = 0; i < pcm.Length; i++) mix[i] += pcm[i] * gain;
                _played += (ulong)(bytes / (Channels * 2));
                while (_pending.Count > 0 && _pending.Peek().End <= _played) _pending.Dequeue();
                if (bytes > 0)
                {
                    _driver._update.Set();
                    Monitor.PulseAll(_driver._lock);
                }
                return true;
            }
            public override bool WasBufferFullyConsumed(AudioBuffer buffer)
            {
                lock (_driver._lock)
                {
                    foreach (var entry in _pending)
                        if (ReferenceEquals(entry.Buffer, buffer)) return false;
                    return true;
                }
            }
            public override ulong GetPlayedSampleCount() { lock (_driver._lock) return _played; }
            public override void SetVolume(float volume) { lock (_driver._lock) _volume = volume; }
            public override float GetVolume() { lock (_driver._lock) return _volume; }
            public override void Start() { lock (_driver._lock) { ObjectDisposedException.ThrowIf(_closed, this); _started = true; } }
            public override void Stop() { lock (_driver._lock) _started = false; }
            public override void PrepareToClose() { }
            public override void UnregisterBuffer(AudioBuffer buffer) { lock (_driver._lock) _samples.Release(buffer); }
            internal void Close()
            {
                if (_closed) return;
                _started = false;
                _closed = true;
                _pending.Clear();
                Monitor.PulseAll(_driver._lock);
                if (_backpressureWaits > 0)
                    Console.Error.WriteLine($"[nxb] audio producer capacity waits={_backpressureWaits}");
            }
            public override void Dispose()
            {
                lock (_driver._lock) { Close(); _driver._sessions.Remove(this); }
            }
        }
    }
}
