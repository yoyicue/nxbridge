import com.limelight.binding.input.virtual_controller.LayoutCommitGuard;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.atomic.AtomicBoolean;

public class LayoutCommitGuardTest {
    public static void main(String[] args) throws Exception {
        LayoutCommitGuard guard = new LayoutCommitGuard();
        long old = guard.snapshot(); guard.invalidate();
        AtomicBoolean wrote = new AtomicBoolean();
        try { guard.commit(old, () -> wrote.set(true)); throw new AssertionError("stale commit accepted"); }
        catch (LayoutCommitGuard.StaleDisplay expected) { }
        if (wrote.get()) throw new AssertionError("stale action ran");
        long current = guard.snapshot();
        CountDownLatch committing = new CountDownLatch(1), release = new CountDownLatch(1), invalidating = new CountDownLatch(1);
        AtomicBoolean invalidated = new AtomicBoolean();
        Thread commit = new Thread(() -> {
            try { guard.commit(current, () -> { committing.countDown(); try { release.await(); } catch (InterruptedException e) { throw new java.io.IOException(e); } wrote.set(true); }); }
            catch (Exception e) { throw new RuntimeException(e); }
        });
        commit.start(); committing.await();
        Thread update = new Thread(() -> { invalidating.countDown(); guard.invalidate(); invalidated.set(true); });
        update.start(); invalidating.await();
        Thread.sleep(30);
        if (invalidated.get()) throw new AssertionError("invalidation overlapped commit");
        release.countDown(); commit.join(); update.join();
        if (!wrote.get() || !invalidated.get() || guard.snapshot() == current) throw new AssertionError("ordered commit and invalidation");
        System.out.println("PASS: stale display rejected and commit/invalidation ordered");
    }
}
