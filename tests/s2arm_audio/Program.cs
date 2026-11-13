using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Threading;
using Ryujinx.Audio.Common;
using Ryujinx.Audio.Integration;
using S2A.Apple.Audio;
using static Ryujinx.Audio.Integration.IHardwareDeviceDriver;

static class Test
{
    static int checks;
    static void Check(bool ok, string why)
    {
        checks++;
        if (!ok) throw new Exception(why);
    }
    static void Reject<T>(Action action, string why) where T : Exception
    {
        try { action(); } catch (T) { checks++; return; }
        throw new Exception(why);
    }
    static IHardwareDeviceSession Open(NXBridgeAudioDriver driver) =>
        driver.OpenDeviceSession(Direction.Output, null, SampleFormat.PcmInt16, 48000, 2);
    static AudioBuffer Buffer(int frames, short value)
    {
        byte[] data = new byte[frames * 4];
        MemoryMarshal.Cast<byte, short>(data.AsSpan()).Fill(value);
        return new AudioBuffer { Data = data, DataSize = (ulong)data.Length };
    }
    static int Main()
    {
        var packets = new List<short[]>();
        using (var driver = new NXBridgeAudioDriver(pcm => packets.Add(pcm.ToArray()), false))
        using (var session = Open(driver))
        {
            var buffer = Buffer(480, 12000);
            session.QueueBuffer(buffer);
            Check(!session.WasBufferFullyConsumed(buffer), "enqueue must not release guest audio early");
            driver.Tick();
            Check(packets.Count == 0 && session.GetPlayedSampleCount() == 0, "not started must not consume");
            Array.Clear(buffer.Data); // renderer reuses its storage after QueueBuffer
            session.Start(); driver.Tick();
            Check(packets.Count == 1 && packets[0].Length == 480 && packets[0][0] == 12000 && packets[0][479] == 12000,
                  "queued renderer data must be copied and sent as 240 stereo frames");
            Check(session.GetPlayedSampleCount() == 240 && !session.WasBufferFullyConsumed(buffer), "half buffer is still owned");
            var stats = driver.GetStatistics();
            Check(stats.QueuedFrames == 240 && stats.PeakQueuedFrames == 480 && stats.UnderrunFrames == 0,
                  "queue diagnostics use real submitted/consumed frames");
            session.Stop(); driver.Tick();
            Check(packets.Count == 1 && session.GetPlayedSampleCount() == 240, "pause preserves queued samples");
            session.Start(); session.SetVolume(.5f); driver.Volume = .5f; driver.Tick();
            Check(packets[1][0] == 3000 && session.WasBufferFullyConsumed(buffer), "gain and final completion");
            driver.Tick();
            Check(packets[2][0] == 0 && packets[2][479] == 0 && session.GetPlayedSampleCount() == 480,
                  "underrun emits silence without advancing guest completion");
            Check(driver.GetStatistics().QueuedFrames == 0 && driver.GetStatistics().UnderrunFrames == 240,
                  "diagnostics count missing frames without treating silence as consumed guest audio");
            Check(driver.GetUpdateRequiredEvent().WaitOne(0), "consumer wakes the audio renderer");
            session.Dispose(); driver.Tick();
            Check(packets.Count == 3, "disposed session must stop output");
        }
        packets.Clear();
        using (var driver = new NXBridgeAudioDriver(pcm => packets.Add(pcm.ToArray()), false))
        using (var renderer = new HardwareDeviceImpl(driver, 2, 48000))
        {
            // The real pinned renderer reuses four AudioBuffer objects and one
            // data array. A consumer delay can leave more than four in flight.
            var samples = new short[480];
            for (int i = 0; i < 8; i++)
            {
                Array.Fill(samples, (short)(1000 + i));
                renderer.AppendBuffer(samples, 2);
            }
            Array.Clear(samples);
            for (int i = 0; i < 8; i++) driver.Tick();
            Check(packets.Count == 8, "real renderer may reuse its four descriptors before playback catches up");
            for (int i = 0; i < 8; i++)
                Check(Array.TrueForAll(packets[i], sample => sample == 1000 + i),
                      "each reused renderer descriptor must retain its own queued PCM snapshot");
        }
        packets.Clear();
        using (var driver = new NXBridgeAudioDriver(pcm => packets.Add(pcm.ToArray()), false))
        using (var renderer = new HardwareDeviceImpl(driver, 2, 48000))
        using (var entered = new ManualResetEventSlim())
        using (var finished = new ManualResetEventSlim())
        {
            // A real renderer can outpace the consumer during shader compilation.
            // 16 five-ms packets are 80 ms. Packet 17 must wait: the hard
            // 64-descriptor capacity is not an acceptable steady latency target.
            var samples = new short[480];
            for (int i = 1; i <= 16; i++)
            {
                Array.Fill(samples, (short)i);
                renderer.AppendBuffer(samples, 2);
            }
            Exception error = null;
            var producer = new Thread(() => {
                entered.Set();
                try { Array.Fill(samples, (short)17); renderer.AppendBuffer(samples, 2); }
                catch (Exception ex) { error = ex; }
                finally { finished.Set(); }
            }) { IsBackground = true };
            producer.Start();
            Check(entered.Wait(2000), "renderer producer entered");
            bool returnedEarly = finished.Wait(100);
            driver.Tick();
            Check(producer.Join(2000), "consumption must unblock the renderer");
            Check(!returnedEarly && error == null, "full active renderer queue must apply backpressure, not throw");
            Check(driver.GetStatistics().PeakQueuedFrames <= 3840, "renderer queue lead must stay at most 80 ms");
            for (int i = 1; i < 17; i++) driver.Tick();
            Check(packets.Count == 17, "bounded queue must eventually deliver every renderer packet");
            for (int i = 0; i < 17; i++)
                Check(Array.TrueForAll(packets[i], sample => sample == i + 1),
                      "backpressure must preserve the exact PCM order through descriptor reuse");
        }
        packets.Clear();
        using (var driver = new NXBridgeAudioDriver(pcm => packets.Add(pcm.ToArray()), false))
        using (var session = Open(driver))
        {
            // A larger legitimate AudioOut packet can exceed the renderer lead
            // target. It enters an empty queue intact and completes after every
            // sample is consumed, not when it is admitted to the queue.
            var large = Buffer(4800, 7); session.Start(); session.QueueBuffer(large);
            for (int i = 0; i < 19; i++) driver.Tick();
            Check(!session.WasBufferFullyConsumed(large) && session.GetPlayedSampleCount() == 4560,
                  "large copied packet remains owned until its final sample");
            driver.Tick();
            Check(session.WasBufferFullyConsumed(large) && packets.Count == 20 &&
                  packets.TrueForAll(p => Array.TrueForAll(p, x => x == 7)),
                  "valid packet over the lead target must be delivered intact");
        }
        packets.Clear();
        using (var driver = new NXBridgeAudioDriver(pcm => packets.Add(pcm.ToArray()), false))
        using (var session = Open(driver))
        {
            var reused = Buffer(240, 1000);
            session.QueueBuffer(reused);
            MemoryMarshal.Cast<byte, short>(reused.Data.AsSpan()).Fill(2000);
            session.QueueBuffer(reused);
            Array.Clear(reused.Data);
            session.Start(); driver.Tick();
            Check(packets[0][0] == 1000 && !session.WasBufferFullyConsumed(reused),
                  "first copied submission must not complete the later reuse");
            driver.Tick();
            Check(packets[1][0] == 2000 && session.WasBufferFullyConsumed(reused),
                  "copied descriptor completes after its last queued submission");
        }
        using (var driver = new NXBridgeAudioDriver(_ => {}, false))
        using (var session = Open(driver))
        {
            var guest = new AudioBuffer { DataPointer = 1, DataSize = 4 };
            session.QueueBuffer(guest);
            Reject<InvalidOperationException>(() => session.QueueBuffer(guest), "borrowed guest buffer cannot be submitted twice");
            guest.Data = new byte[4];
            Reject<InvalidOperationException>(() => session.QueueBuffer(guest), "changing a borrowed descriptor does not make its earlier submission owned");
        }
        packets.Clear();
        using (var driver = new NXBridgeAudioDriver(pcm => packets.Add(pcm.ToArray()), false))
        using (var first = Open(driver))
        using (var second = Open(driver))
        {
            first.QueueBuffer(Buffer(100, 20000)); second.QueueBuffer(Buffer(240, 20000));
            first.Start(); second.Start(); driver.Tick();
            Check(packets.Count == 1, "mix all output sessions into one clocked packet");
            Check(packets[0][0] == 32767 && packets[0][199] == 32767 && packets[0][200] == 20000,
                  "saturate overlaps and preserve the longer stream");
            Check(first.GetPlayedSampleCount() == 100 && second.GetPlayedSampleCount() == 240,
                  "each audio session has its own consumed frame count");
            Check(driver.GetStatistics().UnderrunFrames == 0 && driver.GetStatistics().SessionUnderrunFrames == 140,
                  "a partial/unused session is not an underrun when another session covers the mixed output");
            first.Dispose(); second.QueueBuffer(Buffer(240, -32768)); driver.Tick();
            Check(packets[1][0] == -32768, "closing one output must not stop another");
        }
        using (var driver = new NXBridgeAudioDriver(_ => {}, false))
        using (var session = Open(driver))
        {
            Reject<NotSupportedException>(() => driver.OpenDeviceSession(Direction.Input, null, SampleFormat.PcmInt16, 48000, 2), "input rejected");
            Reject<NotSupportedException>(() => driver.OpenDeviceSession(Direction.Output, null, SampleFormat.PcmFloat, 48000, 2), "float requires compat conversion");
            Reject<InvalidOperationException>(() => session.QueueBuffer(new AudioBuffer { Data = new byte[3] }), "partial frame rejected");
            var buffer = Buffer(240, 1); session.QueueBuffer(buffer);
            for (int i = 1; i < NXBridgeAudioDriver.MaxBuffers; i++) session.QueueBuffer(Buffer(1, 1));
            Reject<InvalidOperationException>(() => session.QueueBuffer(Buffer(1, 1)), "metadata queue bounded");
            session.Start(); driver.Tick(); driver.Tick();
            session.QueueBuffer(Buffer(240, 2));
            Check(session.GetPlayedSampleCount() == 303, "queue capacity recovered after consumption");
        }
        using (var driver = new NXBridgeAudioDriver(_ => {}, false))
        using (var session = Open(driver))
        {
            session.QueueBuffer(Buffer(NXBridgeAudioDriver.MaxBytes / 4, 0));
            Reject<InvalidOperationException>(() => session.QueueBuffer(Buffer(1, 0)), "sample queue bounded");
        }
        foreach (bool closeDriver in new[] { false, true })
        using (var driver = new NXBridgeAudioDriver(_ => {}, false))
        using (var session = Open(driver))
        using (var entered = new ManualResetEventSlim())
        using (var finished = new ManualResetEventSlim())
        {
            // Byte capacity is independent of descriptor capacity. Closing
            // either owner must also wake a blocked producer, without playback.
            var full = Buffer(NXBridgeAudioDriver.MaxBytes / 4, 17);
            session.QueueBuffer(full); session.Start();
            Exception error = null;
            var producer = new Thread(() => {
                entered.Set();
                try { session.QueueBuffer(Buffer(1, 19)); }
                catch (Exception ex) { error = ex; }
                finally { finished.Set(); }
            }) { IsBackground = true };
            producer.Start();
            Check(entered.Wait(2000), "byte-capacity producer entered");
            bool returnedEarly = finished.Wait(100);
            if (closeDriver) driver.Dispose(); else session.Dispose();
            Check(producer.Join(2000), "closing must wake a capacity waiter");
            Check(!returnedEarly && error is ObjectDisposedException,
                  "full byte queue must wait until the owner closes");
            Check(session.GetPlayedSampleCount() == 0, "cancellation must not pretend queued PCM was played");
        }
        int one = 0, two = 0;
        using (var a = new NXBridgeAudioDriver(_ => one++, false))
        using (var b = new NXBridgeAudioDriver(_ => two++, false))
        using (var stream = Open(a))
        {
            stream.QueueBuffer(Buffer(240, 1)); stream.Start(); a.Tick(); b.Tick();
            Check(one == 1 && two == 0, "driver instances do not share audio");
            a.Dispose(); a.Dispose(); a.Tick();
            Check(one == 1, "dispose is idempotent and stops callbacks");
        }
        using (var driver = new NXBridgeAudioDriver(_ => {}, false))
        using (var session = Open(driver))
        {
            var reusable = Buffer(240, 9); session.Start();
            for (int i = 0; i < 100; i++) { session.QueueBuffer(reusable); driver.Tick(); }
            long allocated = GC.GetAllocatedBytesForCurrentThread();
            for (int i = 0; i < 2000; i++) { session.QueueBuffer(reusable); driver.Tick(); }
            allocated = GC.GetAllocatedBytesForCurrentThread() - allocated;
            Check(allocated == 0, "steady audio enqueue/mix/diagnostics must not allocate per packet: " + allocated);
        }
        long firstTime = 0, lastTime = 0; int callbacks = 0;
        using (var ready = new ManualResetEventSlim())
        {
            var driver = new NXBridgeAudioDriver(_ => {
                long now = Stopwatch.GetTimestamp();
                if (Interlocked.Increment(ref callbacks) == 1) firstTime = now;
                lastTime = now;
                if (callbacks >= 4) ready.Set();
            }, true);
            using (var session = Open(driver))
            {
                session.QueueBuffer(Buffer(2400, 1)); session.Start();
                Check(ready.Wait(2000), "real consumer must produce audio");
                driver.Dispose();
                Check((lastTime - firstTime) * 1000.0 / Stopwatch.Frequency >= 8,
                      "consumer must pace audio rather than draining immediately");
                int stopped = callbacks; Thread.Sleep(20);
                Check(callbacks == stopped, "worker joined before disposal returns");
            }
        }
        Console.WriteLine($"PASS: {checks} S2Arm audio checks against the pinned Ryujinx audio implementation");
        return 0;
    }
}
