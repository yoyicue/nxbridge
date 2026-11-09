import com.limelight.binding.input.virtual_controller.LayoutProjection;
import com.limelight.binding.input.virtual_controller.LayoutSafeArea;
import java.util.Arrays;

public final class LayoutProjectionTest {
    private static void rect(int[] got, int... want) {
        if (!Arrays.equals(got, want)) throw new AssertionError(Arrays.toString(got) + " expected " + Arrays.toString(want));
    }
    private static void rejected(Runnable f) {
        try { f.run(); throw new AssertionError("expected invalid projection"); }
        catch (IllegalArgumentException expected) { }
    }
    public static void main(String[] args) {
        LayoutSafeArea source = LayoutSafeArea.fromWindow(2560, 1552, 2560, 1552, 0, 0, 48, 0, 48, 0);
        LayoutSafeArea target = LayoutSafeArea.fromWindow(1600, 900, 1600, 900, 0, 0, 30, 20, 30, 20);
        double[] button = LayoutProjection.encode(source, false, 208, 350, 180, 160);
        rect(LayoutProjection.project(2, source, 2560, 1552, false, button[0], button[1], button[2], button[3]), 208, 350, 180, 160);
        int[] resized = LayoutProjection.project(2, target, 1600, 900, false, button[0], button[1], button[2], button[3]);
        if (resized[2] != 100 || resized[3] != 89) throw new AssertionError("short-side scale " + Arrays.toString(resized));
        double center = (resized[0] + resized[2]/2.0 - target.left)/(target.right-target.left);
        if (Math.abs(center-button[0]) > .001) throw new AssertionError("center placement");
        double[] zone = LayoutProjection.encode(source, true, 48, 500, 700, 900);
        rect(LayoutProjection.project(2, source, 2560, 1552, true, zone[0], zone[1], zone[2], zone[3]), 48, 500, 700, 900);
        int[] z = LayoutProjection.project(2, target, 1600, 900, true, zone[0], zone[1], zone[2], zone[3]);
        if (z[0] != target.left || z[2] != 438 || z[3] != 499) throw new AssertionError("zone scales with safe area " + Arrays.toString(z));
        rect(LayoutProjection.project(1, target, 1600, 900, false, .1, .2, .2, .1), 160, 180, 320, 90);
        int[] raw = LayoutProjection.project(2, target, 1600, 900, false, 0, 0, .1, .1);
        int[] clamped = target.constrain(raw[0], raw[1], raw[2], raw[3]);
        if (clamped[0] != target.left || clamped[1] != target.top || clamped[2] != raw[2] || clamped[3] != raw[3]) throw new AssertionError("edge clamp");
        rejected(() -> LayoutProjection.project(2, target, 1600, 900, false, .5, .5, 100, .1));
        rejected(() -> LayoutProjection.project(2, target, 1600, 900, true, .9, .5, .2, .1));
        rejected(() -> LayoutProjection.project(2, target, 1600, 900, false, Double.NaN, .5, .1, .1));
        rect(LayoutProjection.project(3, source, 2560, 1552, false, button[0], button[1], button[2], button[3]), 208, 350, 180, 160);
        rect(LayoutProjection.project(4, source, 2560, 1552, false, button[0], button[1], button[2], button[3]), 208, 350, 180, 160);
        rect(LayoutProjection.project(4, source, 2560, 1552, true, zone[0], zone[1], zone[2], zone[3]), 48, 500, 700, 900);
        rejected(() -> LayoutProjection.project(5, target, 1600, 900, false, .5, .5, .1, .1));
        System.out.println("PASS: schema 1 legacy, schema 2/3/4 safe-area projection and bounds");
    }
}
