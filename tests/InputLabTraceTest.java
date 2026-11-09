import com.limelight.binding.input.InputLabTrace;
import java.util.List;

public class InputLabTraceTest {
    public static void main(String[] args) {
        InputLabTrace trace = new InputLabTrace(2);
        trace.record(100, "touch", "down"); trace.record(110, "input", "A"); trace.record(120, "input", "neutral");
        List<InputLabTrace.Entry> result = trace.snapshot();
        if (result.size() != 2 || result.get(0).sequence != 2 || result.get(1).monotonicNanos != 120) throw new AssertionError("bounded order");
        trace.clear(); trace.record(130, "touch", "up");
        if (trace.snapshot().get(0).sequence != 4) throw new AssertionError("clear must not reuse sequence");
        try { new InputLabTrace(0); throw new AssertionError("capacity"); } catch (IllegalArgumentException expected) { }
        System.out.println("PASS: bounded chronological input lab trace");
    }
}
