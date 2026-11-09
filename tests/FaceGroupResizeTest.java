import com.limelight.binding.input.virtual_controller.FaceGroupResize;
import java.util.Arrays;

public final class FaceGroupResizeTest {
    private static final int[][] GROUP = {{150, 200, 40, 40}, {200, 150, 40, 40}, {250, 200, 40, 40}, {200, 250, 40, 40}};
    private static void expect(boolean ok, String label) { if (!ok) throw new AssertionError(label); }
    public static void main(String[] args) {
        int[][] none = new int[0][];
        expect(Arrays.deepEquals(GROUP, FaceGroupResize.plan(GROUP, none, 0, 0, 400, 400, 30, 1.0)), "identity");
        int[][] larger = FaceGroupResize.plan(GROUP, none, 0, 0, 400, 400, 30, 1.2);
        expect(larger != null && larger[0][2] == 48 && larger[2][0] - larger[0][0] == 120, "uniform enlargement");
        int[][] smaller = FaceGroupResize.plan(GROUP, none, 0, 0, 400, 400, 30, .75);
        expect(smaller != null && smaller[0][2] == 30 && smaller[2][0] - smaller[0][0] == 75, "minimum scale");
        expect(FaceGroupResize.plan(GROUP, none, 0, 0, 400, 400, 30, .70) == null, "too small");
        expect(FaceGroupResize.plan(GROUP, none, 160, 0, 400, 400, 30, 1.2) == null, "safe boundary");
        expect(FaceGroupResize.plan(GROUP, new int[][]{{300, 200, 30, 40}}, 0, 0, 400, 400, 30, 1.2) == null, "obstacle");
        expect(FaceGroupResize.plan(GROUP, none, 0, 0, 400, 400, 30, Double.NaN) == null, "nonfinite scale");
        int[][] touching = {{100, 140, 40, 40}, {140, 100, 40, 40}, {180, 140, 40, 40}, {140, 180, 40, 40}};
        expect(FaceGroupResize.plan(touching, none, 0, 0, 400, 400, 30, 1.13) != null, "shared edges round consistently");
        System.out.println("PASS: uniform scaling, minimum targets, collision and bounds");
    }
}
