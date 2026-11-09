import com.limelight.binding.input.virtual_controller.LayoutGeometry;

public class LayoutGeometryTest {
    public static void main(String[] args) {
        if (!LayoutGeometry.overlaps(10, 10, 20, 20, 29, 29, 20, 20)) throw new AssertionError("one-pixel overlap");
        if (LayoutGeometry.overlaps(10, 10, 20, 20, 30, 10, 20, 20)) throw new AssertionError("adjacent targets");
        if (LayoutGeometry.overlaps(10, 10, 20, 20, 10, 30, 20, 20)) throw new AssertionError("adjacent rows");
        if (!LayoutGeometry.overlaps(0, 0, 100, 100, 20, 20, 10, 10)) throw new AssertionError("contained target");
        if (!LayoutGeometry.overlaps(Integer.MAX_VALUE - 5, 0, 10, 10, Integer.MAX_VALUE - 2, 0, 10, 10)) throw new AssertionError("integer overflow");
        System.out.println("PASS: projected target overlap");
    }
}
