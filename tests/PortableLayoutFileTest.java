import com.limelight.binding.input.virtual_controller.PortableLayoutFile;
import java.io.ByteArrayInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.util.Arrays;

public final class PortableLayoutFileTest {
    private static void expectFailure(Throwing action) throws Exception {
        try { action.run(); throw new AssertionError("expected IOException"); }
        catch (IOException expected) { }
    }
    private interface Throwing { void run() throws Exception; }
    public static void main(String[] args) throws Exception {
        byte[] json = "{\"标题\":\"布局\"}".getBytes(StandardCharsets.UTF_8);
        if (!Arrays.equals(json, PortableLayoutFile.read(new ByteArrayInputStream(json)))) throw new AssertionError("read");
        if (!PortableLayoutFile.decode(json).contains("布局")) throw new AssertionError("UTF-8");
        byte[] max = new byte[PortableLayoutFile.MAX_BYTES];
        if (PortableLayoutFile.read(new ByteArrayInputStream(max)).length != max.length) throw new AssertionError("limit");
        expectFailure(() -> PortableLayoutFile.read(new ByteArrayInputStream(new byte[max.length + 1])));
        expectFailure(() -> PortableLayoutFile.decode(new byte[] {(byte)0xc3, (byte)0x28}));
        expectFailure(() -> PortableLayoutFile.decode(new byte[0]));
        expectFailure(() -> PortableLayoutFile.read(null));
        InputStream zeroThenByte = new InputStream() {
            private int pos;
            @Override public int read(byte[] b, int o, int l) { return pos == 0 ? 0 : -1; }
            @Override public int read() { return pos++ == 0 ? 'x' : -1; }
        };
        if (!"x".equals(PortableLayoutFile.decode(PortableLayoutFile.read(zeroThenByte)))) throw new AssertionError("zero-length read");
        System.out.println("PASS: bounded layout file reads and strict UTF-8");
    }
}
