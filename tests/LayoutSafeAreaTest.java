import com.limelight.binding.input.virtual_controller.LayoutSafeArea;
import java.util.Arrays;

public class LayoutSafeAreaTest {
    private static void rect(int[] actual, int... expected) {
        if (!Arrays.equals(actual, expected)) throw new AssertionError(Arrays.toString(actual));
    }
    public static void main(String[] args) {
        // The container already excludes the bottom navigation bar: do not subtract it twice.
        LayoutSafeArea a = LayoutSafeArea.fromWindow(2560, 1552, 2560, 1600, 0, 0, 48, 0, 48, 48);
        if (a.left != 48 || a.top != 0 || a.right != 2512 || a.bottom != 1552) throw new AssertionError("window intersection");
        rect(a.constrain(0, 400, 800, 1000), 48, 400, 800, 1000);
        rect(a.constrain(2400, 1400, 200, 200), 2312, 1352, 200, 200);
        if (a.contains(0, 0, 100, 100) || !a.contains(48, 0, 100, 100)) throw new AssertionError("containment");
        LayoutSafeArea insetContainer = LayoutSafeArea.fromWindow(1000, 600, 1200, 800, 100, 100, 50, 60, 150, 120);
        if (insetContainer.left != 0 || insetContainer.top != 0 || insetContainer.right != 950 || insetContainer.bottom != 580) throw new AssertionError("container offset");
        rect(insetContainer.constrain(0, 0, 2000, 2000), 0, 0, 950, 580);
        LayoutSafeArea clipped = a.intersect(0, 0, 2400, 1500);
        if (clipped.left != 48 || clipped.right != 2400 || clipped.bottom != 1500) throw new AssertionError("parent clipping");
        try { LayoutSafeArea.fromWindow(100,100,100,100,0,0,60,0,60,0); throw new AssertionError("empty area"); }
        catch (IllegalArgumentException expected) { }
        System.out.println("PASS: safe window intersection, containment and projection");
    }
}
