import com.limelight.binding.video.StartupFrameDetector;

public class StartupFrameDetectorTest {
    public static void main(String[] args) {
        int[] pixels = new int[576];
        if (StartupFrameDetector.hasVisibleContent(null) || StartupFrameDetector.hasVisibleContent(pixels)) throw new AssertionError("black frame");
        for (int i = 0; i < 5; i++) pixels[i] = 0xffffffff;
        if (StartupFrameDetector.hasVisibleContent(pixels)) throw new AssertionError("isolated highlights");
        pixels[5] = 0xff102030;
        if (!StartupFrameDetector.hasVisibleContent(pixels)) throw new AssertionError("visible content");
        System.out.println("PASS: startup black-frame detection");
    }
}
