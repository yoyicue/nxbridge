import com.limelight.binding.input.virtual_controller.LayoutRecoveryFiles;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Arrays;

public class LayoutRecoveryFilesTest {
    public static void main(String[] args) throws Exception {
        Path dir = Files.createTempDirectory("nxb-layout-recovery-");
        try {
            Path source = dir.resolve("profile.json"); byte[] invalid = "broken {".getBytes("UTF-8");
            Files.write(source, invalid); byte[] expected = LayoutRecoveryFiles.fingerprint(source.toFile());
            Path backup = LayoutRecoveryFiles.backupIfUnchanged(source.toFile(), expected).toPath();
            if (!Arrays.equals(Files.readAllBytes(backup), invalid) || !Arrays.equals(Files.readAllBytes(source), invalid)) throw new AssertionError("preserve original and backup");
            String[] names = LayoutRecoveryFiles.backupNames(source.toFile());
            if (names.length != 1 || !names[0].equals(backup.getFileName().toString())) throw new AssertionError("profile backup discovery");
            byte[] backupHash = LayoutRecoveryFiles.backupFingerprint(source.toFile(), names[0]);
            if (!Arrays.equals(LayoutRecoveryFiles.readBackupIfUnchanged(source.toFile(), names[0], backupHash), invalid)) throw new AssertionError("exact export bytes");
            try { LayoutRecoveryFiles.readBackupIfUnchanged(source.toFile(), "../" + names[0], backupHash); throw new AssertionError("path traversal accepted"); }
            catch (java.io.IOException good) { }
            Files.write(backup, "modified backup".getBytes("UTF-8"));
            try { LayoutRecoveryFiles.readBackupIfUnchanged(source.toFile(), names[0], backupHash); throw new AssertionError("changed backup exported"); }
            catch (LayoutRecoveryFiles.Changed good) { }
            Path other = dir.resolve("other.json");
            if (LayoutRecoveryFiles.backupNames(other.toFile()).length != 0) throw new AssertionError("different profile exposed");
            Path symlink = dir.resolve(source.getFileName().toString() + ".invalid-link.backup");
            Files.createSymbolicLink(symlink, backup.getFileName());
            if (LayoutRecoveryFiles.backupNames(source.toFile()).length != 1) throw new AssertionError("symlink listed");
            try { LayoutRecoveryFiles.backupFingerprint(source.toFile(), symlink.getFileName().toString()); throw new AssertionError("symlink followed"); }
            catch (java.io.IOException good) { }
            try (java.io.OutputStream out = Files.newOutputStream(backup)) { byte[] block = new byte[8192]; for (int i = 0; i < 1025; i++) out.write(block); }
            try { LayoutRecoveryFiles.readBackupIfUnchanged(source.toFile(), names[0], backupHash); throw new AssertionError("oversized backup exported"); }
            catch (java.io.IOException good) { }
            Files.write(source, "changed".getBytes("UTF-8"));
            try { LayoutRecoveryFiles.backupIfUnchanged(source.toFile(), expected); throw new AssertionError("stale recovery accepted"); }
            catch (LayoutRecoveryFiles.Changed good) { }
            if (!"changed".equals(new String(Files.readAllBytes(source), "UTF-8"))) throw new AssertionError("changed file overwritten");
            try (java.io.OutputStream out = Files.newOutputStream(source)) { byte[] block = new byte[8192]; for (int i = 0; i < 1025; i++) out.write(block); }
            try { LayoutRecoveryFiles.fingerprint(source.toFile()); throw new AssertionError("oversized recovery accepted"); }
            catch (java.io.IOException good) { }
            System.out.println("PASS: exact backup/export, profile isolation, traversal, changed source and size bound");
        } finally {
            try (java.util.stream.Stream<Path> paths = Files.walk(dir)) {
                paths.sorted(java.util.Comparator.reverseOrder()).forEach(p -> { try { Files.delete(p); } catch (java.io.IOException e) { throw new RuntimeException(e); } });
            }
        }
    }
}
