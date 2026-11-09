import com.limelight.binding.input.virtual_controller.FloatingStickMath;

public class FloatingStickMathTest {
    private static void near(float actual, double expected) {
        if (Math.abs(actual - expected) > 0.0001) throw new AssertionError(actual + " != " + expected);
    }
    public static void main(String[] args) {
        float[] zero = FloatingStickMath.axes(0, 0, 100);
        near(zero[0], 0); near(zero[1], 0);
        float[] dead = FloatingStickMath.axes(5, 0, 100);
        near(dead[0], 0); near(dead[1], 0);
        float[] up = FloatingStickMath.axes(0, -100, 100);
        near(up[0], 0); near(up[1], 1);
        float[] left = FloatingStickMath.axes(-200, 0, 100);
        near(left[0], -1); near(left[1], 0);
        float[] diagonal = FloatingStickMath.axes(100, 100, 100);
        near(diagonal[0], Math.sqrt(0.5)); near(diagonal[1], -Math.sqrt(0.5));
        float[] half = FloatingStickMath.axes(56, 0, 100);
        near(half[0], 0.5); near(half[1], 0);
        float[] invalid = FloatingStickMath.axes(1, 1, 0);
        near(invalid[0], 0); near(invalid[1], 0);
        float[] earlySaturation = FloatingStickMath.axes(80, 0, 100, 0.1f, 0.8f, 1f);
        near(earlySaturation[0], 1);
        float[] fineControl = FloatingStickMath.axes(55, 0, 100, 0.1f, 1f, 2f);
        near(fineControl[0], 0.25);
        float[] badCurve = FloatingStickMath.axes(50, 0, 100, 0.5f, 0.5f, 1f);
        near(badCurve[0], 0); near(badCurve[1], 0);
        float[] notFinite = FloatingStickMath.axes(Float.NaN, 0, 100, 0.1f, 1f, 1f);
        near(notFinite[0], 0); near(notFinite[1], 0);
        if (FloatingStickMath.validCurve(0.1f, 1f, Float.POSITIVE_INFINITY)) throw new AssertionError("infinite exponent");
        if (!FloatingStickMath.validVisualScale(0.6f) || !FloatingStickMath.validVisualScale(1.5f) ||
                FloatingStickMath.validVisualScale(0.59f) || FloatingStickMath.validVisualScale(1.51f) ||
                FloatingStickMath.validVisualScale(Float.NaN)) throw new AssertionError("visual scale limits");
        System.out.println("PASS: floating stick axes, dead zone, clamping and direction");
    }
}
