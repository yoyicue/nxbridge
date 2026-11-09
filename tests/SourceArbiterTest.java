import com.limelight.binding.input.SourceArbiter;

public class SourceArbiterTest {
    private static void yes(boolean value) { if (!value) throw new AssertionError(); }
    private static void no(boolean value) { if (value) throw new AssertionError(); }
    public static void main(String[] args) {
        SourceArbiter a = new SourceArbiter();
        yes(a.owner() == SourceArbiter.Source.TOUCH);
        no(a.physicalUpdate(false, true)); // noise / neutral cannot steal control
        yes(a.physicalUpdate(true, false));
        yes(a.owner() == SourceArbiter.Source.PHYSICAL);
        long first = a.generation();
        yes(a.claimTouch(false)); // existing physical hold is now blocked
        no(a.physicalUpdate(true, false)); // retransmission cannot reclaim
        no(a.physicalUpdate(false, true)); // release rearms, does not claim
        yes(a.physicalUpdate(true, false)); // fresh physical action claims
        yes(a.generation() > first);
        a.reset(false); // editing / cancellation blocks old physical hold
        no(a.physicalUpdate(true, false));
        no(a.physicalUpdate(false, true));
        yes(a.physicalUpdate(true, false));
        System.out.println("PASS: source claims, neutral rearming and stale state rejection");
    }
}
