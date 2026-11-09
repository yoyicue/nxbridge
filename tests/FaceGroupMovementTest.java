import com.limelight.binding.input.virtual_controller.FaceGroupMovement;
import java.util.Arrays;

public final class FaceGroupMovementTest {
    private static void check(int[] actual, int x, int y) {
        if (!Arrays.equals(actual, new int[]{x, y})) throw new AssertionError(Arrays.toString(actual) + " != " + x + "," + y);
    }
    public static void main(String[] args) {
        int[][] group = {{150, 200, 40, 40}, {200, 150, 40, 40}, {250, 200, 40, 40}, {200, 250, 40, 40}};
        int[][] obstacles = {{320, 150, 40, 140}};
        check(FaceGroupMovement.clamp(group, obstacles, 0, 0, 400, 400, -50, 20), -50, 20);
        check(FaceGroupMovement.clamp(group, new int[0][], 0, 0, 400, 400, -999, 999), -150, 110);
        check(FaceGroupMovement.clamp(group, obstacles, 0, 0, 400, 400, 30, 0), 30, 0); // touching edge is allowed
        check(FaceGroupMovement.clamp(group, obstacles, 0, 0, 400, 400, 31, 0), 0, 0); // no partial group move
        check(FaceGroupMovement.clamp(group, obstacles, 0, 0, 400, 400, Integer.MAX_VALUE, 0), 0, 0);
        check(FaceGroupMovement.clamp(new int[0][], obstacles, 0, 0, 400, 400, 1, 1), 0, 0);
        int[][] shoulderPair = {{40, 50, 50, 40}, {100, 50, 50, 40}};
        int[][] otherButtons = {{40, 180, 50, 40}, {250, 50, 50, 40}};
        check(FaceGroupMovement.clamp(shoulderPair, otherButtons, 0, 0, 400, 400, 0, 90), 0, 90);
        check(FaceGroupMovement.clamp(shoulderPair, otherButtons, 0, 0, 400, 400, 0, 91), 0, 0);
        check(FaceGroupMovement.clamp(shoulderPair, new int[0][], 0, 0, 400, 400, -100, 0), -40, 0);
        System.out.println("PASS: whole-group movement, safe-area clamp and obstacle refusal");
    }
}
