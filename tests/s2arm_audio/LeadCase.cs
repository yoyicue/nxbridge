using System;
using System.Collections.Generic;
using System.Threading;
using Ryujinx.Audio.Integration;
using S2A.Apple.Audio;

static class LeadCase
{
    // This runs unchanged against the preceding driver as a negative control.
    static int Main()
    {
        var output = new List<short[]>();
        using var driver = new NXBridgeAudioDriver(pcm => output.Add(pcm.ToArray()), false);
        using var renderer = new HardwareDeviceImpl(driver, 2, 48000);
        var samples = new short[480];
        for (int i = 1; i <= 16; i++) { Array.Fill(samples, (short)i); renderer.AppendBuffer(samples, 2); }
        using var entered = new ManualResetEventSlim();
        using var finished = new ManualResetEventSlim();
        Exception error = null;
        var producer = new Thread(() => {
            entered.Set();
            try { Array.Fill(samples, (short)17); renderer.AppendBuffer(samples, 2); }
            catch (Exception e) { error = e; }
            finally { finished.Set(); }
        }) { IsBackground = true };
        producer.Start();
        if (!entered.Wait(2000)) throw new Exception("producer never entered");
        bool early = finished.Wait(100);
        driver.Tick();
        if (!producer.Join(2000)) throw new Exception("consumer did not wake the producer");
        for (int i = 1; i < 17; i++) driver.Tick();
        if (early || error != null || output.Count != 17)
        {
            Console.Error.WriteLine("FAIL: 80 ms admission / complete delivery");
            return 1;
        }
        for (int i = 0; i < output.Count; i++)
            foreach (short sample in output[i]) if (sample != i + 1) throw new Exception("PCM dropped or reordered");
        Console.WriteLine("PASS: renderer waits at 80 ms, wakes after real consumption, all 17 packets retained in order");
        return 0;
    }
}
