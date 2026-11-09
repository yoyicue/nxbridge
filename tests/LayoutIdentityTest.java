import com.limelight.binding.input.virtual_controller.LayoutIdentity;

public class LayoutIdentityTest {
    public static void main(String[] args) {
        String a = LayoutIdentity.key("01007EF00011E000", "host-a", 5, 2, true);
        if (!a.equals(LayoutIdentity.key("01007EF00011E000", "host-b", 19, 2, true))) throw new AssertionError("stable game identity");
        if (a.equals(LayoutIdentity.key("0100F2C0115B6000", "host-a", 5, 2, true))) throw new AssertionError("distinct games");
        if (a.equals(LayoutIdentity.key("01007EF00011E000", "host-a", 5, 1, true))) throw new AssertionError("orientation");
        if (a.equals(LayoutIdentity.key("01007EF00011E000", "host-a", 5, 2, false))) throw new AssertionError("stick mode");
        if (LayoutIdentity.key("", "host-a", 5, 2, true).equals(LayoutIdentity.key("", "host-b", 5, 2, true))) throw new AssertionError("legacy hosts");
        try { LayoutIdentity.key("../bad", "", 0, 2, true); throw new AssertionError("invalid identity"); }
        catch (IllegalArgumentException expected) { }
        if (!a.equals(LayoutIdentity.key("01007EF00011E000", "host-a", 5, 2, true, false, false))) throw new AssertionError("existing default identity preserved");
        if (a.equals(LayoutIdentity.key("01007EF00011E000", "host-a", 5, 2, true, true, false))) throw new AssertionError("stick clicks mode isolation");
        if (a.equals(LayoutIdentity.key("01007EF00011E000", "host-a", 5, 2, true, false, true))) throw new AssertionError("guide mode isolation");
        byte[] invalid = new byte[] {1, 2};
        if (!LayoutIdentity.sameTransferBase(a, 0, null, a, 0, null)) throw new AssertionError("matching empty layout");
        if (LayoutIdentity.sameTransferBase(a, 0, null, LayoutIdentity.key("0100F2C0115B6000", "host-a", 5, 2, true), 0, null)) throw new AssertionError("cross-game transfer");
        if (LayoutIdentity.sameTransferBase(a, 0, null, LayoutIdentity.key("01007EF00011E000", "host-a", 5, 1, true), 0, null)) throw new AssertionError("cross-orientation transfer");
        if (LayoutIdentity.sameTransferBase(a, 0, null, LayoutIdentity.key("01007EF00011E000", "host-a", 5, 2, false), 0, null)) throw new AssertionError("cross-stick-mode transfer");
        if (LayoutIdentity.sameTransferBase(null, 0, null, a, 0, null)) throw new AssertionError("missing saved identity accepted");
        if (LayoutIdentity.sameTransferBase(a, 1, null, a, 2, null)) throw new AssertionError("stale revision accepted");
        if (LayoutIdentity.sameTransferBase(a, 0, invalid, a, 0, new byte[] {1, 3})) throw new AssertionError("changed broken file accepted");
        if (!LayoutIdentity.sameTransferBase(a, 0, invalid, a, 0, new byte[] {1, 2})) throw new AssertionError("matching fingerprint rejected");
        System.out.println("PASS: game layout identity and transfer context isolation");
    }
}
