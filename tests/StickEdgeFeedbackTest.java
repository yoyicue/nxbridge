import com.limelight.binding.input.virtual_controller.StickEdgeFeedback;

public final class StickEdgeFeedbackTest {
    private static void expect(boolean condition, String label) { if (!condition) throw new AssertionError(label); }
    public static void main(String[] args) {
        StickEdgeFeedback feedback = new StickEdgeFeedback();
        expect(!feedback.onMove(false, 1, 0), "disabled");
        expect(!feedback.onMove(true, .5f, 0), "center");
        expect(feedback.onMove(true, 1, 0), "first edge");
        expect(!feedback.onMove(true, 1, 0), "held does not repeat");
        expect(!feedback.onMove(true, 0, 0) && !feedback.onMove(true, 0, 1), "re-entry does not repeat");
        feedback.reset();
        expect(!feedback.onMove(true, Float.NaN, 1), "invalid axis");
        expect(feedback.onMove(true, .707107f, .707107f), "next touch at diagonal edge");
        feedback.reset();
        expect(feedback.onMove(true, 0, -1), "new touch after cancel");
        System.out.println("PASS: optional once-per-touch edge feedback");
    }
}
